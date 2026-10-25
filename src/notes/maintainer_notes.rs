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

//! Maintainer notes: what a subsystem's maintainers want that the code
//! cannot tell (preferences, policies, conventions, review priorities).
//! Code notes are generated from the source; maintainer notes are written
//! by people, one file per subsystem under `maintainers/` in a project's
//! prompt directory:
//!
//! ```markdown
//! +++
//! title = "Networking Drivers"
//! paths = ["drivers/net/"]
//! symbols = ["ethtool_ops", "netdev_*"]
//! +++
//! # Networking Drivers: maintainer notes
//!
//! - One preference per bullet.
//! ```
//!
//! A review loads every note whose `paths` match a file the patch touches
//! or whose `symbols` match an identifier its diff touches. A path is a
//! directory prefix ending in `/`, an exact file, a glob, or `*` for every
//! patch; a symbol is an identifier, or a prefix ending in `*`.
//!
//! Library notes use the same format under `libraries/`. They hold the
//! contracts and known pitfalls of a library the reviewed code calls into,
//! which is how code works rather than what a maintainer prefers, so they
//! render in a block of their own.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::maintainers::CompiledPattern;

/// Directory of maintainer notes inside a project's prompt directory.
pub const MAINTAINER_NOTES_RELDIR: &str = "maintainers";

/// Directory of library notes inside a project's prompt directory.
pub const LIBRARY_NOTES_RELDIR: &str = "libraries";

const DELIMITER: &str = "+++";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrontMatter {
    title: String,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    symbols: Vec<String>,
}

/// One maintainer or library note.
#[derive(Debug, Clone)]
pub struct MaintainerNote {
    /// File name without `.md`.
    pub name: String,
    pub title: String,
    pub paths: Vec<String>,
    pub symbols: Vec<String>,
    /// The markdown after the front matter.
    pub body: String,
}

impl MaintainerNote {
    /// Parses a note from its file contents.
    pub fn parse(name: &str, text: &str) -> Result<Self> {
        let rest = text
            .strip_prefix(DELIMITER)
            .context("a maintainer note must start with a +++ front matter line")?;
        let (front, body) = rest
            .split_once(&format!("\n{DELIMITER}"))
            .context("the front matter has no closing +++ line")?;
        let front: FrontMatter = toml::from_str(front).context("invalid front matter")?;
        if front.title.trim().is_empty() {
            bail!("the front matter needs a title");
        }
        if front.paths.is_empty() && front.symbols.is_empty() {
            bail!("the front matter needs paths or symbols, or the note is never loaded");
        }
        let body = body.trim_start_matches(['\r', '\n']).trim_end().to_string();
        if body.is_empty() {
            bail!("the note has no body");
        }
        Ok(Self {
            name: name.to_string(),
            title: front.title,
            paths: front.paths,
            symbols: front.symbols,
            body,
        })
    }

    /// Whether the note applies to a patch touching `files` with a diff
    /// touching `identifiers`.
    pub fn applies(&self, files: &[String], identifiers: &HashSet<String>) -> bool {
        let path_match = self.paths.iter().any(|pattern| {
            pattern.trim() == "*" || {
                let compiled = CompiledPattern::compile(pattern);
                files.iter().any(|f| compiled.is_match(f))
            }
        });
        path_match
            || self
                .symbols
                .iter()
                .any(|symbol| match symbol.strip_suffix('*') {
                    Some(prefix) => identifiers.iter().any(|id| id.starts_with(prefix)),
                    None => identifiers.contains(symbol),
                })
    }
}

/// Every maintainer note in `prompts_dir`, sorted by name.
pub fn load_maintainer_notes(prompts_dir: &Path) -> Vec<MaintainerNote> {
    load_notes(&prompts_dir.join(MAINTAINER_NOTES_RELDIR))
}

/// Every library note in `prompts_dir`, sorted by name.
pub fn load_library_notes(prompts_dir: &Path) -> Vec<MaintainerNote> {
    load_notes(&prompts_dir.join(LIBRARY_NOTES_RELDIR))
}

/// Every note in `dir`, sorted by name. Notes that fail to parse are
/// skipped with a warning, so one bad note does not cost a review the
/// others.
fn load_notes(dir: &Path) -> Vec<MaintainerNote> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut notes = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".md"))
        else {
            continue;
        };
        let parsed = std::fs::read_to_string(&path)
            .context("unreadable")
            .and_then(|text| MaintainerNote::parse(name, &text));
        match parsed {
            Ok(note) => notes.push(note),
            Err(err) => tracing::warn!("skipping note {}: {err:#}", path.display()),
        }
    }
    notes.sort_by(|a, b| a.name.cmp(&b.name));
    notes
}

