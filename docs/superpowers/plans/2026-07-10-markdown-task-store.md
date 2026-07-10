# Markdown Task Store Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the TOML task store with a human-editable Markdown
checklist (`tasks.md`) plus a machine-side `archive.toml`, with live
external-edit reconciliation — and bundle five small hardening fixes.

**Architecture:** A pure parser/renderer module (`mdstore.rs`) turns
`tasks.md` text into `Task`s + preserved unknown lines and back. A new
`archive.rs` holds completed tasks and history events in TOML.
`store.rs` keeps its existing API surface (`load_or_default`, `save`,
`complete`, `add`, `remove`, `selectable_by_tag`, `has_selectable`)
over the new format, adding mtime tracking, id re-matching, and
file-side completion processing. `session.rs` reloads on external
change each tick and routes every mutation through a write-guarded
`commit`. One-time migration converts an existing `tasks.toml`.

**Tech Stack:** Rust (edition 2021, MSRV 1.89), existing deps only
(chrono, serde, toml, uuid, ratatui, crossterm, tokio, reqwest). No
new dependencies.

**Spec:** `docs/superpowers/specs/2026-07-10-markdown-task-store-design.md`

## Global Constraints

- **Git is the USER's job.** NEVER run `git add`/`git commit`/`git rm`.
  Where this plan says "Draft commit message", record the message in
  your final report — do not execute git.
- `AGENTS.md`, `AGENT_ALIGNMENT.md`, `AGENT_BESTPRACTICES.md` must not
  be modified; `AGENT_PROJECT.md` is read-only.
- Style: run `cargo fmt` after every change (project uses rustfmt
  defaults); `cargo clippy --all-targets` must stay warning-clean.
- Verification loop per task: `cargo fmt && cargo clippy --all-targets
  && cargo test`. The suite has 48 tests before this plan; only tests
  explicitly rewritten here may change.
- Selection math (`selection.rs`, `rand_utils.rs`) must not change.
- Comments: only where logic is not self-evident; match the codebase's
  existing comment voice.

---

### Task 1: Flexible due parsing (`parse_due_flexible`)

