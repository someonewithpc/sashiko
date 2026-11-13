// Copyright 2026 The Sashiko Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Claude Code sessions a later request can continue.
//!
//! The session runner sends a conversation's whole history on every turn. Fed
//! to a fresh `claude --print` as one block of text, that history never
//! matches the prompt cache, so every turn writes the whole conversation to
//! the cache again. A turn that continues a session with `--resume` sends only
//! the new messages, and Claude Code replays the rest as separate messages
//! whose prefix the cache already holds.
//!
//! A session is found by the history it holds: a request whose messages start
//! with the messages the session answered, followed by the reply it gave,
//! continues it.
//! `--fork-session` leaves the parent untouched, and the parent is kept until
//! its child is continued in turn, so a retry of the latest turn, or a second
//! branch from the same history, resumes the same parent.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tracing::warn;

use crate::ai::{AiMessage, AiRequest, AiRole};

pub(super) type PrefixKey = [u8; 32];

/// Sessions kept for continuation, oldest first out. A conversation holds two
/// at most, and nothing says when one has ended, so the last two of every
/// finished conversation stay until a later call finds them expired or this
/// pushes them out
const MAX_SESSIONS: usize = 256;

/// How long a session is kept after its last use, checked on every claude-cli
/// call. Claude Code writes the
/// prompt cache with a one-hour lifetime, and a resume after that writes the
/// whole conversation again, saving nothing over a new one, while the
/// transcripts hold the patch under review
const IDLE_LIFETIME: Duration = Duration::from_secs(60 * 60);

/// Working directories of sessions still alive, for removal on an exit that
/// runs no destructor
static LIVE_DIRS: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(Default::default);

/// A working directory and the transcripts Claude Code keeps for it.
///
/// Claude Code files a session under the directory it ran in, and `--resume`
/// looks it up there, so every turn of a conversation runs in the same one.
pub(super) struct SessionDir {
    cwd: PathBuf,
}

impl SessionDir {
    pub(super) fn new() -> std::io::Result<Self> {
        let cwd = tempfile::tempdir()?.keep();
        let cwd = std::fs::canonicalize(&cwd).unwrap_or(cwd);
        LIVE_DIRS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(cwd.clone());
        Ok(Self { cwd })
    }

    pub(super) fn path(&self) -> &Path {
        &self.cwd
    }
}

impl Drop for SessionDir {
    fn drop(&mut self) {
        LIVE_DIRS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.cwd);
        remove_dir_and_transcripts(&self.cwd);
    }
}

/// Removes every session directory still alive, with its transcripts.
///
/// For an exit through std::process::exit, which drops nothing.
pub fn remove_session_files() {
    let dirs = std::mem::take(&mut *LIVE_DIRS.lock().unwrap_or_else(|e| e.into_inner()));
    for cwd in dirs {
        remove_dir_and_transcripts(&cwd);
    }
}

fn remove_dir_and_transcripts(cwd: &Path) {
    remove_files(cwd, projects_dir().as_deref());
}

fn remove_files(cwd: &Path, projects: Option<&Path>) {
    remove_logged(cwd);
    if let Some(transcripts) = projects.and_then(|projects| transcripts_dir_in(projects, cwd)) {
        remove_logged(&transcripts);
    }
}

/// Removes a directory, warning when it stays: the transcripts in it hold
/// the patch under review
fn remove_logged(dir: &Path) {
    if let Err(e) = std::fs::remove_dir_all(dir)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        warn!(
            "Failed to remove claude-cli session files in {}: {e}",
            dir.display()
        );
    }
}

/// The projects directory of Claude Code's config, which holds a directory of
/// transcripts for every working directory a session ran in
fn projects_dir() -> Option<PathBuf> {
    let config = match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".claude"),
    };
    Some(config.join("projects"))
}

