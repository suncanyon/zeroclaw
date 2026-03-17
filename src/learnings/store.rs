//! Dynamic learnings store with file watching.
//!
//! [`LearningsStore`] wraps a `Vec<Learning>` behind an `Arc<RwLock<_>>` and
//! spawns a background task that polls the learnings directory for changes.
//! This lets agents add new learnings (by writing `LEARNING.toml` files) and
//! have them take effect immediately — without restarting the agent process.
//!
//! ## How it works
//!
//! 1. At startup the store loads all learnings from `<workspace>/learnings/`.
//! 2. A background tokio task (`spawn_watcher`) polls the directory every N
//!    seconds, comparing the most-recent modified-time across all `LEARNING.toml`
//!    files to the last-known snapshot mtime.
//! 3. When a change is detected (new file, edit, deletion) the store reloads
//!    all learnings atomically via `Arc<RwLock<_>>`.
//! 4. Because callers read via `snapshot()` / `read()` they always see the
//!    latest learnings on the *next* prompt build or hook invocation — no
//!    restart required.
//!
//! ## Agent-side programmatic writes
//!
//! An agent can call [`LearningsStore::write_learning`] to serialise a
//! [`Learning`] to disk and immediately reload the store.  Alternatively the
//! agent can use its `file_write` tool to create/update `LEARNING.toml` files
//! directly; the watcher will pick them up within one poll interval.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Result;
use parking_lot::RwLock;
use tracing::{info, warn};

use super::{load_learnings_from_directory, Learning};

// ── LearningsStore ────────────────────────────────────────────────

/// A live-reloading, thread-safe store of learnings.
///
/// Clone is cheap: all clones share the same underlying data.
#[derive(Clone)]
pub struct LearningsStore {
    inner: Arc<RwLock<Vec<Learning>>>,
    /// Absolute path to the `learnings/` directory being watched.
    pub dir: PathBuf,
}

impl LearningsStore {
    // ── Construction ─────────────────────────────────────────────

    /// Create a store backed by `<workspace_dir>/learnings/`.
    ///
    /// Learnings are loaded eagerly on construction; the watcher is *not*
    /// started automatically — call [`spawn_watcher`] to enable live reload.
    pub fn new(workspace_dir: &Path) -> Self {
        let dir = workspace_dir.join("learnings");
        let learnings = load_learnings_from_directory(&dir);
        info!(
            "LearningsStore: loaded {} learnings from {}",
            learnings.len(),
            dir.display()
        );
        Self {
            inner: Arc::new(RwLock::new(learnings)),
            dir,
        }
    }

    /// Create a store that watches `dir` directly (rather than
    /// `<workspace>/learnings/`).  Used in tests.
    pub fn from_dir(dir: PathBuf) -> Self {
        let learnings = load_learnings_from_directory(&dir);
        Self {
            inner: Arc::new(RwLock::new(learnings)),
            dir,
        }
    }

    // ── Read ─────────────────────────────────────────────────────

    /// Return a point-in-time snapshot of the current learnings.
    ///
    /// The snapshot is cloned so the lock is held only briefly.
    pub fn snapshot(&self) -> Vec<Learning> {
        self.inner.read().clone()
    }

    /// Return the number of currently-loaded learnings.
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // ── Reload ───────────────────────────────────────────────────

    /// Force-reload all learnings from disk, replacing the in-memory set.
    pub fn reload(&self) {
        let fresh = load_learnings_from_directory(&self.dir);
        let count = fresh.len();
        *self.inner.write() = fresh;
        info!(
            "LearningsStore: reloaded {} learnings from {}",
            count,
            self.dir.display()
        );
    }

    // ── Programmatic write ────────────────────────────────────────

