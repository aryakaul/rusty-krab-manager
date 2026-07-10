//! The task store: the human-editable `tasks.md` is the single source
//! of truth for live tasks; completed tasks and history events move to
//! the machine-side archive (`archive.rs`). This module keeps the
//! in-memory `Vec<Task>` plus everything needed to rewrite the file
//! faithfully (preamble, section order, preserved unknown lines) and
//! the mtime bookkeeping behind external-edit detection.

use crate::archive::{write_atomic, Archive, CompletedTask};
use crate::mdstore::{self, Extra};
use crate::task::{Status, Task};
use chrono::Local;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::time::SystemTime;
use uuid::Uuid;

// Preamble written into brand-new stores so the file explains itself.
const DEFAULT_PREAMBLE: [&str; 4] = [
    "# Tasks",
    "Sections are tags; tasks are checkboxes; indent subtasks under a task.",
    "Annotations: `@due 2026-01-31 17:00` (time optional) · `@every 3d` (recurring).",
    "Check a task `[x]` to complete it — finished tasks move to archive.toml.",
];

#[derive(Default)]
pub struct Store {
    pub tasks: Vec<Task>,
    // Parse warnings from the last load — surfaced in the TUI footer.
    pub warnings: Vec<String>,
    preamble: Vec<String>,
    sections: Vec<String>,
    extras: Vec<Extra>,
    last_mtime: Option<SystemTime>,
}