/// The directory in projects for cwd: the path with every character other
/// than an ASCII letter or digit replaced by a dash
fn transcripts_dir_in(projects: &Path, cwd: &Path) -> Option<PathBuf> {
    let name: String = cwd
        .to_str()?
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    Some(projects.join(name))
}

#[derive(Clone)]
pub(super) struct Session {
    pub(super) id: String,
    pub(super) dir: Arc<SessionDir>,
}

/// A request's place in a conversation
pub(super) struct Lineage {
    /// keys[k] identifies the request carrying the first k messages
    keys: Vec<PrefixKey>,
}

impl Lineage {
    /// Keys every prefix of the request's messages, together with what else
    /// shapes the conversation: the model, the effort, the system prompt,
    /// the tools and the response format
    pub(super) fn of(request: &AiRequest, model: &str, effort: Option<&str>) -> Self {
        let header = serde_json::to_vec(&(
            model,
            effort,
            &request.system,
            &request.tools,
            &request.response_format,
            &request.reasoning_effort,
        ))
        .unwrap_or_default();
        let mut key: PrefixKey = Sha256::digest(&header).into();
        let mut keys = Vec::with_capacity(request.messages.len() + 1);
        keys.push(key);
        for message in &request.messages {
            key = chain(key, message);
            keys.push(key);
        }
        Self { keys }
    }

    /// The key a session that gave reply to the whole request is kept under:
    /// that of a later request whose history holds both. Keying on the reply
    /// as well keeps two conversations that sent the same request apart
    pub(super) fn answered(&self, reply: &AiMessage) -> PrefixKey {
        chain(self.keys[self.keys.len() - 1], reply)
    }
}

fn chain(key: PrefixKey, message: &AiMessage) -> PrefixKey {
    let mut hasher = Sha256::new();
    hasher.update(key);
    hasher.update(serde_json::to_vec(message).unwrap_or_default());
    hasher.finalize().into()
}

/// What a request continues: the session, the key it was kept under and how
/// many of the request's messages it already holds
pub(super) struct Continuation {
    pub(super) session: Session,
    pub(super) parent: PrefixKey,
    pub(super) known: usize,
    /// Whether resuming this session failed before, since it last worked
    pub(super) failed_before: bool,
}

#[derive(Default)]
pub struct Sessions {
    inner: Mutex<SessionMap>,
}

#[derive(Default)]
struct SessionMap {
    by_key: HashMap<PrefixKey, Kept>,
    order: VecDeque<PrefixKey>,
    /// The session each kept session continued, while that one is kept
    parents: HashMap<PrefixKey, PrefixKey>,
}

struct Kept {
    session: Session,
    used: Instant,
    failed: bool,
}

impl SessionMap {
    fn remove(&mut self, key: PrefixKey, removed: &mut Vec<Session>) {
        if let Some(kept) = self.by_key.remove(&key) {
            removed.push(kept.session);
        }
        self.order.retain(|k| *k != key);
        self.parents.remove(&key);
    }

    fn expire(&mut self, now: Instant, removed: &mut Vec<Session>) {
        let idle: Vec<PrefixKey> = self
            .by_key
            .iter()
            .filter(|(_, kept)| now.duration_since(kept.used) > IDLE_LIFETIME)
            .map(|(key, _)| *key)
            .collect();
        for key in idle {
            self.remove(key, removed);
        }
    }
}

/// Drops a turn's reference to its directory. When it is the last one, for a
/// conversation no session was kept for, the files go on a blocking thread
pub(super) async fn release_dir(dir: Arc<SessionDir>) {
    if let Ok(dir) = Arc::try_unwrap(dir)
        && let Err(e) = tokio::task::spawn_blocking(move || drop(dir)).await
    {
        warn!("Failed to remove claude-cli session files: {e}");
    }
}

/// Drops sessions taken out of the map once its lock is released. The last
/// reference to a session's directory removes it and its transcripts, which
/// goes to a blocking thread rather than holding up the runtime
async fn release(removed: Vec<Session>) {
    if removed.is_empty() {
        return;
    }
    if let Err(e) = tokio::task::spawn_blocking(move || drop(removed)).await {
        warn!("Failed to remove claude-cli session files: {e}");
    }
}