**Files:**
- Modify: `src/task.rs` (after `parse_due`, ~line 233)
- Test: `src/task.rs` `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: existing `parse_due`, `DUE_FORMAT`.
- Produces: `pub fn parse_due_flexible(s: &str) -> Option<DateTime<Local>>`
  (accepts `"YYYY-MM-DD HH:MM"` or bare `"YYYY-MM-DD"` → 23:59 that
  day). Also: change `mod due_fmt` (line 65) to `pub(crate) mod
  due_fmt` — Task 4's archive types reuse it.

- [ ] **Step 1: Write the failing test** (append inside `mod tests`):

```rust
    #[test]
    fn parse_due_flexible_accepts_date_only_as_end_of_day() {
        let full = parse_due_flexible("2050-01-01 10:00").unwrap();
        assert_eq!(full, parse_due("2050-01-01 10:00").unwrap());

        let bare = parse_due_flexible("2050-01-01").unwrap();
        assert_eq!(
            bare.format("%Y-%m-%d %H:%M").to_string(),
            "2050-01-01 23:59"
        );
        assert_eq!(parse_due_flexible(" 2050-01-01 "), Some(bare));
        assert!(parse_due_flexible("tomorrow").is_none());
        assert!(parse_due_flexible("").is_none());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test parse_due_flexible`
Expected: FAIL — `cannot find function parse_due_flexible`.

- [ ] **Step 3: Implement** (in `src/task.rs`, directly after
  `parse_due`; also change `mod due_fmt` → `pub(crate) mod due_fmt`):

```rust
// Like `parse_due`, but also accepts a bare "YYYY-MM-DD", meaning end
// of that day (23:59). Used by the Markdown store's @due annotation.
pub fn parse_due_flexible(s: &str) -> Option<DateTime<Local>> {
    let s = s.trim();
    parse_due(s).or_else(|| {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_opt(23, 59, 0))
            .and_then(|naive| Local.from_local_datetime(&naive).single())
    })
}
```

- [ ] **Step 4: Verify**

Run: `cargo fmt && cargo clippy --all-targets && cargo test`
Expected: all pass (49 tests).

- [ ] **Step 5: Draft commit message** (do not run git):
`feat(task): parse_due_flexible accepts date-only dues (end of day)`

---

### Task 2: `mdstore` parser

**Files:**
- Create: `src/mdstore.rs`
- Modify: `src/main.rs:6-15` (add `mod mdstore;` to the module list)

**Interfaces:**
- Consumes: `Task::new`, `Status`, `Subtask`, `parse_due_flexible`,
  `parse_recur` from `task.rs`; `Uuid`.
- Produces (used by Tasks 3 and 5):

```rust
pub struct ParsedDoc {
    pub preamble: Vec<String>,
    pub sections: Vec<String>,
    pub tasks: Vec<Task>,
    pub extras: Vec<Extra>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Extra {
    pub section: String,
    pub anchor: Option<Uuid>, // task line it follows; None = section top
    pub lines: Vec<String>,
}
pub fn parse(text: &str) -> ParsedDoc
```

Parsing rules (from the spec): `## <tag>` starts a section; lines
before the first section are preamble. An unindented `- [m] rest` is a
task (`m` ∈ ` `/`x`/`/` → Todo/Done/InProgress); its name ends at the
first ` @due`/` @every` token at a word boundary; `@due` takes
`parse_due_flexible`, `@every` must satisfy `parse_recur`. A bad or
duplicate annotation, unknown marker, or empty name → warning + the
whole line preserved as an `Extra`. An indented `- [ ]`/`- [x]` is a
subtask of the last task in the section (orphans → `Extra`). Duplicate
(tag, name) → warning, both kept. Any other non-blank line → `Extra`
anchored to the preceding task (or section top). Blank lines are not
preserved (the renderer emits canonical spacing).

- [ ] **Step 1: Write the failing tests** (bottom of the new
  `src/mdstore.rs` — create the file with just the tests and stub
  types first if you prefer, or write tests + implementation and rely
  on Step 2's targeted run):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::Status;

    const SAMPLE: &str = "\
# Tasks
Edit freely.

## job search
- [ ] Email Nikolai back  @due 2026-07-01 10:00
  - [x] Read about company
- [/] research tacit labs  @due 2026-07-06
some stray prose here

## general life
- [x] answer all messages  @every 1d
  - [ ] respond to whatsapp
";

    #[test]
    fn parses_sections_tasks_annotations_and_subtasks() {
        let doc = parse(SAMPLE);
        assert_eq!(doc.preamble, vec!["# Tasks", "Edit freely."]);
        assert_eq!(doc.sections, vec!["job search", "general life"]);
        assert_eq!(doc.tasks.len(), 3);

        let t0 = &doc.tasks[0];
        assert_eq!(t0.tag, "job search");
        assert_eq!(t0.name, "Email Nikolai back");
        assert_eq!(t0.due_str().as_deref(), Some("2026-07-01 10:00"));
        assert_eq!(t0.status, Status::Todo);
        assert_eq!(t0.subtasks.len(), 1);
        assert!(t0.subtasks[0].done);

        let t1 = &doc.tasks[1];
        assert_eq!(t1.status, Status::InProgress);
        // date-only @due = end of day
        assert_eq!(t1.due_str().as_deref(), Some("2026-07-06 23:59"));

        let t2 = &doc.tasks[2];
        assert_eq!(t2.status, Status::Done);
        assert_eq!(t2.recur.as_deref(), Some("1d"));

        // prose preserved, anchored to the task it followed
        assert_eq!(doc.extras.len(), 1);
        assert_eq!(doc.extras[0].section, "job search");
        assert_eq!(doc.extras[0].anchor, Some(doc.tasks[1].id));
        assert_eq!(doc.extras[0].lines, vec!["some stray prose here"]);
        assert!(doc.warnings.is_empty());
    }

    #[test]
    fn name_ends_at_first_annotation_token() {
        let doc = parse("## t\n- [ ] email bob @home about it  @due 2050-01-01\n");
        assert_eq!(doc.tasks[0].name, "email bob @home about it");
        assert!(doc.tasks[0].due.is_some());
    }

    #[test]
    fn bad_annotation_preserves_line_and_warns() {
        let doc = parse("## t\n- [ ] pay rent  @due tomorrow\n- [ ] ok task\n");
        assert_eq!(doc.tasks.len(), 1);
        assert_eq!(doc.tasks[0].name, "ok task");
        assert_eq!(doc.warnings.len(), 1);
        assert!(doc.warnings[0].contains("tomorrow"));
        assert_eq!(doc.extras[0].lines, vec!["- [ ] pay rent  @due tomorrow"]);
        assert_eq!(doc.extras[0].anchor, None); // preceded no task
    }

    #[test]
    fn duplicate_task_names_and_bad_every_warn() {
        let doc = parse("## t\n- [ ] a\n- [ ] a\n- [ ] b  @every fortnight\n");
        assert_eq!(doc.tasks.len(), 2); // both 'a's kept
        assert_eq!(doc.warnings.len(), 2);
    }

    #[test]
    fn orphan_subtask_and_unknown_marker_are_preserved() {
        let doc = parse("## t\n  - [ ] floating sub\n- [?] weird\n");
        assert!(doc.tasks.is_empty());
        assert_eq!(doc.extras.iter().flat_map(|e| &e.lines).count(), 2);
        assert_eq!(doc.warnings.len(), 2);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test mdstore`
Expected: FAIL to compile (module contents missing) — that counts;
add stubs only if you want cleaner failure output.

- [ ] **Step 3: Implement** (top of `src/mdstore.rs`, above the
  tests; add `mod mdstore;` to `main.rs`'s module list
  alphabetically, after `mod manager;`):

```rust
//! Parser and renderer for the human-editable Markdown task store
//! (`tasks.md`). Pure functions, no I/O. `parse` turns file text into
//! tasks plus preserved unknown lines; `render` (Task 3) writes the
//! canonical form back. Lines the parser doesn't understand are never
//! dropped — they come back out verbatim, in place.

use crate::task::{parse_due_flexible, parse_recur, Status, Subtask, Task};
use chrono::{DateTime, Local};
use uuid::Uuid;

pub struct ParsedDoc {
    pub preamble: Vec<String>,
    pub sections: Vec<String>,
    pub tasks: Vec<Task>,
    pub extras: Vec<Extra>,
    pub warnings: Vec<String>,
}

// A run of lines the parser didn't understand (prose, typos), kept
// verbatim and re-rendered after the task line it followed.
#[derive(Clone, Debug, PartialEq)]
pub struct Extra {
    pub section: String,
    pub anchor: Option<Uuid>,
    pub lines: Vec<String>,
}

// Split a checkbox line "- [m] rest" into (marker, rest).
fn checkbox(line: &str) -> Option<(char, &str)> {
    let rest = line.strip_prefix("- [")?;
    let mut chars = rest.chars();
    let mark = chars.next()?;
    let rest = chars.as_str().strip_prefix("] ")?;
    Some((mark, rest))
}

fn marker_status(m: char) -> Option<Status> {
    match m {
        ' ' => Some(Status::Todo),
        '/' => Some(Status::InProgress),
        'x' => Some(Status::Done),
        _ => None,
    }
}

// Byte offsets of @due/@every keywords sitting at a word boundary and
// followed by whitespace/end — sorted by position. The task name ends
// at the first one; a plain "@word" elsewhere stays part of the name.
fn annotation_starts(s: &str) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    for kw in ["@due", "@every"] {
        let mut from = 0;
        while let Some(rel) = s[from..].find(kw) {
            let i = from + rel;
            let before_ok = i == 0 || s[..i].ends_with(char::is_whitespace);
            let after = &s[i + kw.len()..];
            let after_ok = after.is_empty() || after.starts_with(char::is_whitespace);
            if before_ok && after_ok {
                found.push((i, kw));
            }
            from = i + kw.len();
        }
    }
    found.sort_unstable();
    found
}

// Parse the text after "- [m] ": name + optional @due/@every values.
fn parse_task_line(
    rest: &str,
) -> Result<(String, Option<DateTime<Local>>, Option<String>), String> {
    let anns = annotation_starts(rest);
    let name_end = anns.first().map_or(rest.len(), |&(i, _)| i);
    let name = rest[..name_end].trim().to_string();
    if name.is_empty() {
        return Err("task line has no name".to_string());
    }
    let mut due = None;
    let mut every = None;
    for (idx, &(start, kw)) in anns.iter().enumerate() {
        let end = anns.get(idx + 1).map_or(rest.len(), |&(j, _)| j);
        let value = rest[start + kw.len()..end].trim();
        match kw {
            "@due" => {
                if due.is_some() {
                    return Err(format!("duplicate @due on '{name}'"));
                }
                due = Some(parse_due_flexible(value).ok_or_else(|| {
                    format!("bad @due '{value}' on '{name}'")
                })?);
            }
            _ => {
                if every.is_some() {
                    return Err(format!("duplicate @every on '{name}'"));
                }
                if parse_recur(value).is_none() {
                    return Err(format!("bad @every '{value}' on '{name}'"));
                }
                every = Some(value.to_string());
            }
        }
    }
    Ok((name, due, every))
}

fn push_extra(extras: &mut Vec<Extra>, section: &str, anchor: Option<Uuid>, line: &str) {
    if let Some(e) = extras.last_mut() {
        if e.section == section && e.anchor == anchor {
            e.lines.push(line.to_string());
            return;
        }
    }
    extras.push(Extra {
        section: section.to_string(),
        anchor,
        lines: vec![line.to_string()],
    });
}

pub fn parse(text: &str) -> ParsedDoc {
    let mut doc = ParsedDoc {
        preamble: Vec::new(),
        sections: Vec::new(),
        tasks: Vec::new(),
        extras: Vec::new(),
        warnings: Vec::new(),
    };
    let mut section: Option<String> = None;
    let mut last_task: Option<Uuid> = None;

    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        if let Some(h) = line.strip_prefix("## ") {
            let tag = h.trim().to_string();
            if doc.sections.contains(&tag) {
                doc.warnings.push(format!("duplicate section '{tag}' merged"));
            } else {
                doc.sections.push(tag.clone());
            }
            section = Some(tag);
            last_task = None;
            continue;
        }
        let Some(tag) = section.clone() else {
            doc.preamble.push(line.to_string());
            continue;
        };
        let indented = raw.starts_with(' ') || raw.starts_with('\t');
        match (indented, checkbox(line.trim_start())) {
            // task line
            (false, Some((mark, rest))) => {
                let parsed = marker_status(mark)
                    .ok_or_else(|| format!("unknown marker '[{mark}]' in '{tag}'"))
                    .and_then(|st| parse_task_line(rest).map(|p| (st, p)));
                match parsed {
                    Ok((status, (name, due, every))) => {
                        if doc.tasks.iter().any(|t| t.tag == tag && t.name == name) {
                            doc.warnings
                                .push(format!("duplicate task '{name}' in '{tag}'"));
                        }
                        let mut t = Task::new(tag, name, due);
                        t.status = status;
                        t.recur = every;
                        last_task = Some(t.id);
                        doc.tasks.push(t);
                    }
                    Err(w) => {
                        doc.warnings.push(w);
                        push_extra(&mut doc.extras, &tag, last_task, line);
                    }
                }
            }
            // subtask line under the section's last task
            (true, Some((mark, rest))) if last_task.is_some() => {
                let done = match mark {
                    ' ' => Some(false),
                    'x' => Some(true),
                    _ => None,
                };
                match (done, rest.trim()) {
                    (Some(done), name) if !name.is_empty() => {
                        let id = last_task.expect("guarded by match arm");
                        let task = doc
                            .tasks
                            .iter_mut()
                            .find(|t| t.id == id)
                            .expect("last_task is always in tasks");
                        task.subtasks.push(Subtask {
                            name: name.to_string(),
                            done,
                        });
                    }
                    _ => {
                        doc.warnings.push(format!("unparseable subtask: {line}"));
                        push_extra(&mut doc.extras, &tag, last_task, line);
                    }
                }
            }
            // subtask with no task above it
            (true, Some(_)) => {
                doc.warnings.push(format!("subtask without a task: {line}"));
                push_extra(&mut doc.extras, &tag, last_task, line);
            }
            // anything else: preserved verbatim, silently (prose is fine)
            _ => push_extra(&mut doc.extras, &tag, last_task, raw),
        }
    }
    doc
}
```

Note the test `orphan_subtask_and_unknown_marker_are_preserved`
expects the orphan subtask to warn — the dedicated `(true, Some(_))`
arm above does that; plain prose does not warn.

- [ ] **Step 4: Verify**

Run: `cargo fmt && cargo clippy --all-targets && cargo test mdstore`
Expected: 5 mdstore tests pass; then `cargo test` → full suite green.

- [ ] **Step 5: Draft commit message:**
`feat(mdstore): parse the Markdown task-store format`

---

### Task 3: `mdstore` renderer + round-trip

**Files:**
- Modify: `src/mdstore.rs`

**Interfaces:**
- Produces:
```rust
pub fn render(
    preamble: &[String],
    sections: &[String],
    tasks: &[Task],
    extras: &[Extra],
) -> String
```
Canonical form: preamble verbatim, blank line before each `## `
heading, tasks in vec order under their tag's heading, two spaces
before each annotation, `@due` always rendered with full
`"YYYY-MM-DD HH:MM"`, subtasks indented two spaces. Extras render
after their anchor task (or at section top); an extra whose anchor
task no longer exists re-anchors to section top. A task whose tag has
no heading yet gets a new section appended.

- [ ] **Step 1: Write the failing tests** (append inside `mod tests`):

```rust
    #[test]
    fn render_writes_canonical_form_and_roundtrips() {
        let doc = parse(SAMPLE);
        let out = render(&doc.preamble, &doc.sections, &doc.tasks, &doc.extras);
        // date-only due came back canonicalized to 23:59
        assert!(out.contains("- [/] research tacit labs  @due 2026-07-06 23:59"));
        assert!(out.contains("\n\n## general life\n"));
        assert!(out.contains("some stray prose here\n"));
        // canonical form is a fixed point: parse(render(x)) == render(x)
        let doc2 = parse(&out);
        let out2 = render(&doc2.preamble, &doc2.sections, &doc2.tasks, &doc2.extras);
        assert_eq!(out, out2);
    }

    #[test]
    fn render_reanchors_extras_of_deleted_tasks_and_adds_new_sections() {
        let mut doc = parse("## a\n- [ ] gone\nnote under gone\n");
        doc.tasks.clear(); // the anchor task was deleted in-app
        doc.tasks.push(Task::new("brand new tag".into(), "fresh".into(), None));
        let out = render(&doc.preamble, &doc.sections, &doc.tasks, &doc.extras);
        assert!(out.contains("## a\nnote under gone\n")); // kept, at section top
        assert!(out.contains("## brand new tag\n- [ ] fresh\n"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test mdstore::tests::render`
Expected: FAIL — `render` not found.

- [ ] **Step 3: Implement** (in `src/mdstore.rs`; add
  `use std::collections::HashSet;` at the top):

```rust
pub fn render(
    preamble: &[String],
    sections: &[String],
    tasks: &[Task],
    extras: &[Extra],
) -> String {
    // Extras anchored to a task deleted in-app fall back to their
    // section top rather than vanishing.
    let live: HashSet<Uuid> = tasks.iter().map(|t| t.id).collect();
    let extras: Vec<Extra> = extras
        .iter()
        .cloned()
        .map(|mut e| {
            if e.anchor.is_some_and(|a| !live.contains(&a)) {
                e.anchor = None;
            }
            e
        })
        .collect();

    let mut ordered: Vec<&String> = sections.iter().collect();
    for t in tasks {
        if !ordered.contains(&&t.tag) {
            ordered.push(&t.tag);
        }
    }

    let mut out = String::new();
    for line in preamble {
        out.push_str(line);
        out.push('\n');
    }
    for section in ordered {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("## ");
        out.push_str(section);
        out.push('\n');
        emit_extras(&mut out, &extras, section, None);
        for task in tasks.iter().filter(|t| &t.tag == section) {
            render_task(&mut out, task);
            emit_extras(&mut out, &extras, section, Some(task.id));
        }
    }
    out
}

fn emit_extras(out: &mut String, extras: &[Extra], section: &str, anchor: Option<Uuid>) {
    for e in extras
        .iter()
        .filter(|e| e.section == section && e.anchor == anchor)
    {
        for line in &e.lines {
            out.push_str(line);
            out.push('\n');
        }
    }
}

fn render_task(out: &mut String, task: &Task) {
    let mark = match task.status {
        Status::Todo => ' ',
        Status::InProgress => '/',
        Status::Done => 'x',
    };
    out.push_str(&format!("- [{mark}] {}", task.name));
    if let Some(d) = task.due_str() {
        out.push_str(&format!("  @due {d}"));
    }
    if let Some(r) = &task.recur {
        out.push_str(&format!("  @every {r}"));
    }
    out.push('\n');
    for s in &task.subtasks {
        let m = if s.done { 'x' } else { ' ' };
        out.push_str(&format!("  - [{m}] {}\n", s.name));
    }
}
```

- [ ] **Step 4: Verify**

Run: `cargo fmt && cargo clippy --all-targets && cargo test`
Expected: full suite green.

- [ ] **Step 5: Draft commit message:**
`feat(mdstore): render tasks back to canonical Markdown (round-trip safe)`

---

### Task 4: `archive.rs`

**Files:**
- Create: `src/archive.rs`
- Modify: `src/main.rs` module list (add `mod archive;` first in the
  list)

**Interfaces:**
- Consumes: `Task`, `Subtask`, `pub(crate) due_fmt` (Task 1).
- Produces (used by Tasks 5, 6, 8):

```rust
pub struct CompletedTask { /* tag, name, due, recur, subtasks, completed_at */ }
pub struct HistoryEvent { pub tag: String, pub task: String,
                          pub at: DateTime<Local>, pub note: String }
#[derive(Default)]
pub struct Archive { pub completed: Vec<CompletedTask>,
                     pub events: Vec<HistoryEvent> }
impl Archive {
    pub fn load_or_default(path: &Path) -> Self
    pub fn save(&self, path: &Path) -> io::Result<()>
    pub fn event(&mut self, tag: &str, task: &str, note: impl Into<String>)
}
impl CompletedTask {
    pub fn from_task(t: &Task, at: DateTime<Local>) -> Self
}
pub fn archive_path(store_path: &Path) -> PathBuf  // sibling archive.toml
```
`save` uses `store::write_atomic` — Task 5 introduces it; for THIS
task use a local `fs::write` and switch to `write_atomic` in Task 5,
OR (preferred, to avoid churn) add the tiny helper here and have
store.rs reuse it: put `write_atomic` in `archive.rs` as
`pub(crate) fn write_atomic(path: &Path, contents: &str) -> io::Result<()>`.
This plan assumes the latter.

- [ ] **Step 1: Write the failing tests** (bottom of new
  `src/archive.rs`):

```rust
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
        a.completed.push(CompletedTask::from_task(&t, chrono::Local::now()));
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test archive`
Expected: compile failure (module missing).

- [ ] **Step 3: Implement:**

```rust
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
```

- [ ] **Step 4: Verify**

Run: `cargo fmt && cargo clippy --all-targets && cargo test`
Expected: green.

- [ ] **Step 5: Draft commit message:**
`feat(archive): machine-side archive.toml for completed tasks + history`

---

### Task 5: `store.rs` rewrite over the Markdown format

**Files:**
- Modify: `src/store.rs` (full rewrite of struct + impl; tests
  rewritten)

**Interfaces:**
- Consumes: `mdstore::{parse, render, Extra}`, `archive::{Archive,
  CompletedTask, write_atomic}`.
- Produces (session/main rely on these — unchanged unless noted):
```rust
pub struct Store { pub tasks: Vec<Task>, pub warnings: Vec<String>, /* private: preamble, sections, extras, last_mtime */ }
impl Store {
    pub fn load_or_default(path: &Path) -> Self            // unchanged sig
    pub fn save(&mut self, path: &Path) -> io::Result<()>  // now &mut (mtime)
    pub fn to_markdown(&self) -> String                     // replaces to_toml
    pub fn seed(tasks: Vec<Task>) -> Store                  // NEW: default preamble
    pub fn external_change(&self, path: &Path) -> bool      // NEW
    pub fn rematch_ids(&mut self, old: &Store)              // NEW
    pub fn process_file_completions(&mut self, archive: &mut Archive) -> bool // NEW
    pub fn drain_history(&mut self, archive: &mut Archive)  // NEW
    pub fn complete(&mut self, id: Uuid, archive: &mut Archive) -> bool // +archive arg
    // task/task_mut/add/remove/selectable_by_tag/has_selectable unchanged
}
```
Deleted: `STORE_HEADER`, `to_toml`, `load` (folded into
`load_or_default`; keep a private `fn load` if convenient).
**Semantics:** `complete` on a recurring task reschedules (due = now +
interval, subtasks reset, `event("… (recurs)")`); on a non-recurring
task it REMOVES the task from `tasks` and appends a `CompletedTask` +
event (previously it set `Status::Done` in place).
`process_file_completions` applies the same rule to every task the
parser loaded as `Status::Done` (i.e. `[x]` typed in the file).
`add` appends the tag to `sections` when it's new.

- [ ] **Step 1: Rewrite the tests** (replace the whole
  `#[cfg(test)] mod tests` in `src/store.rs`):

```rust
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
        store.tasks = mdstore::parse(
            "## t\n- [x] done in editor\n- [x] chores  @every 1d\n- [ ] open\n",
        )
        .tasks;
        assert!(store.process_file_completions(&mut arch));
        assert_eq!(store.tasks.len(), 2); // 'done in editor' archived
        assert_eq!(store.tasks[0].name, "chores");
        assert_eq!(store.tasks[0].status, Status::Todo); // rescheduled
        assert!(store.tasks[0].due.is_some());
        assert_eq!(arch.completed.len(), 1);
    }

    #[test]
    fn save_then_load_roundtrips_markdown() {
        let path = tmp_path();
        let mut t = Task::new("school".into(), "thesis".into(), parse_due("2050-01-01 10:00"));
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
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test store`
Expected: compile failure (`seed`, new signatures missing).

- [ ] **Step 3: Rewrite `src/store.rs`** (replace everything above
  the tests):

```rust
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
    // the current task all survive external edits.
    pub fn rematch_ids(&mut self, old: &Store) {
        for t in &mut self.tasks {
            if let Some(o) = old
                .tasks
                .iter()
                .find(|o| o.tag == t.tag && o.name == t.name)
            {
                t.id = o.id;
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
```

Note on `process_file_completions`: a recurring task is rescheduled
in place (status becomes Todo, so `i` advances); a removed task shifts
the next element into `i`, so `i` must NOT advance — the `is_some_and`
check handles both. Make sure the two new-in-this-task callers
(session, Task 8; migration, Task 6) don't exist yet; this task only
needs `cargo test` green, and `session.rs` still calls the OLD
`complete(id)` signature — **fix the two existing call sites now,
minimally**, so the crate compiles:
- `src/session.rs:427` (`complete_current`): change
  `self.store.complete(id);` to
  `self.store.complete(id, &mut Archive::default());` with a
  `use crate::archive::Archive;` import — TEMPORARY, replaced in
  Task 8 (the archive isn't wired into Session yet; events are
  discarded exactly as history was silently dropped before).
- `src/session.rs:463` (`persist`): `save` now takes `&mut self` —
  change `fn persist(&self)` to `fn persist(&mut self)` and
  `let _ = self.store.save(...)` stays. Callers already have
  `&mut self`.
- `src/main.rs:164`: `Store::load_or_default(&cfg.store_path)` —
  unchanged signature, still compiles.
- `src/default_files.rs:97-99` builds the example store with a
  struct literal + the deleted `to_toml`:

```rust
    Store { tasks }
        .to_toml()
        .expect("example store should serialize")
```
  The struct literal also breaks (Store gains private fields).
  Minimal compile fix now — Task 7 finishes the rename/paths:

```rust
    Store::seed(tasks).to_markdown()
```
  (The `.expect` goes away — `to_markdown` is infallible.)

- [ ] **Step 4: Verify**

Run: `cargo fmt && cargo clippy --all-targets && cargo test`
Expected: full suite green (store tests all rewritten; task/session/
settings/ui/manager tests untouched and passing).

- [ ] **Step 5: Draft commit message:**
`feat(store): Markdown-backed store with archive, mtime guard, id rematch`

---

### Task 6: Migration from the v2.0 TOML store

**Files:**
- Modify: `src/store.rs` (add `migrate_from_toml` + tests)

**Interfaces:**
- Consumes: `Task`'s serde (unchanged), `Archive`, `Store::seed`.
- Produces (main.rs calls in Task 8):
```rust
pub fn migrate_from_toml(
    toml_path: &Path, md_path: &Path, archive_path: &Path,
) -> io::Result<String>  // human-readable summary
```
Behavior: parse the legacy TOML (`{ tasks: Vec<Task> }`); open
(non-Done) tasks → a seeded `Store` saved to `md_path`; Done tasks →
`archive.completed` (completed_at = their latest history timestamp,
else now); ALL history entries → `archive.events`; rename the
original to `<name>.toml.bak`. Errors leave the original untouched.

- [ ] **Step 1: Write the failing test** (append inside store's
  `mod tests`):

```rust
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
            arch.completed[0].completed_at.format("%Y-%m-%d").to_string(),
            "2026-07-02"
        );
        assert_eq!(arch.events.len(), 1);

        assert!(!toml_path.exists());
        assert!(dir.join("tasks.toml.bak").exists());
        std::fs::remove_dir_all(&dir).ok();
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test migrates_legacy`
Expected: FAIL — `migrate_from_toml` not found.

- [ ] **Step 3: Implement** (in `src/store.rs`, below the `impl`;
  add `use serde::Deserialize;` if not present):

```rust
// The v2.0 TOML store shape, read once during migration.
#[derive(Deserialize)]
struct LegacyStore {
    #[serde(default)]
    tasks: Vec<Task>,
}

// One-time conversion of a v2.0 tasks.toml: open tasks become
// tasks.md, done tasks and all history go to the archive, and the
// original is kept as tasks.toml.bak. Fails without touching anything
// if the legacy file doesn't parse.
pub fn migrate_from_toml(
    toml_path: &Path,
    md_path: &Path,
    archive_path: &Path,
) -> io::Result<String> {
    let text = fs::read_to_string(toml_path)?;
    let legacy: LegacyStore =
        toml::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let mut archive = Archive::load_or_default(archive_path);
    let mut open = Vec::new();
    let (mut n_open, mut n_done) = (0, 0);
    for mut t in legacy.tasks {
        let last_at = t.history.last().map(|h| h.at);
        for h in t.history.drain(..) {
            archive.events.push(crate::archive::HistoryEvent {
                tag: t.tag.clone(),
                task: t.name.clone(),
                at: h.at,
                note: h.note,
            });
        }
        if t.status == Status::Done {
            archive
                .completed
                .push(CompletedTask::from_task(&t, last_at.unwrap_or_else(Local::now)));
            n_done += 1;
        } else {
            open.push(t);
            n_open += 1;
        }
    }

    let mut store = Store::seed(open);
    store.save(md_path)?;
    archive.save(archive_path)?;
    fs::rename(toml_path, toml_path.with_extension("toml.bak"))?;
    Ok(format!(
        "migrated {n_open} open task(s) to {}, {n_done} done task(s) to the archive; \
         original saved as tasks.toml.bak",
        md_path.display()
    ))
}
```

- [ ] **Step 4: Verify**

Run: `cargo fmt && cargo clippy --all-targets && cargo test`
Expected: green.

- [ ] **Step 5: Draft commit message:**
`feat(store): one-time migration from the v2.0 TOML store`

---

### Task 7: Config default, scaffolding, and template docs

**Files:**
- Modify: `src/settings_util.rs:47-50` (default store filename) and
  its test `store_path_defaults_under_config_dir`
- Modify: `src/default_files.rs` (scaffold `tasks.md`, drop the TOML
  example)
- Modify: `assets/config.template.toml:3-10` (store_filepath comment)

**Interfaces:**
- Consumes: `Store::seed`/`to_markdown` (Task 5).
- Produces: fresh installs get `tasks.md`; `store_filepath` defaults
  to `<config dir>/tasks.md`.

- [ ] **Step 1: Update the settings test** (in
  `src/settings_util.rs:216-223`, `store_path_defaults_under_config_dir`
  — change only the expected filename on line 221):

```rust
        assert_eq!(
            cfg.store_path,
            crate::default_files::config_dir().join("tasks.md")
        );
```
(Line 217's `BASE.replace("store_filepath = \"/tmp/rkm-test-tasks.toml\"\n", "")`
stays as-is — it strips the explicit setting to exercise the default.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test store_path_defaults`
Expected: FAIL (default still `tasks.toml`).

- [ ] **Step 3: Implement:**
  - `settings_util.rs` (~line 47): change the fallback filename:

```rust
    let store_path = settings
        .get::<String>("store_filepath")
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::default_files::config_dir().join("tasks.md"));
```
(Keep the existing structure — only the `"tasks.toml"` literal
changes to `"tasks.md"`.)

  - `default_files.rs`: line 40 becomes
    `let store_filepath = config_subdir.join("tasks.md");`. The
    example generator (Task 5 already switched its last line to
    `Store::seed(tasks).to_markdown()`) is renamed
    `example_store_toml` → `example_store_md` with its comment
    updated; everything else in it stays verbatim:

```rust
// An example task store demonstrating the format (sections, subtasks,
// due dates). Generated from the model so it never drifts from the
// types.
fn example_store_md() -> String {
    let mut homestead = Task::new(
        "personal".to_string(),
        "Build a defensible homestead 🏡".to_string(),
        parse_due("2050-01-01 11:11"),
    );
    homestead.subtasks = vec![
        Subtask {
            name: "Dig the moat".to_string(),
            done: false,
        },
        Subtask {
            name: "Raise the ramparts".to_string(),
            done: false,
        },
    ];

    let tasks = vec![
        Task::new(
            "school".to_string(),
            "Problem Set 📜".to_string(),
            parse_due("2050-10-12 10:00"),
        ),
        Task::new(
            "job".to_string(),
            "Code the thing 🖥️".to_string(),
            parse_due("2050-12-12 10:00"),
        ),
        homestead,
    ];

    Store::seed(tasks).to_markdown()
}
```
    Update the call site (line 61) to
    `create_default_file(&store_filepath, example_store_md().as_bytes());`,
    its `// Seed the single TOML store …` comment to say Markdown, and
    the file list in `create()`'s doc comment (line 33) from
    `tasks.toml` to `tasks.md`.