    /// Write a new (or updated) learning to disk under
    /// `<learnings_dir>/<name>/LEARNING.toml` and immediately reload the
    /// store.
    ///
    /// This is the API agents use to persist a new behavioral rule at runtime:
    /// call `write_learning`, the file is written, and the store reloads — the
    /// new rule is active on the *very next* prompt build without restart.
    ///
    /// # Format
    ///
    /// The TOML is written in the standard `LEARNING.toml` format understood by
    /// the learnings loader.  `rules` are emitted as `[[rules]]` entries.
    pub fn write_learning(&self, learning: &Learning) -> Result<()> {
        let slug = sanitise_name(&learning.name);
        let dir = self.dir.join(&slug);
        std::fs::create_dir_all(&dir)?;
        let toml = serialise_learning_toml(learning);
        let toml_path = dir.join("LEARNING.toml");
        std::fs::write(&toml_path, toml)?;
        info!(
            "LearningsStore: wrote learning '{}' to {}",
            learning.name,
            toml_path.display()
        );
        // Reload immediately so callers see the new learning right away.
        self.reload();
        Ok(())
    }

    // ── Watcher ───────────────────────────────────────────────────

    /// Spawn a background tokio task that polls the learnings directory every
    /// `interval_secs` seconds and calls [`reload`] when a change is detected.
    ///
    /// The task holds an `Arc<LearningsStore>` so it keeps the store alive
    /// independently of any other holders.
    ///
    /// # Change detection
    ///
    /// The watcher compares the most-recent `mtime` across all `LEARNING.toml`
    /// files in the directory tree.  Any write (create, modify, delete+recreate)
    /// to any `LEARNING.toml` triggers a full reload.
    pub fn spawn_watcher(self: Arc<Self>, interval_secs: u64) {
        let store = Arc::clone(&self);
        tokio::spawn(async move {
            let mut tick =
                tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));
            let mut last_mtime = scan_mtime(&store.dir);

            loop {
                tick.tick().await;
                let current_mtime = scan_mtime(&store.dir);
                if current_mtime != last_mtime {
                    store.reload();
                    last_mtime = current_mtime;
                }
            }
        });
    }
}

// ── Helpers ───────────────────────────────────────────────────────

/// Walk the learnings directory and return the most-recent `mtime` seen across
/// all `LEARNING.toml` files (and the top-level directory entry itself so that
/// deletions are also detected).
fn scan_mtime(dir: &Path) -> Option<SystemTime> {
    let mut latest: Option<SystemTime> = None;

    // Include the directory itself — its mtime changes on file add/delete.
    if let Ok(meta) = std::fs::metadata(dir) {
        if let Ok(mtime) = meta.modified() {
            update_latest(&mut latest, mtime);
        }
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return latest;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // Subdirectory mtime (changes when files inside are added/removed).
        if let Ok(meta) = entry.metadata() {
            if let Ok(mtime) = meta.modified() {
                update_latest(&mut latest, mtime);
            }
        }
        // The LEARNING.toml file itself.
        let toml = path.join("LEARNING.toml");
        if let Ok(meta) = std::fs::metadata(&toml) {
            if let Ok(mtime) = meta.modified() {
                update_latest(&mut latest, mtime);
            }
        }
        // Optional LEARNING.md.
        let md = path.join("LEARNING.md");
        if let Ok(meta) = std::fs::metadata(&md) {
            if let Ok(mtime) = meta.modified() {
                update_latest(&mut latest, mtime);
            }
        }
    }

    latest
}

fn update_latest(latest: &mut Option<SystemTime>, candidate: SystemTime) {
    match latest {
        None => *latest = Some(candidate),
        Some(prev) if candidate > *prev => *latest = Some(candidate),
        _ => {}
    }
}