impl Sessions {
    /// Finds the latest session this request continues. A session answered
    /// the first k messages when message k is the assistant's reply, and the
    /// request has at least one message after it
    pub(super) async fn find(
        &self,
        request: &AiRequest,
        lineage: &Lineage,
    ) -> Option<Continuation> {
        let now = Instant::now();
        let mut removed = Vec::new();
        let found = {
            let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            map.expire(now, &mut removed);
            let messages = &request.messages;
            (1..messages.len().saturating_sub(1)).rev().find_map(|k| {
                if messages[k].role != AiRole::Assistant {
                    return None;
                }
                let parent = lineage.keys[k + 1];
                map.by_key.get_mut(&parent).map(|kept| {
                    kept.used = now;
                    Continuation {
                        session: kept.session.clone(),
                        parent,
                        known: k + 1,
                        failed_before: kept.failed,
                    }
                })
            })
        };
        release(removed).await;
        found
    }

    /// Keeps the session that answered a request. The session it continued
    /// stays, for a retry of the same request, and the one before that goes,
    /// so a conversation holds two sessions at most
    pub(super) async fn keep(&self, key: PrefixKey, session: Session, parent: Option<PrefixKey>) {
        self.keep_at(Instant::now(), key, session, parent).await;
    }

    async fn keep_at(
        &self,
        now: Instant,
        key: PrefixKey,
        session: Session,
        parent: Option<PrefixKey>,
    ) {
        let mut removed = Vec::new();
        {
            let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            map.expire(now, &mut removed);
            if let Some(parent) = parent {
                if let Some(grandparent) = map.parents.get(&parent).copied() {
                    map.remove(grandparent, &mut removed);
                }
                if let Some(resumed) = map.by_key.get_mut(&parent) {
                    resumed.failed = false;
                }
                map.parents.insert(key, parent);
            }
            let kept = Kept {
                session,
                used: now,
                failed: false,
            };
            match map.by_key.insert(key, kept) {
                Some(replaced) => removed.push(replaced.session),
                None => map.order.push_back(key),
            }
            while map.order.len() > MAX_SESSIONS {
                if let Some(oldest) = map.order.pop_front() {
                    map.remove(oldest, &mut removed);
                }
            }
        }
        release(removed).await;
    }

    /// Notes that resuming a session failed, so the next failure gives up on it
    pub(super) fn mark_failed(&self, key: PrefixKey) {
        if let Some(kept) = self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .by_key
            .get_mut(&key)
        {
            kept.failed = true;
        }
    }

