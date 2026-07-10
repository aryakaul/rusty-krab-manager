# Markdown Task Store — Design

Date: 2026-07-10
Status: approved by user (conversation), pending spec review

## Problem

The single-TOML task store (`tasks.toml`) is the human-editable source
of truth, but hand-editing it hurts (all three confirmed with the user):

1. **Noise drowns the signal** — UUIDs, `[[tasks.history]]` blocks, and
   completed tasks clutter the file.
2. **TOML block structure** — `[[tasks.subtasks]]` attaches positionally
   to whatever `[[tasks]]` block precedes it; hand-placement is
   error-prone.
3. **Too verbose to write** — adding a task means a multi-line block of
   quoted keys; a capture should be one line.

The user wants full hand-editing (add, rename, re-tag, due/recur,
subtask checks, delete) in a friendly file, respected even while the
app runs, with machine bookkeeping out of sight.

**Constraint from project history:** v2 deliberately killed the v1.x
CSV↔JSON *sync* because the stale human file silently clobbered in-app
edits on launch. This design must not reintroduce a two-copy sync.

## Design summary

Two files, split by **role, not copies** — each datum lives in exactly
one place, so there is no bidirectional sync to go stale:

- **`tasks.md`** — the live task list; source of truth for everything
  hand-editable: names, tags (as sections), due, recurrence, subtasks,
  open/done state.
- **`archive.toml`** — machine-side, append-mostly, never hand-edited:
  completed non-recurring tasks (full record + completion time) and
  history events (completions, manager notes, recurring completions).

UUIDs disappear from disk. Identity is **(tag section, name)**.

## 1. `tasks.md` format

Location: `~/.config/rusty-krab-manager/tasks.md` (via
`store_filepath`, which now defaults to and points at a `.md`).

### Structure

- `## <tag>` headings partition the file into tag sections. A task's
  tag is the section it sits under. Section order is the user's and is
  preserved on rewrite. Sections whose tag is not in the config are
  parsed and kept (unselectable, as unconfigured tags are today).
- A task is one checkbox line, with optional annotations after the
  name (order-independent, two-space separated by convention):

      - [ ] find contacts at perimeter bio  @due 2026-07-11 08:00
      - [ ] answer all messages  @every 1d

  - `@due YYYY-MM-DD[ HH:MM]` — time optional; a bare date means end
    of that day (23:59).
  - `@every <interval>` — recurrence; same grammar as today's `recur`
    (`12h` / `3d` / `2w`).
  - The name ends at the first ` @due` / ` @every` token; a bad value
    after a recognized token is a parse warning (line preserved), and
    a literal `@due`/`@every` inside a name is not supported.
- Subtasks are checkboxes indented under their task (nesting is the
  attachment — no positional ambiguity):

      - [ ] answer all messages  @every 1d
        - [x] respond to whatsapp
        - [ ] respond to imessage

- Checkbox states: `[ ]` todo, `[x]` done, `[/]` in-progress (maps to
  `Status::InProgress`; optional to ever type).

### Semantics

- **Checking `- [x]` on a task line completes it.** On the next app
  read: a non-recurring task is archived (removed from the md,
  appended to `archive.toml`); a recurring task reschedules — due
  moves to read-time + interval (completion-anchored, matching TUI
  `c`), subtasks reset, the box renders back as `[ ]`.