/// Sanitise a learning name into a safe filesystem slug.
fn sanitise_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Serialise a [`Learning`] to TOML string in `LEARNING.toml` format.
fn serialise_learning_toml(learning: &Learning) -> String {
    let mut out = String::new();

    out.push_str("[learning]\n");
    out.push_str(&format!("name        = {:?}\n", learning.name));
    out.push_str(&format!("description = {:?}\n", learning.description));
    out.push_str(&format!("version     = {:?}\n", learning.version));

    if let Some(ref author) = learning.author {
        out.push_str(&format!("author      = {:?}\n", author));
    }
    if !learning.tags.is_empty() {
        let tags: Vec<String> = learning.tags.iter().map(|t| format!("{t:?}")).collect();
        out.push_str(&format!("tags        = [{}]\n", tags.join(", ")));
    }

    match learning.scopes.len() {
        0 => {}
        1 => {
            out.push_str(&format!("scope       = {:?}\n", learning.scopes[0].to_string()));
        }
        _ => {
            let scopes: Vec<String> = learning
                .scopes
                .iter()
                .map(|s| format!("{:?}", s.to_string()))
                .collect();
            out.push_str(&format!("scopes      = [{}]\n", scopes.join(", ")));
        }
    }

    for rule in &learning.rules {
        out.push_str("\n[[rules]]\n");
        out.push_str(&format!("content = {:?}\n", rule));
    }

    out
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learnings::LearningScope;

    fn make_toml(name: &str, scope: &str, rule: &str) -> String {
        format!(
            r#"
[learning]
name = "{name}"
description = "test"
scope = "{scope}"

[[rules]]
content = "{rule}"
"#
        )
    }

    #[test]
    fn loads_on_construction() {
        let tmp = tempfile::tempdir().unwrap();
        let learnings_dir = tmp.path().join("learnings").join("rule-a");
        std::fs::create_dir_all(&learnings_dir).unwrap();
        std::fs::write(
            learnings_dir.join("LEARNING.toml"),
            make_toml("rule-a", "global", "Do the thing."),
        )
        .unwrap();

        let store = LearningsStore::new(tmp.path());
        assert_eq!(store.len(), 1);
        assert_eq!(store.snapshot()[0].name, "rule-a");
    }

    #[test]
    fn reload_picks_up_new_file() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LearningsStore::new(tmp.path());
        assert_eq!(store.len(), 0);

        // Write a new learning.
        let learnings_dir = tmp.path().join("learnings").join("rule-b");
        std::fs::create_dir_all(&learnings_dir).unwrap();
        std::fs::write(
            learnings_dir.join("LEARNING.toml"),
            make_toml("rule-b", "global", "New rule."),
        )
        .unwrap();

        store.reload();
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn write_learning_persists_and_reloads() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LearningsStore::new(tmp.path());

        let learning = Learning {
            name: "written-rule".into(),
            description: "A programmatically written rule".into(),
            version: "0.1.0".into(),
            author: None,
            tags: vec![],
            scopes: vec![LearningScope::Global],
            rules: vec!["Always write tests.".into()],
            location: None,
        };

        store.write_learning(&learning).unwrap();

        assert_eq!(store.len(), 1);
        assert_eq!(store.snapshot()[0].name, "written-rule");

        // The file should exist on disk.
        let toml_path = tmp
            .path()
            .join("learnings")
            .join("written-rule")
            .join("LEARNING.toml");
        assert!(toml_path.exists());
        let contents = std::fs::read_to_string(&toml_path).unwrap();
        assert!(contents.contains("written-rule"));
        assert!(contents.contains("Always write tests."));
    }

    #[test]
    fn write_learning_special_chars_in_name() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LearningsStore::new(tmp.path());

        let learning = Learning {
            name: "my rule / thing!".into(),
            description: "test".into(),
            version: "0.1.0".into(),
            author: None,
            tags: vec![],
            scopes: vec![LearningScope::Global],
            rules: vec!["rule".into()],
            location: None,
        };

        store.write_learning(&learning).unwrap();
        let slug = sanitise_name("my rule / thing!");
        let toml_path = tmp
            .path()
            .join("learnings")
            .join(&slug)
            .join("LEARNING.toml");
        assert!(toml_path.exists());
    }

    #[test]
    fn scan_mtime_returns_none_for_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let learnings_dir = tmp.path().join("learnings");
        // Don't create the dir — should return None.
        assert!(scan_mtime(&learnings_dir).is_none());
    }

    #[test]
    fn scan_mtime_updates_after_write() {
        let tmp = tempfile::tempdir().unwrap();
        let learnings_dir = tmp.path().join("learnings").join("rule-x");
        std::fs::create_dir_all(&learnings_dir).unwrap();

        let before = scan_mtime(tmp.path().join("learnings").as_path());

        // Small sleep to ensure mtime changes.
        std::thread::sleep(std::time::Duration::from_millis(10));

        std::fs::write(
            learnings_dir.join("LEARNING.toml"),
            make_toml("rule-x", "global", "x"),
        )
        .unwrap();

        let after = scan_mtime(tmp.path().join("learnings").as_path());
        assert_ne!(before, after);
    }
}