    /// Forgets a session that could not be resumed
    pub(super) async fn forget(&self, key: PrefixKey) {
        let mut removed = Vec::new();
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(key, &mut removed);
        release(removed).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(role: AiRole, content: &str) -> AiMessage {
        let tool_call_id = (role == AiRole::Tool).then(|| "c1".to_string());
        AiMessage {
            role,
            content: Some(content.to_string()),
            thought: None,
            thought_signature: None,
            tool_calls: None,
            tool_call_id,
        }
    }

    fn request(messages: Vec<AiMessage>) -> AiRequest {
        AiRequest {
            messages,
            ..AiRequest::default()
        }
    }

    fn session(id: &str) -> Session {
        Session {
            id: id.to_string(),
            dir: Arc::new(SessionDir::new().unwrap()),
        }
    }

    /// The key the session that answered request with reply is kept under
    fn answered(request: &AiRequest, reply: &str) -> PrefixKey {
        Lineage::of(request, "m", None).answered(&message(AiRole::Assistant, reply))
    }

    fn kept(sessions: &Sessions, key: PrefixKey) -> bool {
        sessions.inner.lock().unwrap().by_key.contains_key(&key)
    }

    #[tokio::test]
    async fn a_request_continues_the_session_that_answered_its_history() {
        let sessions = Sessions::default();
        let first = request(vec![message(AiRole::User, "review")]);
        let lineage = Lineage::of(&first, "m", None);
        assert!(sessions.find(&first, &lineage).await.is_none());
        sessions
            .keep(answered(&first, "call git_log"), session("s1"), None)
            .await;

        let second = request(vec![
            message(AiRole::User, "review"),
            message(AiRole::Assistant, "call git_log"),
            message(AiRole::Tool, "log output"),
        ]);
        let found = sessions
            .find(&second, &Lineage::of(&second, "m", None))
            .await
            .unwrap();
        assert_eq!(found.session.id, "s1");
        assert_eq!(found.known, 2);
    }

    #[tokio::test]
    async fn a_history_holding_another_reply_starts_over() {
        let sessions = Sessions::default();
        let first = request(vec![message(AiRole::User, "review")]);
        sessions
            .keep(answered(&first, "call git_log"), session("s1"), None)
            .await;

        let other = request(vec![
            message(AiRole::User, "review"),
            message(AiRole::Assistant, "call git_show"),
            message(AiRole::Tool, "show output"),
        ]);
        assert!(
            sessions
                .find(&other, &Lineage::of(&other, "m", None))
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_changed_history_or_setting_starts_over() {
        let sessions = Sessions::default();
        let first = request(vec![message(AiRole::User, "review")]);
        sessions
            .keep(answered(&first, "call git_log"), session("s1"), None)
            .await;

        let edited = request(vec![
            message(AiRole::User, "review, edited"),
            message(AiRole::Assistant, "call git_log"),
            message(AiRole::Tool, "log output"),
        ]);
        assert!(
            sessions
                .find(&edited, &Lineage::of(&edited, "m", None))
                .await
                .is_none()
        );

        let same = request(vec![
            message(AiRole::User, "review"),
            message(AiRole::Assistant, "call git_log"),
            message(AiRole::Tool, "log output"),
        ]);
        assert!(
            sessions
                .find(&same, &Lineage::of(&same, "other-model", None))
                .await
                .is_none()
        );
        assert!(
            sessions
                .find(&same, &Lineage::of(&same, "m", Some("high")))
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_request_ending_in_the_reply_has_nothing_to_send() {
        let sessions = Sessions::default();
        let first = request(vec![message(AiRole::User, "review")]);
        sessions
            .keep(answered(&first, "done"), session("s1"), None)
            .await;

        let echo = request(vec![
            message(AiRole::User, "review"),
            message(AiRole::Assistant, "done"),
        ]);
        assert!(
            sessions
                .find(&echo, &Lineage::of(&echo, "m", None))
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_continued_session_keeps_its_parent_and_frees_its_grandparent() {
        let sessions = Sessions::default();
        let first = session("s1");
        let cwd = first.dir.path().to_path_buf();
        sessions.keep([1; 32], first.clone(), None).await;
        sessions.keep([2; 32], first.clone(), Some([1; 32])).await;
        assert!(
            kept(&sessions, [1; 32]),
            "a retry of the second turn resumes the first"
        );
        sessions.keep([3; 32], first, Some([2; 32])).await;
        assert!(!kept(&sessions, [1; 32]));
        assert!(kept(&sessions, [2; 32]));
        sessions.forget([2; 32]).await;
        sessions.forget([3; 32]).await;
        assert!(!cwd.exists());
    }

    #[tokio::test]
    async fn a_retry_of_the_latest_turn_resumes_the_same_parent() {
        let sessions = Sessions::default();
        let first = request(vec![message(AiRole::User, "review")]);
        sessions
            .keep(answered(&first, "looking"), session("s1"), None)
            .await;
        let second = request(vec![
            message(AiRole::User, "review"),
            message(AiRole::Assistant, "looking"),
            message(AiRole::User, "go on"),
        ]);
        let lineage = Lineage::of(&second, "m", None);
        let found = sessions.find(&second, &lineage).await.unwrap();
        sessions
            .keep(answered(&second, "done"), session("s2"), Some(found.parent))
            .await;

        let retried = sessions.find(&second, &lineage).await.unwrap();
        assert_eq!(retried.session.id, "s1");
    }

    #[tokio::test]
    async fn a_resume_that_works_clears_an_earlier_failure() {
        let sessions = Sessions::default();
        let first = request(vec![message(AiRole::User, "review")]);
        sessions
            .keep(answered(&first, "looking"), session("s1"), None)
            .await;
        let second = request(vec![
            message(AiRole::User, "review"),
            message(AiRole::Assistant, "looking"),
            message(AiRole::User, "go on"),
        ]);
        let lineage = Lineage::of(&second, "m", None);
        let found = sessions.find(&second, &lineage).await.unwrap();
        assert!(!found.failed_before);
        sessions.mark_failed(found.parent);
        assert!(
            sessions
                .find(&second, &lineage)
                .await
                .unwrap()
                .failed_before
        );

        sessions
            .keep(answered(&second, "done"), session("s2"), Some(found.parent))
            .await;
        assert!(
            !sessions
                .find(&second, &lineage)
                .await
                .unwrap()
                .failed_before
        );
    }

    #[tokio::test]
    async fn the_oldest_sessions_go_first() {
        let sessions = Sessions::default();
        for i in 0..=MAX_SESSIONS {
            let mut key = [0; 32];
            key[..8].copy_from_slice(&(i as u64).to_le_bytes());
            sessions.keep(key, session("s"), None).await;
        }
        let map = sessions.inner.lock().unwrap();
        assert_eq!(map.by_key.len(), MAX_SESSIONS);
        assert!(!map.by_key.contains_key(&[0; 32]));
    }

    #[tokio::test]
    async fn an_idle_session_expires_with_its_files() {
        let sessions = Sessions::default();
        let idle = session("s1");
        let cwd = idle.dir.path().to_path_buf();
        let start = Instant::now();
        sessions.keep_at(start, [1; 32], idle, None).await;
        sessions
            .keep_at(start + IDLE_LIFETIME, [2; 32], session("s2"), None)
            .await;
        assert!(
            kept(&sessions, [1; 32]),
            "a session lives its whole lifetime"
        );

        let later = start + IDLE_LIFETIME + Duration::from_secs(1);
        sessions.keep_at(later, [3; 32], session("s3"), None).await;
        assert!(!kept(&sessions, [1; 32]));
        assert!(kept(&sessions, [2; 32]));
        assert!(!cwd.exists());
    }

    #[test]
    fn an_exit_finds_every_live_session_directory() {
        let dir = SessionDir::new().unwrap();
        let cwd = dir.path().to_path_buf();
        let live = || LIVE_DIRS.lock().unwrap().contains(&cwd);
        assert!(live(), "remove_session_files reaches it on exit");
        drop(dir);
        assert!(!live());
        assert!(!cwd.exists());
    }

    #[test]
    fn removal_takes_the_transcripts_too() {
        let cwd = tempfile::tempdir().unwrap().keep();
        let projects = tempfile::tempdir().unwrap();
        let transcripts = transcripts_dir_in(projects.path(), &cwd).unwrap();
        std::fs::create_dir(&transcripts).unwrap();
        std::fs::write(transcripts.join("session.jsonl"), "{}").unwrap();

        remove_files(&cwd, Some(projects.path()));
        assert!(!cwd.exists());
        assert!(!transcripts.exists());
    }

    #[test]
    fn transcripts_sit_under_the_dashed_path() {
        let dir = transcripts_dir_in(Path::new("/c/projects"), Path::new("/tmp/.tmpAb_9")).unwrap();
        assert_eq!(dir, Path::new("/c/projects/-tmp--tmpAb-9"));
    }
}
