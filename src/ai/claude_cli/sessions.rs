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
//! A session is found by the request it answered: a request whose messages
//! start with that request's messages, followed by the reply, continues it.
//! `--fork-session` leaves the parent untouched, so a retry or a second
//! branch from the same history resumes the same parent.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use sha2::{Digest, Sha256};

use crate::ai::{AiRequest, AiRole};

pub(super) type PrefixKey = [u8; 32];

/// Sessions kept for continuation, oldest first out. Normal turns replace
/// their parent, so this bounds only conversations a stage left unfinished
const MAX_SESSIONS: usize = 256;

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
    let _ = std::fs::remove_dir_all(cwd);
    if let Some(transcripts) = transcripts_dir(cwd) {
        let _ = std::fs::remove_dir_all(transcripts);
    }
}

/// Where Claude Code keeps the transcripts of sessions run in cwd: the
/// projects directory of its config, under the path with every character
/// other than an ASCII letter or digit replaced by a dash
fn transcripts_dir(cwd: &Path) -> Option<PathBuf> {
    let config = match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".claude"),
    };
    let name: String = cwd
        .to_str()?
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    Some(config.join("projects").join(name))
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
            let mut hasher = Sha256::new();
            hasher.update(key);
            hasher.update(serde_json::to_vec(message).unwrap_or_default());
            key = hasher.finalize().into();
            keys.push(key);
        }
        Self { keys }
    }

    /// The key of the whole request, under which its reply's session is kept
    pub(super) fn own(&self) -> PrefixKey {
        self.keys[self.keys.len() - 1]
    }
}

/// What a request continues: the session, the key it was kept under and how
/// many of the request's messages it already holds
pub(super) struct Continuation {
    pub(super) session: Session,
    pub(super) parent: PrefixKey,
    pub(super) known: usize,
}

#[derive(Default)]
pub struct Sessions {
    inner: Mutex<SessionMap>,
}

#[derive(Default)]
struct SessionMap {
    by_key: HashMap<PrefixKey, Session>,
    order: VecDeque<PrefixKey>,
}

impl Sessions {
    /// Finds the latest session this request continues. A session answered
    /// the first k messages when message k is the assistant's reply, and the
    /// request has at least one message after it
    pub(super) fn find(&self, request: &AiRequest, lineage: &Lineage) -> Option<Continuation> {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let messages = &request.messages;
        (1..messages.len().saturating_sub(1)).rev().find_map(|k| {
            if messages[k].role != AiRole::Assistant {
                return None;
            }
            let parent = lineage.keys[k];
            map.by_key.get(&parent).map(|session| Continuation {
                session: session.clone(),
                parent,
                known: k + 1,
            })
        })
    }

    /// Keeps the session that answered a request, in place of the one it
    /// continued
    pub(super) fn keep(&self, key: PrefixKey, session: Session, parent: Option<PrefixKey>) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(parent) = parent {
            map.by_key.remove(&parent);
            map.order.retain(|k| *k != parent);
        }
        if map.by_key.insert(key, session).is_none() {
            map.order.push_back(key);
        }
        while map.order.len() > MAX_SESSIONS {
            if let Some(oldest) = map.order.pop_front() {
                map.by_key.remove(&oldest);
            }
        }
    }

    /// Forgets a session that could not be resumed
    pub(super) fn forget(&self, key: PrefixKey) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.by_key.remove(&key);
        map.order.retain(|k| *k != key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::AiMessage;

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

    #[test]
    fn a_request_continues_the_session_that_answered_its_history() {
        let sessions = Sessions::default();
        let first = request(vec![message(AiRole::User, "review")]);
        let lineage = Lineage::of(&first, "m", None);
        assert!(sessions.find(&first, &lineage).is_none());
        sessions.keep(lineage.own(), session("s1"), None);

        let second = request(vec![
            message(AiRole::User, "review"),
            message(AiRole::Assistant, "call git_log"),
            message(AiRole::Tool, "log output"),
        ]);
        let found = sessions
            .find(&second, &Lineage::of(&second, "m", None))
            .unwrap();
        assert_eq!(found.session.id, "s1");
        assert_eq!(found.known, 2);
    }

    #[test]
    fn a_changed_history_or_setting_starts_over() {
        let sessions = Sessions::default();
        let first = request(vec![message(AiRole::User, "review")]);
        sessions.keep(Lineage::of(&first, "m", None).own(), session("s1"), None);

        let edited = request(vec![
            message(AiRole::User, "review, edited"),
            message(AiRole::Assistant, "call git_log"),
            message(AiRole::Tool, "log output"),
        ]);
        assert!(
            sessions
                .find(&edited, &Lineage::of(&edited, "m", None))
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
                .is_none()
        );
        assert!(
            sessions
                .find(&same, &Lineage::of(&same, "m", Some("high")))
                .is_none()
        );
    }

    #[test]
    fn a_request_ending_in_the_reply_has_nothing_to_send() {
        let sessions = Sessions::default();
        let first = request(vec![message(AiRole::User, "review")]);
        sessions.keep(Lineage::of(&first, "m", None).own(), session("s1"), None);

        let echo = request(vec![
            message(AiRole::User, "review"),
            message(AiRole::Assistant, "done"),
        ]);
        assert!(
            sessions
                .find(&echo, &Lineage::of(&echo, "m", None))
                .is_none()
        );
    }

    #[test]
    fn a_continued_session_replaces_its_parent_and_frees_its_directory() {
        let sessions = Sessions::default();
        let parent = session("s1");
        let cwd = parent.dir.path().to_path_buf();
        sessions.keep([1; 32], parent.clone(), None);
        sessions.keep([2; 32], parent, Some([1; 32]));
        assert!(!sessions.inner.lock().unwrap().by_key.contains_key(&[1; 32]));
        sessions.forget([2; 32]);
        assert!(!cwd.exists());
    }

    #[test]
    fn the_oldest_sessions_go_first() {
        let sessions = Sessions::default();
        for i in 0..=MAX_SESSIONS {
            let mut key = [0; 32];
            key[..8].copy_from_slice(&(i as u64).to_le_bytes());
            sessions.keep(key, session("s"), None);
        }
        let map = sessions.inner.lock().unwrap();
        assert_eq!(map.by_key.len(), MAX_SESSIONS);
        assert!(!map.by_key.contains_key(&[0; 32]));
    }

    #[test]
    fn transcripts_sit_under_the_dashed_path() {
        let dir = transcripts_dir(Path::new("/tmp/.tmpAb_9")).unwrap();
        assert!(dir.ends_with("projects/-tmp--tmpAb-9"));
    }
}