/// Every identifier on a diff's added and removed lines and in its hunk
/// headers. Unlike code-note focusing, short and common names count:
/// maintainer notes name subsystem prefixes such as `skb_` or `bpf`.
pub fn diff_identifiers_all(diff: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut word = String::new();
    for line in diff.lines() {
        let text = if line.starts_with("+++") || line.starts_with("---") {
            continue;
        } else if let Some(rest) = line.strip_prefix("@@") {
            rest.split_once("@@").map_or("", |(_, context)| context)
        } else if let Some(rest) = line.strip_prefix(['+', '-']) {
            rest
        } else {
            continue;
        };
        for c in text.chars().chain(std::iter::once(' ')) {
            if c.is_ascii_alphanumeric() || c == '_' {
                word.push(c);
            } else if !word.is_empty() {
                if !word.starts_with(|c: char| c.is_ascii_digit()) {
                    out.insert(std::mem::take(&mut word));
                } else {
                    word.clear();
                }
            }
        }
    }
    out
}

/// Renders the review prompt block for maintainer `notes`.
pub fn render_maintainer_notes(notes: &[&MaintainerNote]) -> String {
    render_notes(
        notes,
        ("maintainer_notes", "maintainer_note", "subsystem"),
        "These notes record what the maintainers of the subsystems this patch touches expect beyond correctness: their preferences, policies, conventions and review priorities. Apply them when judging the patch; they do not describe how the code works.",
    )
}

/// Renders the review prompt block for library `notes`.
pub fn render_library_notes(notes: &[&MaintainerNote]) -> String {
    render_notes(
        notes,
        ("library_notes", "library_note", "library"),
        "These notes describe the contracts and known pitfalls of the libraries and interfaces this patch uses. Check the patch against them, but read the source at Target Commit SHA for what the code actually does.",
    )
}

