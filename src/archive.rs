//! The machine-side archive (`archive.toml`, sibling of `tasks.md`).
//! Append-mostly and never hand-edited: completed non-recurring tasks
//! land here in full, and history events (completions, manager notes)
//! accumulate here keyed by (tag, task name). This is the data a
//! future stats/review feature reads.

use crate::task::{due_fmt, Subtask, Task};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize)]
pub struct CompletedTask {
    pub tag: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "due_fmt")]
    pub due: Option<DateTime<Local>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recur: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subtasks: Vec<Subtask>,
    pub completed_at: DateTime<Local>,
}

impl CompletedTask {
    pub fn from_task(t: &Task, at: DateTime<Local>) -> Self {
        Self {
            tag: t.tag.clone(),
            name: t.name.clone(),
            due: t.due,
            recur: t.recur.clone(),
            subtasks: t.subtasks.clone(),
            completed_at: at,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct HistoryEvent {
    pub tag: String,
    pub task: String,
    pub at: DateTime<Local>,
    pub note: String,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Archive {
    #[serde(default)]
    pub completed: Vec<CompletedTask>,
    #[serde(default)]
    pub events: Vec<HistoryEvent>,
}

impl Archive {
    // Missing or corrupt archives start empty (warn on corrupt) — the
    // archive is bookkeeping, never worth failing startup over.
    pub fn load_or_default(path: &Path) -> Self {
        let Ok(data) = fs::read_to_string(path) else {
            return Archive::default();
        };
        toml::from_str(&data).unwrap_or_else(|e| {
            eprintln!("warning: could not read archive ({e}); starting empty");
            Archive::default()
        })
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let body = toml::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        write_atomic(path, &body)
    }

    pub fn event(&mut self, tag: &str, task: &str, note: impl Into<String>) {
        self.events.push(HistoryEvent {
            tag: tag.to_string(),
            task: task.to_string(),
            at: Local::now(),
            note: note.into(),
        });
    }
}

// The archive sits next to the store file.
pub fn archive_path(store_path: &Path) -> PathBuf {
    store_path.with_file_name("archive.toml")
}

// Write via a temp file + rename so an interrupted save can never
// truncate the destination. Shared with the store (spec §3).
pub(crate) fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{parse_due, Task};

    fn tmp_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("rkm-arch-{}.toml", uuid::Uuid::new_v4()))
    }

    #[test]
    fn archive_roundtrips_and_defaults_empty() {
        let path = tmp_path();
        assert!(Archive::load_or_default(&path).completed.is_empty());

        let mut a = Archive::default();
        let mut t = Task::new("job".into(), "ship".into(), parse_due("2050-01-01 10:00"));
        t.add_subtask("docs".into());
        a.completed
            .push(CompletedTask::from_task(&t, chrono::Local::now()));
        a.event("job", "ship", "completed");
        a.save(&path).unwrap();

        let back = Archive::load_or_default(&path);
        assert_eq!(back.completed.len(), 1);
        assert_eq!(back.completed[0].name, "ship");
        assert_eq!(back.completed[0].subtasks.len(), 1);
        assert_eq!(back.events.len(), 1);
        assert_eq!(back.events[0].note, "completed");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn archive_path_is_sibling_archive_toml() {
        let p = archive_path(std::path::Path::new("/x/y/tasks.md"));
        assert_eq!(p, std::path::PathBuf::from("/x/y/archive.toml"));
    }
}
