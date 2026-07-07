//! The task store: a single human-editable TOML file (`tasks.toml`) that is the
//! one source of truth. `load_or_default` reads it; `save` rewrites it with a
//! how-to-edit header. Also provides the queries selection needs
//! (`selectable_by_tag`, `has_selectable`) and the mutations the UI/manager use
//! (`add`, `remove`, `complete`).

use crate::task::{Status, Task};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use uuid::Uuid;

// Header written atop the TOML store to orient anyone hand-editing it.
const STORE_HEADER: &str = "\
# rusty-krab-manager task store (TOML). Edit tasks here or in the app.
# Add a task by appending a [[tasks]] block; leave `id` out and it will be
# filled in on the next save. `due` is \"YYYY-MM-DD HH:MM\" (omit for none).
# Optional `recur` (\"12h\" / \"3d\" / \"2w\") makes a task recurring: completing
# it reschedules `due` that far ahead instead of finishing it (subtasks
# reset), and it never expires when overdue.
# subtasks/status/history are managed for you. This file is rewritten on save,
# so any comments you add here will not be preserved.
";

// The persisted task list. Backed by a single human-editable TOML file.
#[derive(Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub tasks: Vec<Task>,
}

impl Store {
    pub fn load(path: &Path) -> io::Result<Self> {
        let data = fs::read_to_string(path)?;
        toml::from_str(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    // Serialize to the on-disk TOML form (header comment + tasks).
    pub fn to_toml(&self) -> io::Result<String> {
        let body = toml::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(format!("{STORE_HEADER}\n{body}"))
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        fs::write(path, self.to_toml()?)
    }

    // Load the TOML store. A missing file yields an empty store; a corrupt one
    // warns and starts empty rather than failing.
    pub fn load_or_default(store_path: &Path) -> Self {
        if !store_path.exists() {
            return Store::default();
        }
        Store::load(store_path).unwrap_or_else(|e| {
            eprintln!("warning: could not read task store ({e}); starting empty");
            Store::default()
        })
    }

    // Selectable tasks grouped by tag, restricted to the configured tags.
    // Tasks whose tag is not configured are ignored (rather than panicking).
    pub fn selectable_by_tag<'a>(&'a self, tags: &[String]) -> HashMap<String, Vec<&'a Task>> {
        let mut map: HashMap<String, Vec<&Task>> =
            tags.iter().map(|t| (t.clone(), Vec::new())).collect();
        for task in self.tasks.iter().filter(|t| t.is_selectable()) {
            if let Some(bucket) = map.get_mut(&task.tag) {
                bucket.push(task);
            }
        }
        map
    }

    pub fn has_selectable(&self, tags: &[String]) -> bool {
        self.tasks
            .iter()
            .any(|t| t.is_selectable() && tags.iter().any(|tag| tag == &t.tag))
    }

    // Find a task by id.
    pub fn task(&self, id: Uuid) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn task_mut(&mut self, id: Uuid) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    // Mark a task done and log it. A recurring task is rescheduled instead:
    // subtasks reset and `due` moves one interval past now (completion-
    // anchored, like a chore). Returns true if the task was found.
    pub fn complete(&mut self, id: Uuid) -> bool {
        match self.task_mut(id) {
            Some(task) => {
                match task.recurrence() {
                    Some(interval) => {
                        task.log("completed (recurs)");
                        for s in &mut task.subtasks {
                            s.done = false;
                        }
                        task.due = Some(chrono::Local::now() + interval);
                    }
                    None => {
                        task.status = Status::Done;
                        task.log("completed");
                    }
                }
                true
            }
            None => false,
        }
    }

    pub fn add(&mut self, task: Task) {
        self.tasks.push(task);
    }

    // Remove a task by id. Returns true if one was removed.
    pub fn remove(&mut self, id: Uuid) -> bool {
        let before = self.tasks.len();
        self.tasks.retain(|t| t.id != id);
        self.tasks.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{parse_due, Status};

    fn tmp_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("rkm-test-{}.toml", Uuid::new_v4()))
    }

    #[test]
    fn complete_marks_done_logs_and_handles_unknown() {
        let mut store = Store {
            tasks: vec![Task::new("school".into(), "t".into(), None)],
        };
        let id = store.tasks[0].id;
        assert!(store.complete(id));
        assert_eq!(store.tasks[0].status, Status::Done);
        assert_eq!(store.tasks[0].history.len(), 1);
        assert!(!store.complete(Uuid::new_v4()));
    }

    #[test]
    fn complete_reschedules_recurring_task() {
        let mut t = Task::new("home".into(), "water plants".into(), None);
        t.recur = Some("3d".into());
        t.add_subtask("front".into());
        t.toggle_subtask(0);
        let id = t.id;
        let mut store = Store { tasks: vec![t] };

        assert!(store.complete(id));
        let t = store.task(id).unwrap();
        // Not finished: still Todo, subtasks reset, due pushed ~3 days out.
        assert_eq!(t.status, Status::Todo);
        assert!(!t.subtasks[0].done);
        assert_eq!(t.history.len(), 1);
        let mins = t.minutes_until_due().unwrap();
        assert!(mins > 2 * 24 * 60 && mins <= 3 * 24 * 60);
        assert!(t.is_selectable());
    }

    #[test]
    fn selectable_filters_done_and_unconfigured_tags() {
        let mut done = Task::new("school".into(), "done".into(), None);
        done.status = Status::Done;
        let store = Store {
            tasks: vec![
                Task::new("school".into(), "keep".into(), None),
                done,
                Task::new("unknown".into(), "skip".into(), None),
            ],
        };
        let tags = vec!["school".to_string(), "job".to_string()];
        let grouped = store.selectable_by_tag(&tags);
        assert_eq!(grouped["school"].len(), 1);
        assert!(grouped["job"].is_empty());
        assert!(store.has_selectable(&tags));
    }

    #[test]
    fn save_then_load_roundtrips() {
        let path = tmp_path();
        let mut t = Task::new(
            "school".into(),
            "thesis".into(),
            parse_due("2050-01-01 10:00"),
        );
        t.add_subtask("outline".into());
        let store = Store { tasks: vec![t] };
        store.save(&path).unwrap();
        let loaded = Store::load(&path).unwrap();
        assert_eq!(loaded.tasks.len(), 1);
        assert_eq!(loaded.tasks[0].name, "thesis");
        assert_eq!(loaded.tasks[0].subtasks.len(), 1);
        assert!(loaded.tasks[0].due.is_some());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn load_or_default_is_empty_when_missing() {
        let path = tmp_path();
        assert!(!path.exists());
        assert!(Store::load_or_default(&path).tasks.is_empty());
    }
}