  - `assets/config.template.toml`: replace the `store_filepath`
    comment block (lines 3-9) with:

```toml
# path to the task store (Markdown). This is the human-editable task
# list: `## headings` are tags, `- [ ] name` lines are tasks, indented
# checkboxes are subtasks. Annotate with `@due 2026-01-31 17:00` (time
# optional) and `@every 3d` (recurring). Check a task off with [x] to
# complete it — finished tasks move to the sibling archive.toml, which
# the app maintains for you. Edits are picked up live while the app
# runs.
```

- [ ] **Step 4: Verify**

Run: `cargo fmt && cargo clippy --all-targets && cargo test`
Expected: green. Also headless smoke: `rm -rf /tmp/rkm-fresh && env
XDG_CONFIG_HOME=/tmp/rkm-fresh cargo run 2>&1 | head -5` → scaffolds
`tasks.md` (check `/tmp/rkm-fresh/rusty-krab-manager/tasks.md`
contains `## personal`), exits at the TTY boundary ("Device not
configured" is the expected headless stop).

- [ ] **Step 5: Draft commit message:**
`feat(config): default + scaffold tasks.md; document the new format`

---

### Task 8: Session integration — live reload, write guard, notices

**Files:**
- Modify: `src/session.rs` (Session fields, persist/commit/reload,
  call sites, tick hook, draw)
- Modify: `src/ui/mod.rs:375` (`draw_footer` notice parameter)
- Modify: `src/main.rs` (store-path resolution + migration call,
  Archive load, `Session::new` arg)

**Interfaces:**
- Consumes: everything from Tasks 5-7.
- Produces:
  - `Session::new(cfg, no_sound, store, archive: Archive, fx, manager,
    manager_available, manager_tx)` — new 4th parameter.
  - `fn commit(&mut self, apply: impl FnOnce(&mut Store, &mut Archive) -> bool)`
  - `fn reload_from_disk(&mut self)`, `fn set_notice(&mut self, msg:
    impl Into<String>)`
  - `pub fn draw_footer(f, area, manager_available: bool,
    notice: Option<&str>)`
  - main.rs: `fn resolve_store(cfg: &mut ConfigOptions)` runs the
    migration and rewrites `cfg.store_path` to the `.md` path.

- [ ] **Step 1: Session fields + helpers** (in `src/session.rs`):
  - Imports: `use crate::archive::{archive_path, Archive};`
  - Fields on `Session` (near `store`):

```rust
    archive: Archive,
    // Transient footer message (save errors, parse warnings, dropped
    // actions) with its expiry.
    notice: Option<(String, Instant)>,
```
  - `Session::new`: add `archive: Archive` as the 4th parameter
    (after `store`), initialize `archive`, `notice: None`.
  - Helpers (near `persist`):

```rust
    const NOTICE_TTL: Duration = Duration::from_secs(8);

    fn set_notice(&mut self, msg: impl Into<String>) {
        self.notice = Some((msg.into(), Instant::now() + Self::NOTICE_TTL));
    }

    // Persist the store + archive. The single place disk writes flow
    // through; failures surface in the footer instead of vanishing.
    fn persist(&mut self) {
        self.store.drain_history(&mut self.archive);
        if let Err(e) = self.store.save(&self.cfg.store_path) {
            self.set_notice(format!("⚠ save failed: {e}"));
        }
        if let Err(e) = self.archive.save(&archive_path(&self.cfg.store_path)) {
            self.set_notice(format!("⚠ archive save failed: {e}"));
        }
    }

    // Apply a store mutation behind the external-edit write guard: if
    // tasks.md changed on disk since our last read, reload (re-matching
    // ids) BEFORE applying, so we never clobber a hand edit. A mutation
    // whose target vanished in that reload returns false and is dropped
    // with a notice (spec §3).
    fn commit(&mut self, apply: impl FnOnce(&mut Store, &mut Archive) -> bool) {
        if self.store.external_change(&self.cfg.store_path) {
            self.reload_from_disk();
        }
        if apply(&mut self.store, &mut self.archive) {
            self.persist();
        } else {
            self.set_notice("task changed on disk — action dropped");
        }
        self.rebuild_task_list();
        self.refresh_current_subtasks();
        self.refresh_expanded_subs();
    }

    // Mirror an external edit of tasks.md into the running session.
    fn reload_from_disk(&mut self) {
        let mut fresh = Store::load_or_default(&self.cfg.store_path);
        fresh.rematch_ids(&self.store);
        self.store = fresh;
        // [x] marks typed in the file are completions: archive or
        // reschedule them, then write the result back.
        if self.store.process_file_completions(&mut self.archive) {
            self.persist();
        }
        if !self.store.warnings.is_empty() {
            self.set_notice(format!(
                "tasks.md: {} line(s) not understood (kept as-is)",
                self.store.warnings.len()
            ));
        }
        self.rebuild_task_list();
        self.refresh_current_subtasks();
        self.refresh_expanded_subs();
        // the current task may have been completed or deleted in the file
        if self
            .current_id
            .is_some_and(|id| self.store.task(id).is_none())
        {
            self.reroll(false);
        }
    }
```
(`Duration` here is `std::time::Duration` — already what `Instant`
math expects; add the import if missing.)

- [ ] **Step 2: Route the eight mutation sites through `commit`:**