- **Unknown lines are never lost.** Lines the parser doesn't
  understand (typo'd tasks, prose, comments) are preserved verbatim in
  place on rewrite and flagged in the TUI footer. Good lines still
  load; the file is never rejected wholesale.
- **Duplicate names in one section:** footer warning; both kept; first
  wins for identity matching.
- New tasks created by the app (TUI `a`, manager capture) are appended
  to the end of their tag section.

### Not in the file

UUIDs, history, completed non-recurring tasks.

## 2. `archive.toml` + identity

- **Completed tasks:** full record (name, tag, subtasks, due, recur,
  completion time) appended on completion, whether completed via TUI
  or via `- [x]` in the file. Feeds any future stats/review feature.
- **History events:** keyed by (tag, name); completion logs, manager
  `add_note` notes, recurring completion events. A recurring task
  accumulates history here while staying one line in the md.

### Identity rules

- In memory the app keeps per-session `Uuid`s (cursor, current task,
  manager targeting — session.rs is undisturbed). On each file read,
  tasks re-match to the previous in-memory set by (tag section, name),
  so selection/expansion/current task survive reloads.
- **File-side rename = delete + add.** Old history stays in the
  archive under the old name; if it was the current task, reroll.
  Renames via TUI/manager keep continuity (the app sees both sides).
- Accepted trade-off: history is currently write-only; ids-in-md was
  rejected as reintroducing the noise being evicted. If file renames
  prove common, add a same-section rename heuristic (one removed + one
  added in a single reload) later — no format change required.

## 3. Live-edit reconciliation

- **Reload on change:** the existing 250 ms tick checks the md's
  mtime; on change, reload, re-match identities, rebuild ALL TASKS,
  and apply file-side completions/reschedules. Editor saves appear in
  the TUI within one tick.
- **App writes only on its own mutations** (add/delete/complete,
  subtask toggle, manager tool application, recurring reschedule) —
  never on a timer.
- **Write guard:** before saving, re-check mtime. If the file changed
  since last read: reload first, re-apply the pending mutation on top
  by identity (if its task vanished, drop the mutation with a footer
  notice), then write. Worst case loses one keypress, never the file.
- **Atomic write:** temp file + rename (adopted from the hardening
  review). Save failures surface in the footer instead of being
  silently discarded (fixes `Session::persist`'s `let _ =`).
- A partially-unparseable file still reloads: valid lines load,
  invalid lines preserved + flagged.

## 4. Migration + config

- One-time, automatic, on startup: if `store_filepath` points at a
  `.toml` store (or `tasks.md` is missing and `tasks.toml` exists),
  convert: open tasks → `tasks.md`; done tasks + all history →
  `archive.toml`; original backed up as `tasks.toml.bak`; print a
  summary.
- Config default becomes `tasks.md`. Config template and file-header
  docs updated. No new config keys. `default_files` scaffolds an
  example `tasks.md`.

## 5. Code impact

- **`store.rs`:** md parser + renderer and archive handling behind the
  unchanged `Store` API (`load`, `save`, `complete`, `add`, `remove`,
  `selectable_by_tag`, `has_selectable`). Parser/renderer are pure
  functions.
- **`session.rs`:** only the mtime-reload hook in `handle_tick` plus
  footer notices (parse warnings, dropped mutation, save failure).
- **`task.rs`:** `Task` keeps its shape; serde-to-TOML remains for the
  archive. Status gains a `[/]` mapping.
- **`main.rs` / `manager.rs`:** the hardening items in Section 6
  (panic hook, modifier-aware key mapping, `--version`; reqwest
  client timeout).
- **Untouched:** selection math, manager tools, UI draw layer (beyond
  footer messages).

## 6. Hardening additions (from the 2026-07-10 scope review)

Small robustness fixes bundled into this work, since the store rewrite
already touches the riskiest of them:

- **Atomic store writes + surfaced save failures** — part of the new
  `save` (Section 3).
- **Panic hook restores the terminal:** `std::panic::set_hook` in
  `main.rs` disables raw mode and leaves the alternate screen before
  the panic message prints, so a panic can't garble the shell or hide
  its own message. Same restoration on `?`-error exit from the event
  loop.
- **Ctrl-C quits instead of masquerading as `c`:** the event loop
  currently forwards only `key.code`, so Ctrl-C arrives as a plain
  `'c'` (the complete-task key). Pass modifiers through; map Ctrl-C to
  quit from any mode.
- **HTTP timeout on manager requests:** both backends get a reqwest
  client timeout (120 s) so a hung local server can't leave
  `manager_busy` stuck forever. On timeout, the normal error path runs
  (footer/popup notice, busy flag cleared).
- **Real `--version`:** replace the hardcoded `"X.X"` with
  `env!("CARGO_PKG_VERSION")`.

## 7. Testing

Unit tests on the pure cores:

- Round-trip property: parse → render → parse is identical (including
  preserved unknown lines and section order).
- Annotation grammar: `@due` with/without time, `@every`, both, junk
  annotations → parse warning, name-only lines.
- Checkbox semantics: `[x]` task → archived; `[x]` recurring →
  rescheduled + reset; `[/]` ↔ InProgress; subtask `[x]`.
- Identity re-match across reloads (same name → same session id;
  rename → new id + reroll case).
- Write guard: external change + pending mutation → mutation
  re-applied; mutation on a file-deleted task → dropped.
- Migration: today's real `tasks.toml` shape → expected md + archive;
  idempotent (second run is a no-op); `.bak` created.
- Atomic save: failure path surfaces an error (no silent loss).
- Hardening: key-event mapping (Ctrl-C → quit, plain `c` unchanged) is
  unit-testable; version string asserts against `CARGO_PKG_VERSION`.
  Panic-hook and HTTP-timeout behavior are verified manually (panic
  smoke test; timeout against a non-responding local port).

## Out of scope (this design)

- Rename-detection heuristic (future option).
- Stats/review UI over `archive.toml` (future; this design feeds it —
  noted in the scope review as the one new feature worth building).
- Release hygiene (tag v2.x, wiki update) — user-driven, after the
  human TTY pass.
