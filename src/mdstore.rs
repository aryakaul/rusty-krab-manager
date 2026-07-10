//! Parser and renderer for the human-editable Markdown task store
//! (`tasks.md`). Pure functions, no I/O. `parse` turns file text into
//! tasks plus preserved unknown lines; `render` (Task 3) writes the
//! canonical form back. Lines the parser doesn't understand are never
//! dropped — they come back out verbatim, in place.

use crate::task::{parse_due_flexible, parse_recur, Status, Subtask, Task};
use chrono::{DateTime, Local};
use std::collections::HashSet;
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
        'x' | 'X' => Some(Status::Done),
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

// Parsed pieces of a task line: name, @due value, @every value.
type TaskLineParts = (String, Option<DateTime<Local>>, Option<String>);

// Parse the text after "- [m] ": name + optional @due/@every values.
fn parse_task_line(rest: &str) -> Result<TaskLineParts, String> {
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
                due = Some(
                    parse_due_flexible(value)
                        .ok_or_else(|| format!("bad @due '{value}' on '{name}'"))?,
                );
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
                doc.warnings
                    .push(format!("duplicate section '{tag}' merged"));
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
                    'x' | 'X' => Some(true),
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

    let mut ordered: Vec<String> = sections.to_vec();
    for t in tasks {
        if !ordered.contains(&t.tag) {
            ordered.push(t.tag.clone());
        }
    }
    // A section referenced only by preserved lines still renders —
    // nothing in the file is ever silently dropped.
    for e in &extras {
        if !ordered.contains(&e.section) {
            ordered.push(e.section.clone());
        }
    }

    let mut out = String::new();
    for line in preamble {
        out.push_str(line);
        out.push('\n');
    }
    for section in &ordered {
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
    fn uppercase_x_marks_task_and_subtask_done_and_canonicalizes_to_lowercase() {
        let doc = parse("## t\n- [X] big task\n  - [X] sub\n");
        assert_eq!(doc.tasks.len(), 1);
        assert_eq!(doc.tasks[0].status, Status::Done);
        assert_eq!(doc.tasks[0].subtasks.len(), 1);
        assert!(doc.tasks[0].subtasks[0].done);

        let out = render(&doc.preamble, &doc.sections, &doc.tasks, &doc.extras);
        assert!(out.contains("- [x] big task"));
        assert!(out.contains("- [x] sub"));
        assert!(!out.contains('X'));
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
        doc.tasks
            .push(Task::new("brand new tag".into(), "fresh".into(), None));
        let out = render(&doc.preamble, &doc.sections, &doc.tasks, &doc.extras);
        assert!(out.contains("## a\nnote under gone\n")); // kept, at section top
        assert!(out.contains("## brand new tag\n- [ ] fresh\n"));
    }

    #[test]
    fn render_keeps_extras_whose_section_lost_all_tasks() {
        let mut doc = parse("## a\n- [ ] only\nkept note\n");
        doc.tasks.clear();
        doc.sections.clear(); // caller dropped the section too
        let out = render(&doc.preamble, &doc.sections, &doc.tasks, &doc.extras);
        assert!(out.contains("## a\nkept note\n"));
    }
}