fn mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Store {
    // A fresh store carrying the self-describing preamble.
    pub fn seed(tasks: Vec<Task>) -> Self {
        let mut store = Store {
            preamble: DEFAULT_PREAMBLE.iter().map(|s| s.to_string()).collect(),
            ..Store::default()
        };
        for t in tasks {
            store.add(t);
        }
        store
    }

    // Load tasks.md. Missing file yields an empty (seeded) store; an
    // unreadable one warns and starts empty rather than failing.
    pub fn load_or_default(path: &Path) -> Self {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Store::seed(Vec::new());
            }
            Err(e) => {
                eprintln!("warning: could not read task store ({e}); starting empty");
                return Store::seed(Vec::new());
            }
        };
        let doc = mdstore::parse(&text);
        Store {
            tasks: doc.tasks,
            warnings: doc.warnings,
            preamble: doc.preamble,
            sections: doc.sections,
            extras: doc.extras,
            last_mtime: mtime(path),
        }
    }

    pub fn to_markdown(&self) -> String {
        mdstore::render(&self.preamble, &self.sections, &self.tasks, &self.extras)
    }

    // Atomic write (temp + rename); refreshes our mtime baseline so
    // our own save doesn't read back as an external change.
    pub fn save(&mut self, path: &Path) -> io::Result<()> {
        write_atomic(path, &self.to_markdown())?;
        self.last_mtime = mtime(path);
        Ok(())
    }

    // Has the file on disk changed (or vanished) since we last
    // read/wrote it?
    pub fn external_change(&self, path: &Path) -> bool {
        mtime(path) != self.last_mtime
    }

    // Carry session ids across a reload: a task with the same
    // (tag, name) keeps its previous id, so the cursor, expansion, and
    // the current task all survive external edits. Each old task's id
    // is consumed at most once, so duplicate (tag, name) pairs (the
    // parser permits them with a warning) never alias the same id onto
    // two different new tasks.
    pub fn rematch_ids(&mut self, old: &Store) {
        let mut used: Vec<Uuid> = Vec::new();
        for t in &mut self.tasks {
            if let Some(o) = old
                .tasks
                .iter()
                .find(|o| o.tag == t.tag && o.name == t.name && !used.contains(&o.id))
            {
                t.id = o.id;
                used.push(o.id);
            }
        }
    }

    // Complete one task by id (TUI `c` / manager). Recurring tasks
    // reschedule; others move to the archive. True if found.
    pub fn complete(&mut self, id: Uuid, archive: &mut Archive) -> bool {
        let Some(pos) = self.tasks.iter().position(|t| t.id == id) else {
            return false;
        };
        Self::complete_at(&mut self.tasks, pos, archive);
        true
    }

    // `[x]` marks typed into the file are completions too. Returns
    // true when anything changed (caller should save).
    pub fn process_file_completions(&mut self, archive: &mut Archive) -> bool {
        let mut changed = false;
        let mut i = 0;
        while i < self.tasks.len() {
            if self.tasks[i].status == Status::Done {
                Self::complete_at(&mut self.tasks, i, archive);
                changed = true;
                // complete_at either rescheduled in place or removed
                if self.tasks.get(i).is_some_and(|t| t.status != Status::Done) {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
        changed
    }

    // Shared completion core: reschedule a recurring task in place,
    // or remove + archive a normal one (completion-anchored, spec §1).
    fn complete_at(tasks: &mut Vec<Task>, pos: usize, archive: &mut Archive) {
        let task = &mut tasks[pos];
        match task.recurrence() {
            Some(interval) => {
                archive.event(&task.tag, &task.name, "completed (recurs)");
                for s in &mut task.subtasks {
                    s.done = false;
                }
                task.due = Some(Local::now() + interval);
                task.status = Status::Todo;
            }
            None => {
                let task = tasks.remove(pos);
                archive.event(&task.tag, &task.name, "completed");
                archive
                    .completed
                    .push(CompletedTask::from_task(&task, Local::now()));
            }
        }
    }

    // Move accumulated in-memory history (task.log — e.g. manager
    // notes) out to the archive. Called on every persist.
    pub fn drain_history(&mut self, archive: &mut Archive) {
        for t in &mut self.tasks {
            for h in t.history.drain(..) {
                archive.events.push(crate::archive::HistoryEvent {
                    tag: t.tag.clone(),
                    task: t.name.clone(),
                    at: h.at,
                    note: h.note,
                });
            }
        }
    }

    // Selectable tasks grouped by tag, restricted to the configured
    // tags. Unconfigured tags are ignored (rather than panicking).
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

    pub fn task(&self, id: Uuid) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn task_mut(&mut self, id: Uuid) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    // Add a task; a new tag gets a new section heading at render time
    // order — record it so section order stays stable.
    pub fn add(&mut self, task: Task) {
        if !self.sections.contains(&task.tag) {
            self.sections.push(task.tag.clone());
        }
        self.tasks.push(task);
    }

    pub fn remove(&mut self, id: Uuid) -> bool {
        let before = self.tasks.len();
        self.tasks.retain(|t| t.id != id);
        self.tasks.len() != before
    }
}

// The v2.0 TOML store shape, read once during migration.
#[derive(Deserialize)]
struct LegacyStore {
    #[serde(default)]
    tasks: Vec<Task>,
}

// One-time conversion of a v2.0 tasks.toml: open tasks become
// tasks.md, done tasks and all history go to the archive, and the
// original is kept as tasks.toml.bak. Fails without touching anything
// if the legacy file doesn't parse. Refuses to run if `md_path`
// already exists, so a retry after a partial failure can never
// double-append to the archive.
pub fn migrate_from_toml(
    toml_path: &Path,
    md_path: &Path,
    archive_path: &Path,
) -> io::Result<String> {
    if md_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "tasks.md already exists; refusing to migrate over it",
        ));
    }
    let text = fs::read_to_string(toml_path)?;
    let legacy: LegacyStore =
        toml::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let mut archive = Archive::load_or_default(archive_path);
    let mut open = Vec::new();
    let (mut n_open, mut n_done) = (0, 0);
    for mut t in legacy.tasks {
        let last_at = t.history.iter().map(|h| h.at).max();
        for h in t.history.drain(..) {
            archive.events.push(crate::archive::HistoryEvent {
                tag: t.tag.clone(),
                task: t.name.clone(),
                at: h.at,
                note: h.note,
            });
        }
        if t.status == Status::Done {
            archive.completed.push(CompletedTask::from_task(
                &t,
                last_at.unwrap_or_else(Local::now),
            ));
            n_done += 1;
        } else {
            open.push(t);
            n_open += 1;
        }
    }

    let mut store = Store::seed(open);
    store.save(md_path)?;
    archive.save(archive_path)?;
    let bak = toml_path.with_extension("toml.bak");
    fs::rename(toml_path, &bak)?;
    Ok(format!(
        "migrated {n_open} open task(s) to {}, {n_done} done task(s) to the archive; \
         original saved as {}",
        md_path.display(),
        bak.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::Archive;
    use crate::task::{parse_due, Status};

    fn tmp_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("rkm-test-{}.md", Uuid::new_v4()))
    }

    fn store_with(tasks: Vec<Task>) -> Store {
        Store::seed(tasks)
    }

    #[test]
    fn complete_archives_nonrecurring_and_reschedules_recurring() {
        let mut arch = Archive::default();
        let t = Task::new("school".into(), "essay".into(), None);
        let mut r = Task::new("home".into(), "water plants".into(), None);
        r.recur = Some("3d".into());
        r.add_subtask("front".into());
        r.toggle_subtask(0);
        let (tid, rid) = (t.id, r.id);
        let mut store = store_with(vec![t, r]);

        // non-recurring: removed from the store, archived with a note
        assert!(store.complete(tid, &mut arch));
        assert!(store.task(tid).is_none());
        assert_eq!(arch.completed.len(), 1);
        assert_eq!(arch.completed[0].name, "essay");
        assert_eq!(arch.events.len(), 1);

        // recurring: stays, due pushed out, subtasks reset
        assert!(store.complete(rid, &mut arch));
        let r = store.task(rid).unwrap();
        assert_eq!(r.status, Status::Todo);
        assert!(!r.subtasks[0].done);
        let mins = r.minutes_until_due().unwrap();
        assert!(mins > 2 * 24 * 60 && mins <= 3 * 24 * 60);
        assert_eq!(arch.completed.len(), 1); // not archived
        assert_eq!(arch.events.len(), 2);

        assert!(!store.complete(Uuid::new_v4(), &mut arch));
    }

    #[test]
    fn file_side_x_completions_are_processed_on_load() {
        let mut arch = Archive::default();
        let mut store = Store::seed(Vec::new());
        store.tasks =
            mdstore::parse("## t\n- [x] done in editor\n- [x] chores  @every 1d\n- [ ] open\n")
                .tasks;
        assert!(store.process_file_completions(&mut arch));
        assert_eq!(store.tasks.len(), 2); // 'done in editor' archived
        assert_eq!(store.tasks[0].name, "chores");
        assert_eq!(store.tasks[0].status, Status::Todo); // rescheduled
        assert!(store.tasks[0].due.is_some());
        assert_eq!(arch.completed.len(), 1);
    }

    #[test]
    fn startup_sequence_processes_file_completions() {
        // Mirrors the sequence main.rs now runs at startup: load, run
        // process_file_completions, save, and reload — before any
        // has_selectable check. `[x]` marks typed while the app was
        // closed must archive/reschedule, not just sit there.
        let path = tmp_path();
        fs::write(
            &path,
            "## t\n- [x] done in editor\n- [x] chores  @every 1d\n",
        )
        .unwrap();

        let mut store = Store::load_or_default(&path);
        let mut arch = Archive::default();
        assert!(store.process_file_completions(&mut arch));

        // non-recurring task archived and gone from the store
        assert_eq!(store.tasks.len(), 1);
        assert!(!store.tasks.iter().any(|t| t.name == "done in editor"));
        assert_eq!(arch.completed.len(), 1);
        assert_eq!(arch.completed[0].name, "done in editor");

        // recurring task is back to Todo with a due date, still selectable
        let recurring = &store.tasks[0];
        assert_eq!(recurring.name, "chores");
        assert_eq!(recurring.status, Status::Todo);
        assert!(recurring.due.is_some());
        assert!(recurring.is_selectable());

        // persist and reload: no [x] task lines remain; the recurring
        // task line is back to "- [ ]"
        store.save(&path).unwrap();
        let reloaded = Store::load_or_default(&path);
        assert!(!reloaded.tasks.iter().any(|t| t.status == Status::Done));
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("[x]") && !text.contains("[X]"));
        assert!(text.contains("- [ ] chores"));

        fs::remove_file(&path).ok();
    }

    #[test]
    fn save_then_load_roundtrips_markdown() {
        let path = tmp_path();
        let mut t = Task::new(
            "school".into(),
            "thesis".into(),
            parse_due("2050-01-01 10:00"),
        );
        t.add_subtask("outline".into());
        let mut store = store_with(vec![t]);
        store.save(&path).unwrap();

        let loaded = Store::load_or_default(&path);
        assert_eq!(loaded.tasks.len(), 1);
        assert_eq!(loaded.tasks[0].name, "thesis");
        assert_eq!(loaded.tasks[0].subtasks.len(), 1);
        assert!(loaded.tasks[0].due.is_some());
        assert!(loaded.warnings.is_empty());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn rematch_ids_keeps_session_identity_by_tag_and_name() {
        let old = store_with(vec![Task::new("t".into(), "same".into(), None)]);
        let old_id = old.tasks[0].id;
        let mut new = store_with(vec![
            Task::new("t".into(), "same".into(), None),
            Task::new("t".into(), "brand new".into(), None),
        ]);
        let fresh_id = new.tasks[1].id;
        new.rematch_ids(&old);
        assert_eq!(new.tasks[0].id, old_id);
        assert_eq!(new.tasks[1].id, fresh_id);
    }

    #[test]
    fn rematch_ids_never_duplicates_ids_for_duplicate_names() {
        let old = store_with(vec![Task::new("t".into(), "same".into(), None)]);
        let old_id = old.tasks[0].id;
        let mut new = store_with(vec![
            Task::new("t".into(), "same".into(), None),
            Task::new("t".into(), "same".into(), None),
        ]);
        new.rematch_ids(&old);
        assert_ne!(new.tasks[0].id, new.tasks[1].id);
        let matches_old = [new.tasks[0].id, new.tasks[1].id]
            .iter()
            .filter(|id| **id == old_id)
            .count();
        assert_eq!(matches_old, 1);
    }

    #[test]
    fn external_change_tracks_mtime_and_missing_files() {
        let path = tmp_path();
        let mut store = store_with(vec![Task::new("t".into(), "a".into(), None)]);
        store.save(&path).unwrap();
        assert!(!store.external_change(&path));
        std::fs::remove_file(&path).unwrap();
        assert!(store.external_change(&path));
    }

    #[test]
    fn drain_history_moves_task_logs_to_archive_events() {
        let mut arch = Archive::default();
        let mut store = store_with(vec![Task::new("t".into(), "a".into(), None)]);
        store.tasks[0].log("manager note");
        store.drain_history(&mut arch);
        assert!(store.tasks[0].history.is_empty());
        assert_eq!(arch.events.len(), 1);
        assert_eq!(arch.events[0].note, "manager note");
        assert_eq!(arch.events[0].task, "a");
    }

    #[test]
    fn selectable_filters_done_and_unconfigured_tags() {
        let mut done = Task::new("school".into(), "done".into(), None);
        done.status = Status::Done;
        let store = store_with(vec![
            Task::new("school".into(), "keep".into(), None),
            done,
            Task::new("unknown".into(), "skip".into(), None),
        ]);
        let tags = vec!["school".to_string(), "job".to_string()];
        let grouped = store.selectable_by_tag(&tags);
        assert_eq!(grouped["school"].len(), 1);
        assert!(grouped["job"].is_empty());
        assert!(store.has_selectable(&tags));
    }

    #[test]
    fn load_or_default_is_empty_when_missing() {
        let path = tmp_path();
        assert!(Store::load_or_default(&path).tasks.is_empty());
    }

    #[test]
    fn migrates_legacy_toml_store_to_md_plus_archive() {
        let dir = std::env::temp_dir().join(format!("rkm-mig-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let toml_path = dir.join("tasks.toml");
        let md_path = dir.join("tasks.md");
        let arch_path = dir.join("archive.toml");
        std::fs::write(
            &toml_path,
            r#"
[[tasks]]
id = "41852356-070f-4955-9a5a-f4be8f3279e3"
tag = "job search"
name = "Apply somewhere"
due = "2026-07-10 10:00"
status = "done"

[[tasks.history]]
at = "2026-07-02T11:59:01.314545-04:00"
note = "completed"

[[tasks]]
tag = "general life"
name = "answer all messages"
status = "todo"
recur = "1d"

[[tasks.subtasks]]
name = "respond to whatsapp"
done = false
"#,
        )
        .unwrap();

        let summary = migrate_from_toml(&toml_path, &md_path, &arch_path).unwrap();
        assert!(summary.contains("1 open"));
        assert!(summary.contains("1 done"));

        let store = Store::load_or_default(&md_path);
        assert_eq!(store.tasks.len(), 1);
        assert_eq!(store.tasks[0].name, "answer all messages");
        assert_eq!(store.tasks[0].recur.as_deref(), Some("1d"));
        assert_eq!(store.tasks[0].subtasks.len(), 1);

        let arch = Archive::load_or_default(&arch_path);
        assert_eq!(arch.completed.len(), 1);
        assert_eq!(arch.completed[0].name, "Apply somewhere");
        // completed_at taken from the task's history
        assert_eq!(
            arch.completed[0]
                .completed_at
                .format("%Y-%m-%d")
                .to_string(),
            "2026-07-02"
        );
        assert_eq!(arch.events.len(), 1);

        assert!(!toml_path.exists());
        assert!(dir.join("tasks.toml.bak").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn migration_refuses_when_md_already_exists() {
        let dir = std::env::temp_dir().join(format!("rkm-mig2-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let toml_path = dir.join("tasks.toml");
        let md_path = dir.join("tasks.md");
        std::fs::write(&toml_path, "[[tasks]]\ntag = \"t\"\nname = \"a\"\n").unwrap();
        std::fs::write(&md_path, "## t\n").unwrap();
        let err = migrate_from_toml(&toml_path, &md_path, &dir.join("archive.toml")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(toml_path.exists()); // original untouched
        std::fs::remove_dir_all(&dir).ok();
    }
}