fn render_notes(
    notes: &[&MaintainerNote],
    (block_tag, note_tag, attribute): (&str, &str, &str),
    preamble: &str,
) -> String {
    if notes.is_empty() {
        return String::new();
    }
    let mut block = format!("\n\n<{block_tag}>\n{preamble}\n");
    for note in notes {
        block.push_str(&format!(
            "\n<{note_tag} {attribute}=\"{}\">\n{}\n</{note_tag}>\n",
            note.title.replace('"', "'"),
            note.body
        ));
    }
    block.push_str(&format!("</{block_tag}>"));
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str = "+++\ntitle = \"Networking Drivers\"\npaths = [\"drivers/net/\"]\nsymbols = [\"ethtool_ops\", \"netdev_*\"]\n+++\n# Networking Drivers: maintainer notes\n\n- Prefer standard stats.\n";

    #[test]
    fn test_parse_and_apply() {
        let note = MaintainerNote::parse("networking-drivers", NOTE).unwrap();
        assert_eq!(note.title, "Networking Drivers");
        assert!(note.body.starts_with("# Networking Drivers"));

        let none = HashSet::new();
        assert!(note.applies(&["drivers/net/e1000/main.c".to_string()], &none));
        assert!(!note.applies(&["fs/ext4/inode.c".to_string()], &none));
        let ids = HashSet::from(["netdev_priv".to_string()]);
        assert!(note.applies(&[], &ids));
        assert!(note.applies(&[], &HashSet::from(["ethtool_ops".to_string()])));
        assert!(!note.applies(&[], &HashSet::from(["ethtool_ops_x".to_string()])));

        let general = MaintainerNote::parse(
            "general",
            "+++\ntitle = \"General\"\npaths = [\"*\"]\n+++\n- Be kind.\n",
        )
        .unwrap();
        assert!(general.applies(&["any/file.c".to_string()], &none));
    }

    #[test]
    fn test_parse_rejects_bad_notes() {
        for (text, expected) in [
            ("# no front matter", "front matter line"),
            ("+++\ntitle = \"x\"\npaths = [\"a/\"]\n", "closing"),
            ("+++\ntitle = \"x\"\n+++\n- b\n", "paths or symbols"),
            (
                "+++\ntitle = \"x\"\npaths = [\"a/\"]\nextra = 1\n+++\n- b\n",
                "invalid",
            ),
            ("+++\ntitle = \"x\"\npaths = [\"a/\"]\n+++\n\n", "no body"),
        ] {
            let err = format!("{:#}", MaintainerNote::parse("n", text).unwrap_err());
            assert!(err.contains(expected), "expected {expected:?} in {err:?}");
        }
    }

    #[test]
    fn test_diff_identifiers_all_keeps_short_names() {
        let ids = diff_identifiers_all(
            "--- a/x.c\n+++ b/x.c\n@@ -1 +1 @@ int bpf_prog_run(void)\n-\tskb_put(skb, 4);\n+\tret = 0x10;\n context_line(ignored);\n",
        );
        for id in ["bpf_prog_run", "skb_put", "skb", "ret", "int"] {
            assert!(ids.contains(id), "missing {id}");
        }
        assert!(!ids.contains("context_line") && !ids.contains("0x10"));
    }

    #[test]
    fn test_render_block() {
        let note = MaintainerNote::parse("n", NOTE).unwrap();
        let block = render_maintainer_notes(&[&note]);
        assert!(block.contains("<maintainer_note subsystem=\"Networking Drivers\">"));
        assert!(block.contains("- Prefer standard stats."));
        assert_eq!(render_maintainer_notes(&[]), "");
    }

    #[test]
    fn test_library_and_maintainer_notes_load_from_their_own_dirs() {
        let prompts = tempfile::tempdir().unwrap();
        for (reldir, name) in [
            (MAINTAINER_NOTES_RELDIR, "net-policy"),
            (LIBRARY_NOTES_RELDIR, "glib"),
        ] {
            let dir = prompts.path().join(reldir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{name}.md")), NOTE).unwrap();
        }
        let names = |notes: Vec<MaintainerNote>| -> Vec<String> {
            notes.into_iter().map(|n| n.name).collect()
        };
        assert_eq!(names(load_maintainer_notes(prompts.path())), ["net-policy"]);
        assert_eq!(names(load_library_notes(prompts.path())), ["glib"]);
    }

    #[test]
    fn test_render_library_block() {
        let note = MaintainerNote::parse("n", NOTE).unwrap();
        let block = render_library_notes(&[&note]);
        assert!(block.starts_with("\n\n<library_notes>\n"));
        assert!(block.contains("<library_note library=\"Networking Drivers\">"));
        assert!(block.ends_with("</library_notes>"));
        assert!(!block.contains("<maintainer_note"));
        assert_eq!(render_library_notes(&[]), "");
    }

    /// Reads every maintainer and library note shipped under `projects/*/prompts/`.
    fn shipped_maintainer_notes() -> Vec<(std::path::PathBuf, String)> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("projects");
        let mut notes = Vec::new();
        for project in std::fs::read_dir(&root).unwrap().flatten() {
            for reldir in [MAINTAINER_NOTES_RELDIR, LIBRARY_NOTES_RELDIR] {
                let dir = project.path().join("prompts").join(reldir);
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    let text = std::fs::read_to_string(&path).unwrap();
                    notes.push((path, text));
                }
            }
        }
        notes
    }

    #[test]
    fn test_shipped_maintainer_notes_parse() {
        for (path, text) in shipped_maintainer_notes() {
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            MaintainerNote::parse(&name, &text)
                .unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        }
    }

    /// Priority words that rank an issue outside the Low, Medium, High and
    /// Critical levels Sashiko reports.
    const NON_SEVERITY_RANK_WORDS: &[&str] = &[
        "nit",
        "nits",
        "blocker",
        "blockers",
        "showstopper",
        "p0",
        "p1",
        "p2",
        "p3",
    ];

    /// Returns the non-severity priority words used in `text`. Words inside
    /// double quotes are mentions rather than rankings (the notes policy
    /// lists them as examples) and are skipped.
    fn non_severity_rank_words(text: &str) -> Vec<String> {
        let unquoted: Vec<&str> = text.split('"').step_by(2).collect();
        unquoted
            .join(" ")
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| NON_SEVERITY_RANK_WORDS.contains(&w.to_ascii_lowercase().as_str()))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn test_non_severity_rank_words() {
        assert_eq!(
            non_severity_rank_words("A missing #[inline] is a nit; P1 at most."),
            vec!["nit", "P1"]
        );
        assert!(non_severity_rank_words("Report it as Low. Do not nitpick.").is_empty());
        assert!(non_severity_rank_words("Avoid terms such as \"nit\" or \"P0\".").is_empty());
    }

    #[test]
    fn test_shipped_maintainer_notes_use_severity_levels() {
        for (path, text) in shipped_maintainer_notes() {
            let words = non_severity_rank_words(&text);
            assert!(
                words.is_empty(),
                "{}: uses {words:?} to rank issues; use Low, Medium, High or \
                 Critical instead",
                path.display()
            );
        }
    }
}