  - `add_subtask_to` (line 380):
```rust
    fn add_subtask_to(&mut self, id: Uuid, name: String) {
        self.commit(|s, _| {
            s.task_mut(id).map(|t| t.add_subtask(name)).is_some()
        });
    }
```
  - `complete_current` (line 424) — `record_completion` must run
    BEFORE the commit (a non-recurring task is REMOVED by complete,
    and record_completion reads its tag from the store):
```rust
    fn complete_current(&mut self, id: Uuid) {
        self.record_completion(id);
        self.play_sound();
        self.commit(|s, a| s.complete(id, a));
        if self.current_id == Some(id) {
            self.reroll(false);
        }
    }
```
    (The old `else { self.rebuild_task_list() }` is covered by
    `commit`.) Remove the Task 5 temporary
    `&mut Archive::default()` hack here.
  - `toggle_selected_subtask` (line 438): keep the cursor
    destructure; replace the mutation + persist + refresh block with:
```rust
        self.commit(|s, _| {
            s.task_mut(parent_id)
                .is_some_and(|t| t.toggle_subtask(sub_index))
        });
```
    (Drop the now-redundant manual refresh calls — `commit` does
    them; `refresh_current_subtasks` is unconditional there and safe.)
  - `Adding` submit (line 765):
```rust
                InputEvent::Submitted => match parse_new_task(&input.buffer, &tag) {
                    Ok(task) => {
                        self.commit(|s, _| {
                            s.add(task);
                            true
                        });
                    }
```
  - `ConfirmDelete` `y` (line 780):
```rust
                KeyCode::Char('y') => {
                    self.commit(|s, _| s.remove(id));
                    if self.current_id == Some(id) {
                        self.reroll(false);
                    }
                }
```
  - `apply_manager_outcome` (line 1200):
```rust
    fn apply_manager_outcome(&mut self, outcome: manager::ManagerOutcome) {
        let id = outcome.task.id;
        self.commit(|s, _| {
            s.task_mut(id)
                .map(|slot| *slot = outcome.task)
                .is_some()
        });
    }
```
  - `apply_capture_outcome` (line 1214):
```rust
    fn apply_capture_outcome(&mut self, outcome: manager::CaptureOutcome) {
        self.commit(|s, _| {
            for task in outcome.tasks {
                s.add(task);
            }
            true
        });
    }
```
  - Planning-chat capture (line 1344): the `plan` borrow conflicts
    with `self.commit`, so buffer first. Replace the
    `for task in outcome.captured { … }` loop body with pushes into a
    local, and commit after the borrow ends:
```rust
        let mut captured: Vec<Task> = Vec::new();
        // …inside the existing `if let Some(plan) = &mut self.plan` /
        // Ok(outcome) arm, replacing the store.add loop:
                    for task in outcome.captured {
                        plan.added.push(task.name.clone());
                        captured.push(task);
                    }
        // …after the plan borrow ends (replacing `if added { … }`):
        if !captured.is_empty() {
            self.commit(|s, _| {
                for task in captured {
                    s.add(task);
                }
                true
            });
        }
```
(`use crate::task::Task;` is already imported in session.rs.)

- [ ] **Step 3: Tick hook + notice expiry** (top of `handle_tick`,
  line 1365, after the CheckInOffer block and BEFORE the
  paused/planning early-return — external edits must land even while
  paused):

```rust
        // Mirror external edits to tasks.md into the running session.
        if self.store.external_change(&self.cfg.store_path) {
            self.reload_from_disk();
        }
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, deadline)| Instant::now() >= *deadline)
        {
            self.notice = None;
        }
```

- [ ] **Step 4: Footer notice.** `src/ui/mod.rs:375`:

```rust
pub fn draw_footer(f: &mut Frame, area: Rect, manager_available: bool, notice: Option<&str>) {
    if let Some(msg) = notice {
        let p = Paragraph::new(Line::from(Span::styled(
            format!(" {msg}"),
            Style::default().fg(Color::Yellow),
        )));
        f.render_widget(p, area);
        return;
    }
    // …existing body unchanged…
```
And the call site (`session.rs:706`):
```rust
        draw_footer(
            f,
            root[1],
            self.manager_available,
            self.notice.as_ref().map(|(m, _)| m.as_str()),
        );
```

- [ ] **Step 5: main.rs wiring.** After
  `let cfg = settings_util::readin_settings(&config)?;` (line 161),
  resolve/migrate the store path, then load store + archive:

```rust
    let mut cfg = settings_util::readin_settings(&config)?;
    resolve_store(&mut cfg);

    let store = Store::load_or_default(&cfg.store_path);
    let archive = archive::Archive::load_or_default(&archive::archive_path(&cfg.store_path));
```
with (above `main`):
```rust
// Point the session at a Markdown store, migrating a legacy TOML one
// (v2.0 tasks.toml) in place on first run.
fn resolve_store(cfg: &mut settings_util::ConfigOptions) {
    let path = cfg.store_path.clone();
    let is_toml = path.extension().is_some_and(|e| e == "toml");
    let md = if is_toml {
        path.with_file_name("tasks.md")
    } else {
        path.clone()
    };
    // Migrate when the configured file is TOML, or when the md file is
    // missing but a sibling tasks.toml exists.
    let legacy = if is_toml {
        path.clone()
    } else {
        md.with_file_name("tasks.toml")
    };
    if !md.exists() && legacy.exists() {
        match store::migrate_from_toml(&legacy, &md, &archive::archive_path(&md)) {
            Ok(summary) => eprintln!("store: {summary}"),
            Err(e) => eprintln!("store: migration failed ({e}); starting fresh at {}", md.display()),
        }
    }
    if is_toml {
        eprintln!(
            "store: note — config store_filepath points at {}; now using {}",
            path.display(),
            md.display()
        );
    }
    cfg.store_path = md;
}
```
Pass `archive` into `Session::new` (4th arg, after `store`). Update
the `has_selectable` early-exit message to mention `tasks.md`.

- [ ] **Step 6: Verify**

Run: `cargo fmt && cargo clippy --all-targets && cargo test`
Expected: green (48+ tests). Headless smoke as in Task 7, plus a
migration smoke: copy a real v2.0 `tasks.toml` into a temp
XDG_CONFIG_HOME with a config pointing at it, run once, confirm
`tasks.md` + `archive.toml` + `tasks.toml.bak` exist and the summary
printed. Manual (needs TTY, defer to user): edit tasks.md while the
app runs → change appears within a tick; `[x]` a line → task
completes; save failure (chmod -w) → footer notice.

- [ ] **Step 7: Draft commit message:**
`feat(session): live tasks.md reload, guarded writes, footer notices`

---

### Task 9: Hardening — panic hook, Ctrl-C, run_loop, timeout, version

**Files:**
- Modify: `src/session.rs` (AppEvent + `map_key` + test)
- Modify: `src/main.rs` (panic hook, run_loop extraction, version)
- Modify: `src/manager.rs:349,519` (client timeout)

**Interfaces:**
- Produces: `AppEvent::Quit`;
  `pub fn map_key(key: crossterm::event::KeyEvent) -> AppEvent`;
  `fn http_client() -> reqwest::Client` (manager.rs, both backends).

- [ ] **Step 1: Write the failing test** (session.rs `mod tests`):

```rust
    #[test]
    fn ctrl_c_maps_to_quit_and_plain_c_stays_a_key() {
        use crossterm::event::{KeyEvent, KeyModifiers};
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(matches!(map_key(ctrl_c), AppEvent::Quit));
        let plain_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
        assert!(matches!(map_key(plain_c), AppEvent::Key(KeyCode::Char('c'))));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test ctrl_c_maps`
Expected: FAIL — no `Quit` variant / `map_key`.

- [ ] **Step 3: Implement:**
  - session.rs: add `Quit` to `AppEvent` (line 49) and:

```rust
// Map a terminal key event to an app event. In raw mode Ctrl-C is not
// a signal — it arrives as a key. Without this it would read as a
// plain 'c': the complete-task key.
pub fn map_key(key: crossterm::event::KeyEvent) -> AppEvent {
    use crossterm::event::KeyModifiers;
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        AppEvent::Quit
    } else {
        AppEvent::Key(key.code)
    }
}
```
  - main.rs: real version (line 133): `.version(env!("CARGO_PKG_VERSION"))`.
  - main.rs: panic hook, immediately before `enable_raw_mode()?;`:

```rust
    // Restore the terminal before any panic message prints, so a panic
    // can't leave the shell in raw mode/alt screen (and invisible).
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));
```
  - main.rs: extract the event loop so `?` errors also restore the
    terminal. Replace the `loop { … }` block and the trailing
    restore lines with:

```rust
    let res = run_loop(&mut terminal, &mut session, &mut manager_rx).await;

    // restore the terminal on exit (run_loop errors included)
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    session: &mut Session,
    manager_rx: &mut tokio::sync::mpsc::UnboundedReceiver<ManagerMsg>,
) -> Result<(), Box<dyn Error>> {
    let mut reader = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(250));
    loop {
        terminal.draw(|f| session.draw(f))?;
        let app_event = tokio::select! {
            maybe_event = reader.next() => match maybe_event {
                Some(Ok(CEvent::Key(key))) if key.kind == KeyEventKind::Press => {
                    session::map_key(key)
                }
                _ => continue,
            },
            maybe_result = manager_rx.recv() => match maybe_result {
                Some(result) => AppEvent::Manager(result),
                None => continue,
            },
            _ = ticker.tick() => AppEvent::Tick,
        };
        match app_event {
            AppEvent::Quit => return Ok(()),
            AppEvent::Key(code) => {
                if session.handle_key(code) {
                    return Ok(());
                }
            }
            AppEvent::Manager(result) => session.handle_manager(result),
            AppEvent::Tick => session.handle_tick(),
        }
    }
}
```
(Move the `reader`/`ticker` declarations into `run_loop`; delete them
from `main`. `AppEvent` needs importing as `session::{…}` already
provides it.)
  - manager.rs: above `ClaudeBackend` (near line 306):

```rust
// Both backends share a hard request timeout so a hung server can't
// leave the manager busy forever — the normal error path handles it.
fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}
```
Replace both `client: reqwest::Client::new(),` (lines 349 and 519)
with `client: http_client(),`.

- [ ] **Step 4: Verify**

Run: `cargo fmt && cargo clippy --all-targets && cargo test`
Expected: green. `cargo run -- --version` prints `2.0.0` (wrapped by
the binary name). Manual (defer to user's TTY pass): Ctrl-C quits
cleanly; a deliberate `panic!` leaves the shell usable.

- [ ] **Step 5: Draft commit message:**
`fix: terminal-safe panics, Ctrl-C quits, manager timeout, real --version`

---

### Task 10: CHANGELOG + final sweep

**Files:**
- Modify: `CHANGELOG.md` (append a `## 2.1.0` entry — the file runs
  oldest→newest, so append at the END)

- [ ] **Step 1: Append the CHANGELOG entry:**

```markdown
## 2.1.0
- BREAKING: the task store is now a Markdown checklist (`tasks.md`) —
  `## headings` are tags, `- [ ]` lines are tasks with `@due` /
  `@every` annotations, indented checkboxes are subtasks. An existing
  `tasks.toml` is migrated automatically on first run (original kept
  as `tasks.toml.bak`).
- Completed tasks and history now live in a machine-side
  `archive.toml`; the task file stays clean. Checking `[x]` in the
  file completes a task (recurring tasks reschedule).
- The app picks up hand edits to `tasks.md` live (within ~250 ms) and
  never clobbers them: writes are guarded, atomic, and save failures
  surface in the footer. Unrecognized lines are preserved verbatim.
- Hardening: panics restore the terminal; Ctrl-C quits (it used to
  read as `c` — the complete key); manager HTTP calls time out after
  120 s; `--version` reports the real version.
```

- [ ] **Step 2: Full verification sweep**

Run: `cargo fmt --check && cargo clippy --all-targets && cargo test`
Expected: all clean, all tests green (expect ~60+). Re-run the Task 7
fresh-install smoke and the Task 8 migration smoke. Report anything
the harness can't verify (interactive TTY behavior) as pending the
user's TTY pass.

- [ ] **Step 3: Draft commit message:**
`docs: changelog for the Markdown store + hardening (2.1.0)`

---

## Post-plan notes for the executor

- Known accepted limitations (do NOT "fix" these): file-side `[x]`
  completions don't bump the session's DONE TODAY counter; a file-side
  rename orphans that task's archive history (spec §2); blank lines
  and exact spacing inside tasks.md are canonicalized on the app's
  next write.
- The user's live store (`~/.config/rusty-krab-manager/tasks.toml`)
  will migrate on their first real run — do not touch it during
  development; use temp dirs (`XDG_CONFIG_HOME` override) for smokes.
- Interactive verification (TTY) is the user's; list what needs eyes
  in the final report: live-reload latency, `[x]`-in-editor
  completion, footer notices, Ctrl-C, panic recovery.
