//! The running TUI: all app state and the per-event handlers.
//!
//! `Session` owns the long-lived state (store, view-model, pomodoro phase,
//! input mode, LLM manager handles) and exposes a few methods the event loop in
//! `main` drives: `draw` (render a frame), `handle_key`, `handle_tick`, and
//! `handle_manager` (an off-thread LLM job finished). Everything else is a
//! private helper.
//!
//! Sections below: runtime types · construction · rendering · key handling ·
//! task & pomodoro logic · the LLM manager (message/capture/decompose) · the
//! startup planning chat.

use crate::effects::Effect;
use crate::manager::{self, ChatHistory, ManagerBackend};
use crate::selection::{choose_task, grouped_to_rows, Selection};
use crate::settings_util::ConfigOptions;
use crate::store::Store;
use crate::task::{parse_due, parse_recur, Status, Task};
use crate::ui::{
    draw_current_task, draw_footer, draw_gauge, draw_help, draw_input_popup, draw_plan, draw_popup,
    draw_sponge_popup, draw_tag_counter, draw_task_table, draw_weights, help_items, App,
    CurrentPanel, Cursor, Mode, Screen, SelectableTable, Speaker, TimerView, Turn,
};
use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

// ---- runtime types ----

type ManagerResult = Result<manager::ManagerOutcome, manager::ManagerError>;
type CaptureResult = Result<manager::CaptureOutcome, manager::ManagerError>;
type ChatResult = Result<manager::ChatOutcome, manager::ManagerError>;

// A finished off-thread manager job: a manage run (mutating a task), an NL
// capture (proposing new tasks), or a planning-chat turn.
pub enum ManagerMsg {
    Manage(ManagerResult),
    Capture(CaptureResult),
    Chat(ChatResult),
}

// A single iteration of the main loop reacts to either a keypress, the 250ms
// timer tick, or a finished manager run. `select!` resolves to one of these so
// the heavy handling lives in normal methods rather than inside the macro.
pub enum AppEvent {
    Key(KeyCode),
    Tick,
    Manager(ManagerMsg),
}

// A one-line text editor shared by every input popup: the live buffer plus an
// inline validation error.
struct TextInput {
    buffer: String,
    error: Option<String>,
}

// What a keypress did to a `TextInput`.
enum InputEvent {
    Edited,
    Submitted,
    Cancelled,
}

impl TextInput {
    fn new() -> Self {
        Self {
            buffer: String::new(),
            error: None,
        }
    }

    fn key(&mut self, code: KeyCode) -> InputEvent {
        match code {
            KeyCode::Enter => return InputEvent::Submitted,
            KeyCode::Esc => return InputEvent::Cancelled,
            KeyCode::Char(c) => {
                self.buffer.push(c);
                self.error = None;
            }
            KeyCode::Backspace => {
                self.buffer.pop();
            }
            _ => {}
        }
        InputEvent::Edited
    }
}

// What the keyboard is currently driving: normal commands, a text input, a
// confirmation, or an LLM result. Non-normal modes intercept all keys.
enum InputMode {
    Normal,
    // Step 1 of adding a task: pick a tag from the configured list.
    AddingTag {
        selected: usize,
    },
    // Step 2: name, optional due date, optional recur interval.
    Adding {
        input: TextInput,
        tag: String,
    },
    ConfirmDelete {
        id: Uuid,
        name: String,
    },
    // Confirm marking the current task complete.
    ConfirmComplete {
        id: Uuid,
        name: String,
    },
    // Text input for a subtask to append to the highlighted ALL TASKS task.
    AddingSubtask {
        input: TextInput,
        id: Uuid,
        name: String,
    },
    // Text input for a message to the manager about the highlighted task.
    Managing {
        input: TextInput,
        id: Uuid,
        name: String,
    },
    // Free-form text describing new task(s) to capture.
    Capturing {
        input: TextInput,
    },
    // Review the manager's proposed changes before persisting them.
    ConfirmManager {
        outcome: manager::ManagerOutcome,
    },
    // Review captured new tasks before adding them to the store.
    ConfirmCapture {
        outcome: manager::CaptureOutcome,
    },
    // Display the manager's reply (or an error). `Enter` continues the
    // exchange in the fullscreen planning chat (when there's a turn to
    // continue); any other key dismisses it.
    ManagerReply {
        text: String,
    },
    // Startup offer to plan the day with SpongeBob.
    PlanOffer,
    // The active planning conversation (state lives in `Session.plan`).
    Planning,
    // Offer to check in with the manager about `task`, either after finishing
    // it (`finished`) or at a work-period boundary (`!finished`). `y` opens the
    // planning chat; any other key, or `deadline` elapsing, dismisses it.
    CheckInOffer {
        deadline: Instant,
        task: String,
        finished: bool,
    },
}

// State of an in-progress planning conversation.
#[derive(Default)]
struct PlanState {
    // The running chat history, fed back into each turn (the wire format is
    // the backend's business — opaque here).
    messages: ChatHistory,
    // The displayed transcript.
    transcript: Vec<Turn>,
    // The current input line.
    input: String,
    // Names of tasks captured (and added) during this session.
    added: Vec<String>,
    // True while awaiting SpongeBob's reply.
    busy: bool,
}

const APP_TITLE: &str = "Rusty-Krab-Manager";

// The fixed prompt behind the one-key "decompose" action.
const DECOMPOSE_PROMPT: &str = "Break this task into a short ordered list of \
concrete next steps, adding each as a subtask. Don't duplicate subtasks that \
already exist, and don't invent work beyond what the task implies.";

// Fire-and-forget a desktop notification via the effects worker.
fn notify(fx: &Sender<Effect>, body: impl Into<String>) {
    let _ = fx.send(Effect::Notify {
        summary: APP_TITLE.to_string(),
        body: body.into(),
    });
}

// Parse an add-task input line: "name, [due], [recur]". The tag was already
// picked from the menu step. Extra comma-separated fields are recognized by
// format, in any order: a "YYYY-MM-DD HH:MM" due date or a recur interval
// like "12h" / "3d" / "2w". Names may not contain a comma. Returns a friendly
// error string for the popup on failure.
fn parse_new_task(input: &str, tag: &str) -> Result<Task, String> {
    let mut fields = input.split(',').map(str::trim);
    let name = fields.next().unwrap_or("");
    if name.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    let mut due = None;
    let mut recur = None;
    for f in fields.filter(|f| !f.is_empty()) {
        if let Some(d) = parse_due(f) {
            due = Some(d);
        } else if parse_recur(f).is_some() {
            recur = Some(f.to_string());
        } else {
            return Err(format!(
                "'{f}' is neither a due date (YYYY-MM-DD HH:MM) nor a recur \
                 interval (12h / 3d / 2w)"
            ));
        }
    }
    let mut task = Task::new(tag.to_string(), name.to_string(), due);
    task.recur = recur;
    Ok(task)
}

// Push a fresh roll into the app + weight table, returning the new current id.
fn apply_selection(
    app: &mut App,
    weight_table: &mut SelectableTable,
    sel: Selection,
) -> Option<Uuid> {
    *weight_table = SelectableTable::new(sel.weighttable);
    app.current = sel.current;
    app.current_subtasks = sel.current_subtasks;
    app.items = sel.items;
    app.clamp_selection();
    sel.chosen_id
}

// ---- the session ----

// Owns the long-lived TUI state plus the per-event handlers. `main` keeps only
// the async machinery (terminal, event sources) and dispatches into these.
pub struct Session {
    // configuration, fixed for the run
    cfg: ConfigOptions,
    no_sound: bool,

    // side-effect handles
    fx: Sender<Effect>,
    manager: Arc<dyn ManagerBackend>,
    manager_available: bool,
    manager_tx: tokio::sync::mpsc::UnboundedSender<ManagerMsg>,

    // mutable runtime state
    store: Store,
    app: App,
    weight_table: SelectableTable,
    current_id: Option<Uuid>,
    phase: Mode,
    min_break_ctr: i64,
    input_mode: InputMode,
    manager_busy: bool,
    screen: Screen,
    help_table: SelectableTable,
    // Active planning conversation, when one is open.
    plan: Option<PlanState>,
    // The prompt behind the manager run currently in flight, so it can be
    // paired with the reply once the run finishes.
    last_manager_prompt: Option<String>,
    // The most recent (prompt, reply) manager exchange that finished with a
    // plain text reply, offered as a fullscreen chat continuation from
    // `ManagerReply`.
    last_manager_turn: Option<(String, String)>,
}

impl Session {
    // ---- construction ----

    // Build a session from the loaded config and the already-created runtime
    // handles (store, effects worker, LLM backend, result channel).
    pub fn new(
        cfg: ConfigOptions,
        no_sound: bool,
        store: Store,
        fx: Sender<Effect>,
        manager: Arc<dyn ManagerBackend>,
        manager_available: bool,
        manager_tx: tokio::sync::mpsc::UnboundedSender<ManagerMsg>,
    ) -> Self {
        Session {
            help_table: SelectableTable::new(help_items(manager_available)),
            cfg,
            no_sound,
            fx,
            manager,
            manager_available,
            manager_tx,
            store,
            app: App::default(),
            weight_table: SelectableTable::new(Vec::new()),
            current_id: None,
            phase: Mode::Work,
            min_break_ctr: 0,
            input_mode: InputMode::Normal,
            manager_busy: false,
            screen: Screen::Tasks,
            plan: None,
            last_manager_prompt: None,
            last_manager_turn: None,
        }
    }

    // Seed the tag counter, roll the first task, and (when the manager is on)
    // open the startup planning offer.
    pub fn start(&mut self) {
        self.app.completed = self.cfg.tags.iter().map(|t| (t.clone(), 0)).collect();
        self.reroll(false);
        if self.manager_available {
            self.input_mode = InputMode::PlanOffer;
        }
    }

    // ---- task & pomodoro logic ----

    fn choose(&self) -> Selection {
        choose_task(
            &self.store,
            &self.cfg.tags,
            &self.cfg.initial_tag_weights,
            &self.cfg.use_due_dates,
        )
    }

    // Configured minutes for the current pomodoro phase. Keeps the gauge
    // countdown and the tick state machine on one mapping.
    fn phase_minutes(&self) -> i64 {
        match self.phase {
            Mode::Work => self.cfg.task_time,
            Mode::ShortBreak => self.cfg.min_break_time,
            Mode::LongBreak => self.cfg.max_break_time,
        }
    }

    // Count a completed work unit against the task's tag on DONE TODAY.
    fn record_completion(&mut self, id: Uuid) {
        let Some(tag) = self.store.task(id).map(|t| &t.tag) else {
            return;
        };
        if let Some(entry) = self.app.completed.iter_mut().find(|(t, _)| t == tag) {
            entry.1 += 1;
        }
    }

    // Roll the next task, announce it, and make it current. `break_over` only
    // changes the wording of the notification.
    fn reroll(&mut self, break_over: bool) {
        let sel = self.choose();
        if let Some(name) = &sel.chosen_name {
            let lead = if break_over {
                "Break's over — work on"
            } else {
                "Now work on"
            };
            notify(&self.fx, format!("{lead}: {name}"));
        }
        self.current_id = apply_selection(&mut self.app, &mut self.weight_table, sel);
    }

    // Rebuild the ALL TASKS list from the store without rerolling.
    fn rebuild_task_list(&mut self) {
        let grouped = self.store.selectable_by_tag(&self.cfg.tags);
        self.app.items = grouped_to_rows(&grouped, &self.cfg.tags);
        self.app.clamp_selection();
    }

    // Refresh the current-task subtask checklist from the store.
    fn refresh_current_subtasks(&mut self) {
        if let Some(t) = self.current_id.and_then(|id| self.store.task(id)) {
            self.app.current_subtasks = t.subtask_view();
        }
    }

    // Append a subtask to the task with `id`, persist, and refresh the display.
    fn add_subtask_to(&mut self, id: Uuid, name: String) {
        let found = self.store.task_mut(id).is_some_and(|t| {
            t.add_subtask(name);
            true
        });
        if found {
            self.persist();
            self.refresh_current_subtasks();
            self.refresh_expanded_subs();
            self.rebuild_task_list();
        }
    }

    // Reload the expanded task's subtask list from the store (it may have
    // changed, or the task may be gone).
    fn refresh_expanded_subs(&mut self) {
        let Some(id) = self.app.expanded else { return };
        match self.store.task(id) {
            Some(t) => self.app.expanded_subs = t.subtask_view(),
            None => self.app.clear_expansion(),
        }
    }

    // Enter on the ALL TASKS table: expand/collapse the task under the cursor.
    // On a subtask row, Enter collapses back to its parent task.
    fn toggle_expand(&mut self) {
        match self.app.cursor() {
            Some(Cursor::Task { id, .. }) => {
                if self.app.expanded == Some(id) {
                    self.app.clear_expansion();
                } else {
                    self.app.expanded = Some(id);
                    self.refresh_expanded_subs();
                }
            }
            Some(Cursor::Sub { parent_id, .. }) => self.app.collapse_onto(parent_id),
            None => {}
        }
        self.app.clamp_selection();
    }

    // Mark a task complete (per the `c` confirmation): count it, play the
    // sound, persist Done, and reroll if it was the in-progress task. Using the
    // captured id keeps it correct even if a tick rerolled in the meantime.
    fn complete_current(&mut self, id: Uuid) {
        self.record_completion(id);
        self.play_sound();
        self.store.complete(id);
        self.persist();
        if self.current_id == Some(id) {
            self.reroll(false);
        } else {
            self.rebuild_task_list();
        }
    }

    // Space on the ALL TASKS table: toggle the highlighted subtask done/undone.
    // No-op unless the cursor is on a subtask row.
    fn toggle_selected_subtask(&mut self) {
        let Some(Cursor::Sub {
            parent_id,
            sub_index,
            ..
        }) = self.app.cursor()
        else {
            return;
        };
        let toggled = self
            .store
            .task_mut(parent_id)
            .is_some_and(|t| t.toggle_subtask(sub_index));
        if toggled {
            self.persist();
            self.refresh_expanded_subs();
            if self.current_id == Some(parent_id) {
                self.refresh_current_subtasks();
            }
            self.rebuild_task_list();
        }
    }

    // Persist the store to disk. The single place all mutations save through.
    fn persist(&self) {
        let _ = self.store.save(&self.cfg.store_path);
    }

    // Fire-and-forget the completion sound via the effects worker. No-op when
    // muted (--no-sound) or, downstream, if the file is unreadable.
    fn play_sound(&self) {
        if self.no_sound {
            return;
        }
        let _ = self.fx.send(Effect::Sound {
            path: self.cfg.sound_path.clone(),
            volume: self.cfg.sound_volume as f32,
        });
    }

    // A dismissible check-in offer about `task` when the manager is available,
    // else back to normal. `finished` picks the celebratory (task done) vs the
    // neutral (worked a session) framing.
    fn check_in_offer_mode(&self, task: String, finished: bool) -> InputMode {
        if self.manager_available {
            InputMode::CheckInOffer {
                deadline: Instant::now() + Duration::from_secs(60),
                task,
                finished,
            }
        } else {
            InputMode::Normal
        }
    }

    // ---- rendering ----

    // Render one frame.
    pub fn draw(&mut self, f: &mut Frame) {
        // The planning conversation takes over the whole screen.
        if matches!(self.input_mode, InputMode::Planning) {
            if let Some(plan) = &self.plan {
                draw_plan(
                    f,
                    f.area(),
                    &plan.transcript,
                    &plan.input,
                    &plan.added,
                    plan.busy,
                );
            }
            return;
        }

        // Derive the timer view from the current phase so the gauge can show
        // the mode, color, time remaining, and pomodoro progress.
        let timer = TimerView {
            mode: self.phase,
            phase_minutes: self.phase_minutes(),
            pom_index: self.min_break_ctr + 1,
            pom_total: self.cfg.maxno_min_breaks + 1,
        };

        // Reserve the bottom row for the persistent keybinding footer.
        let root = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)].as_ref())
            .split(f.area());
        let body = root[0];

        match self.screen {
            Screen::Help => draw_help(f, &mut self.help_table, body),
            Screen::Stats => draw_weights(f, &mut self.weight_table, body),
            Screen::Tasks => {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(
                        [
                            Constraint::Percentage(30),
                            Constraint::Percentage(50),
                            Constraint::Percentage(20),
                        ]
                        .as_ref(),
                    )
                    .split(body);
                let mini_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(75), Constraint::Percentage(25)].as_ref())
                    .split(chunks[0]);
                draw_gauge(f, &self.app, chunks[2], &timer);
                draw_task_table(f, &mut self.app, chunks[1]);
                draw_current_task(f, &self.app, mini_chunks[0]);
                draw_tag_counter(f, &self.app, mini_chunks[1]);
            }
        }

        // overlay the active input popup, if any
        match &self.input_mode {
            InputMode::AddingTag { selected } => {
                let mut body = String::from("Pick a tag for the new task:\n\n");
                for (i, t) in self.cfg.tags.iter().enumerate() {
                    let marker = if i == *selected { "▶" } else { " " };
                    body.push_str(&format!(" {marker} {}. {t}\n", i + 1));
                }
                body.push_str("\nj/k = move   ⏎ = choose   1-9 = quick pick   Esc = cancel");
                draw_popup(f, "New task — tag", &body);
            }
            InputMode::Adding { input, tag } => {
                let intro = format!(
                    "New #{tag} task.\n\
                     format: name, [due YYYY-MM-DD HH:MM], [recur e.g. 3d]"
                );
                draw_input_popup(
                    f,
                    "New task",
                    &intro,
                    &input.buffer,
                    input.error.as_deref(),
                    "save",
                );
            }
            InputMode::ConfirmDelete { name, .. } => {
                let body = format!("Delete \"{name}\"?\n\ny = yes   n / Esc = no");
                draw_popup(f, "Confirm delete", &body);
            }
            InputMode::ConfirmComplete { name, .. } => {
                let body = format!("Complete \"{name}\"?\n\ny = yes   n / Esc = no");
                draw_popup(f, "Confirm complete", &body);
            }
            InputMode::AddingSubtask { input, name, .. } => {
                let intro = format!(
                    "Add a subtask to \"{name}\".\n\
                     Subtasks are done in the order added."
                );
                draw_input_popup(
                    f,
                    "New subtask",
                    &intro,
                    &input.buffer,
                    input.error.as_deref(),
                    "add",
                );
            }
            InputMode::Managing { input, name, .. } => {
                draw_input_popup(
                    f,
                    &format!("Manage \"{name}\""),
                    "",
                    &input.buffer,
                    input.error.as_deref(),
                    "send",
                );
            }
            InputMode::Capturing { input } => {
                draw_input_popup(
                    f,
                    "Capture new tasks",
                    "Describe the task(s) in plain words — SpongeBob will \
                     sort out the tags and steps.",
                    &input.buffer,
                    input.error.as_deref(),
                    "capture",
                );
            }
            InputMode::ConfirmManager { outcome } => {
                let speech = if outcome.reply.is_empty() {
                    "Here's your game plan!"
                } else {
                    outcome.reply.as_str()
                };
                let mut extra = String::from("Here's what I'd change:\n");
                for c in &outcome.changes {
                    extra.push_str(&format!("  • {c}\n"));
                }
                extra.push_str("\ny = apply   ⏎ = apply & keep chatting   n / Esc = discard");
                draw_sponge_popup(f, speech, &extra);
            }
            InputMode::ConfirmCapture { outcome } => {
                let speech = if outcome.reply.is_empty() {
                    "Caught these for ya!"
                } else {
                    outcome.reply.as_str()
                };
                let mut extra = String::from("New tasks:\n");
                for t in &outcome.tasks {
                    let due = t
                        .due_str()
                        .map(|d| format!(" · due {d}"))
                        .unwrap_or_default();
                    let recur = t
                        .recur
                        .as_deref()
                        .map(|r| format!(" · ↻ {r}"))
                        .unwrap_or_default();
                    let subs = if t.subtasks.is_empty() {
                        String::new()
                    } else {
                        format!(" · {} subs", t.subtasks.len())
                    };
                    extra.push_str(&format!(
                        "  • {} #{}{}{}{}\n",
                        t.name, t.tag, due, recur, subs
                    ));
                }
                for n in &outcome.notes {
                    extra.push_str(&format!("  ⚠ {n}\n"));
                }
                extra.push_str("\ny = add   ⏎ = add & keep chatting   n / Esc = discard");
                draw_sponge_popup(f, speech, &extra);
            }
            InputMode::ManagerReply { text } => {
                let hint = if self.last_manager_turn.is_some() {
                    "Enter = continue chat   ·   any other key = dismiss"
                } else {
                    "press any key to dismiss"
                };
                draw_sponge_popup(f, text, hint);
            }
            InputMode::PlanOffer => {
                draw_sponge_popup(
                    f,
                    "Hiya! Want to plan the day together?",
                    "P = plan with me   ·   any other key = skip",
                );
            }
            InputMode::CheckInOffer { task, finished, .. } => {
                let speech = if *finished {
                    format!("Nice work finishing '{task}'! Want to check in about what's next?")
                } else {
                    format!("That's a session on '{task}'! How'd it go — want to plan what's next?")
                };
                draw_sponge_popup(
                    f,
                    &speech,
                    "y = chat   ·   any other key = dismiss (auto-dismisses in a bit)",
                );
            }
            // Planning draws full-screen above (early return in `draw`).
            InputMode::Planning | InputMode::Normal => {}
        }

        // while a manager run is in flight, show a thinking indicator (the
        // reply popup replaces it once the result arrives)
        if self.manager_busy && matches!(self.input_mode, InputMode::Normal) {
            draw_sponge_popup(f, "Cookin' up a plan…", "");
        }

        // persistent keybinding hints along the bottom
        draw_footer(f, root[1], self.manager_available);
    }

    // ---- key handling ----

    // Handle a keypress. Returns true when the app should quit. Non-normal
    // modes own the key: the mode is taken by value (leaving Normal behind),
    // each arm acts with full access to `&mut self`, and puts the mode back
    // whenever it should persist.
    pub fn handle_key(&mut self, code: KeyCode) -> bool {
        let mode = std::mem::replace(&mut self.input_mode, InputMode::Normal);
        match mode {
            // ---- normal command mode ----
            InputMode::Normal => return self.handle_normal_key(code),

            // ---- add-task tag menu ----
            InputMode::AddingTag { selected } => {
                let count = self.cfg.tags.len();
                let chosen = match code {
                    KeyCode::Esc => None,
                    KeyCode::Enter => Some(selected),
                    KeyCode::Char('j') | KeyCode::Down => {
                        self.input_mode = InputMode::AddingTag {
                            selected: (selected + 1) % count,
                        };
                        None
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        self.input_mode = InputMode::AddingTag {
                            selected: (selected + count - 1) % count,
                        };
                        None
                    }
                    KeyCode::Char(c @ '1'..='9') => {
                        let idx = c as usize - '1' as usize;
                        if idx < count {
                            Some(idx)
                        } else {
                            self.input_mode = InputMode::AddingTag { selected };
                            None
                        }
                    }
                    _ => {
                        self.input_mode = InputMode::AddingTag { selected };
                        None
                    }
                };
                if let Some(idx) = chosen {
                    self.input_mode = InputMode::Adding {
                        input: TextInput::new(),
                        tag: self.cfg.tags[idx].clone(),
                    };
                }
            }

            // ---- add-task text input ----
            InputMode::Adding { mut input, tag } => match input.key(code) {
                InputEvent::Cancelled => {}
                InputEvent::Edited => self.input_mode = InputMode::Adding { input, tag },
                InputEvent::Submitted => match parse_new_task(&input.buffer, &tag) {
                    Ok(task) => {
                        self.store.add(task);
                        self.persist();
                        self.rebuild_task_list();
                    }
                    Err(e) => {
                        input.error = Some(e);
                        self.input_mode = InputMode::Adding { input, tag };
                    }
                },
            },

            // ---- delete confirmation ----
            InputMode::ConfirmDelete { id, name } => match code {
                KeyCode::Char('y') => {
                    self.store.remove(id);
                    self.persist();
                    if self.current_id == Some(id) {
                        // we deleted the in-progress task: reroll
                        self.reroll(false);
                    } else {
                        self.rebuild_task_list();
                    }
                }
                KeyCode::Char('n') | KeyCode::Esc => {}
                _ => self.input_mode = InputMode::ConfirmDelete { id, name },
            },

            // ---- complete confirmation ----
            InputMode::ConfirmComplete { id, name } => match code {
                KeyCode::Char('y') => {
                    self.complete_current(id);
                    self.input_mode = self.check_in_offer_mode(name, true);
                }
                KeyCode::Char('n') | KeyCode::Esc => {}
                _ => self.input_mode = InputMode::ConfirmComplete { id, name },
            },

            // ---- add-subtask text input ----
            InputMode::AddingSubtask {
                mut input,
                id,
                name,
            } => match input.key(code) {
                InputEvent::Cancelled => {}
                InputEvent::Edited => {
                    self.input_mode = InputMode::AddingSubtask { input, id, name };
                }
                InputEvent::Submitted => {
                    let sub = input.buffer.trim().to_string();
                    if sub.is_empty() {
                        input.error = Some("subtask name cannot be empty".to_string());
                        self.input_mode = InputMode::AddingSubtask { input, id, name };
                    } else {
                        self.add_subtask_to(id, sub);
                    }
                }
            },

            // ---- message-the-manager text input ----
            InputMode::Managing {
                mut input,
                id,
                name,
            } => match input.key(code) {
                InputEvent::Cancelled => {}
                InputEvent::Edited => {
                    self.input_mode = InputMode::Managing { input, id, name };
                }
                InputEvent::Submitted => {
                    if input.buffer.trim().is_empty() {
                        input.error = Some("type a message for the manager".to_string());
                        self.input_mode = InputMode::Managing { input, id, name };
                    } else {
                        // Snapshot the highlighted task and run the manager
                        // off-thread; the result returns via the channel.
                        self.spawn_manager(id, input.buffer);
                    }
                }
            },

            // ---- capture-new-tasks text input ----
            InputMode::Capturing { mut input } => match input.key(code) {
                InputEvent::Cancelled => {}
                InputEvent::Edited => self.input_mode = InputMode::Capturing { input },
                InputEvent::Submitted => {
                    if input.buffer.trim().is_empty() {
                        input.error = Some("describe a task to capture".to_string());
                        self.input_mode = InputMode::Capturing { input };
                    } else {
                        self.spawn_capture(input.buffer);
                    }
                }
            },

            // ---- review the manager's proposed changes ----
            InputMode::ConfirmManager { outcome } => match code {
                KeyCode::Char('y') => self.apply_manager_outcome(outcome),
                // apply, then continue the exchange in the fullscreen chat
                KeyCode::Enter if self.last_manager_turn.is_some() => {
                    self.apply_manager_outcome(outcome);
                    self.continue_last_chat();
                }
                KeyCode::Char('n') | KeyCode::Esc => {}
                _ => self.input_mode = InputMode::ConfirmManager { outcome },
            },

            // ---- review captured new tasks ----
            InputMode::ConfirmCapture { outcome } => match code {
                KeyCode::Char('y') => self.apply_capture_outcome(outcome),
                // apply, then continue the exchange in the fullscreen chat
                KeyCode::Enter if self.last_manager_turn.is_some() => {
                    self.apply_capture_outcome(outcome);
                    self.continue_last_chat();
                }
                KeyCode::Char('n') | KeyCode::Esc => {}
                _ => self.input_mode = InputMode::ConfirmCapture { outcome },
            },

            // ---- manager reply (Enter continues the chat when there is an
            // exchange to continue; any other key dismisses) ----
            InputMode::ManagerReply { .. } => {
                if code == KeyCode::Enter {
                    self.continue_last_chat();
                }
            }

            // ---- startup planning offer ----
            InputMode::PlanOffer => {
                if matches!(code, KeyCode::Char('p') | KeyCode::Char('P')) {
                    self.open_plan_after(None);
                }
            }

            // ---- check-in offer (task done, or a work period ended) ----
            InputMode::CheckInOffer { task, finished, .. } => {
                if matches!(code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                    self.open_plan_after(Some((task, finished)));
                }
            }

            // ---- active planning conversation ----
            InputMode::Planning => {
                if code == KeyCode::Esc {
                    self.plan = None;
                } else {
                    self.input_mode = InputMode::Planning;
                    let mut send: Option<String> = None;
                    if let Some(plan) = &mut self.plan {
                        if !plan.busy {
                            match code {
                                KeyCode::Char(c) => plan.input.push(c),
                                KeyCode::Backspace => {
                                    plan.input.pop();
                                }
                                KeyCode::Enter => {
                                    let text = plan.input.trim().to_string();
                                    if !text.is_empty() {
                                        plan.input.clear();
                                        send = Some(text);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if let Some(text) = send {
                        self.send_plan(text, true);
                    }
                }
            }
        }
        false
    }

    fn handle_normal_key(&mut self, code: KeyCode) -> bool {
        match code {
            // mark the current task complete (asks to confirm first)
            KeyCode::Char('c') => {
                if matches!(self.phase, Mode::Work) && !self.app.paused {
                    if let Some(id) = self.current_id {
                        let name = self
                            .store
                            .task(id)
                            .map(|t| t.name.clone())
                            .unwrap_or_default();
                        self.input_mode = InputMode::ConfirmComplete { id, name };
                    }
                }
            }

            // reroll without marking complete
            KeyCode::Char('r') => {
                if matches!(self.phase, Mode::Work) && !self.app.paused {
                    self.reroll(false);
                }
            }

            // add a new task (opens the tag menu, then the input popup)
            KeyCode::Char('a') => {
                if self.screen == Screen::Tasks && !self.cfg.tags.is_empty() {
                    self.input_mode = InputMode::AddingTag { selected: 0 };
                }
            }

            // expand / collapse the highlighted task's subtasks inline
            KeyCode::Enter => {
                if self.screen == Screen::Tasks {
                    self.toggle_expand();
                }
            }

            // toggle the highlighted subtask done (when the cursor is on one)
            KeyCode::Char(' ') => {
                if self.screen == Screen::Tasks {
                    self.toggle_selected_subtask();
                }
            }

            // add a subtask to the highlighted task (opens an input popup)
            KeyCode::Char('A') => {
                if self.screen == Screen::Tasks {
                    if let Some((id, name)) = self.app.selected_task() {
                        self.input_mode = InputMode::AddingSubtask {
                            input: TextInput::new(),
                            id,
                            name,
                        };
                    }
                }
            }

            // delete the highlighted task (asks to confirm)
            KeyCode::Char('d') => {
                if self.screen == Screen::Tasks {
                    if let Some((id, name)) = self.app.selected_task() {
                        self.input_mode = InputMode::ConfirmDelete { id, name };
                    }
                }
            }

            // message the manager about the highlighted task
            KeyCode::Char('m') => {
                if self.manager_available && !self.manager_busy && self.screen == Screen::Tasks {
                    if let Some((id, name)) = self.app.selected_task() {
                        self.input_mode = InputMode::Managing {
                            input: TextInput::new(),
                            id,
                            name,
                        };
                    }
                }
            }

            // capture new task(s) from a natural-language description
            KeyCode::Char('N') => {
                if self.manager_available && !self.manager_busy && self.screen == Screen::Tasks {
                    self.input_mode = InputMode::Capturing {
                        input: TextInput::new(),
                    };
                }
            }

            // decompose the highlighted task into subtasks (fixed-prompt manager)
            KeyCode::Char('D') => {
                if self.manager_available && !self.manager_busy && self.screen == Screen::Tasks {
                    if let Some((id, _name)) = self.app.selected_task() {
                        self.spawn_manager(id, DECOMPOSE_PROMPT.to_string());
                    }
                }
            }

            // fast forward / rewind the timer bar
            KeyCode::Char('f') => self.app.progress = 1.0,
            KeyCode::Char('0') => self.app.progress = 0.0,

            // quit
            KeyCode::Char('q') => return true,

            // pause (the panel shows a PAUSED indicator from `app.paused`)
            KeyCode::Char('p') => self.app.paused = !self.app.paused,

            // move cursor down / up in the active table
            KeyCode::Down | KeyCode::Char('j') => match self.screen {
                Screen::Help => self.help_table.next(),
                Screen::Stats => self.weight_table.next(),
                Screen::Tasks => self.app.next(),
            },
            KeyCode::Up | KeyCode::Char('k') => match self.screen {
                Screen::Help => self.help_table.previous(),
                Screen::Stats => self.weight_table.previous(),
                Screen::Tasks => self.app.previous(),
            },

            // toggle help / stats screens
            KeyCode::Char('h') => {
                self.screen = if self.screen == Screen::Help {
                    Screen::Tasks
                } else {
                    Screen::Help
                };
            }
            KeyCode::Char('s') => {
                self.screen = if self.screen == Screen::Stats {
                    Screen::Tasks
                } else {
                    Screen::Stats
                };
            }

            _ => {}
        }
        false
    }

    // ---- the LLM manager ----

    // Snapshot the task with `id` and run the manager off-thread with `prompt`;
    // the result returns via `manager_tx` as an `AppEvent::Manager`.
    fn spawn_manager(&mut self, id: Uuid, prompt: String) {
        if let Some(task) = self.store.task(id).cloned() {
            self.last_manager_prompt = Some(prompt.clone());
            let backend = Arc::clone(&self.manager);
            let tx = self.manager_tx.clone();
            let tags = self.cfg.tags.clone();
            tokio::spawn(async move {
                let msg = ManagerMsg::Manage(backend.manage(&task, &prompt, &tags).await);
                let _ = tx.send(msg);
            });
            self.manager_busy = true;
        }
    }

    // Run NL capture off-thread: turn `prompt` into proposed new tasks,
    // constrained to the configured tags. Result returns via `manager_tx`.
    fn spawn_capture(&mut self, prompt: String) {
        self.last_manager_prompt = Some(prompt.clone());
        let backend = Arc::clone(&self.manager);
        let tx = self.manager_tx.clone();
        let tags = self.cfg.tags.clone();
        tokio::spawn(async move {
            let msg = ManagerMsg::Capture(backend.capture(&prompt, &tags).await);
            let _ = tx.send(msg);
        });
        self.manager_busy = true;
    }

    // An off-thread job finished: route manage / capture / chat results.
    pub fn handle_manager(&mut self, msg: ManagerMsg) {
        self.manager_busy = false;
        match msg {
            ManagerMsg::Manage(result) => self.handle_manage_result(result),
            ManagerMsg::Capture(result) => self.handle_capture_result(result),
            ManagerMsg::Chat(result) => self.handle_chat_result(result),
        }
    }

    // A manage run finished: changes are reviewed before persisting; a
    // change-free run just shows its reply (and offers to continue chatting).
    fn handle_manage_result(&mut self, result: ManagerResult) {
        match result {
            Ok(outcome) if !outcome.changes.is_empty() => {
                let summary = if outcome.reply.is_empty() {
                    format!("Updated: {}", outcome.changes.join("; "))
                } else {
                    outcome.reply.clone()
                };
                self.finish_run(summary, Some(InputMode::ConfirmManager { outcome }));
            }
            Ok(outcome) => {
                let text = if outcome.reply.is_empty() {
                    "(no reply)".to_string()
                } else {
                    outcome.reply
                };
                self.finish_run(text, None);
            }
            Err(e) => self.finish_error(e),
        }
    }

    // A capture run finished: proposed tasks are reviewed before adding; an
    // empty capture just shows the reply (and offers to continue chatting).
    fn handle_capture_result(&mut self, result: CaptureResult) {
        match result {
            Ok(outcome) if !outcome.tasks.is_empty() => {
                let summary = if outcome.reply.is_empty() {
                    let names: Vec<&str> = outcome.tasks.iter().map(|t| t.name.as_str()).collect();
                    format!("Captured: {}", names.join(", "))
                } else {
                    outcome.reply.clone()
                };
                self.finish_run(summary, Some(InputMode::ConfirmCapture { outcome }));
            }
            Ok(outcome) => {
                let text = if outcome.reply.is_empty() {
                    "Couldn't make out any tasks there!".to_string()
                } else {
                    outcome.reply
                };
                self.finish_run(text, None);
            }
            Err(e) => self.finish_error(e),
        }
    }

    // Shared tail of a finished manage/capture run: pair the in-flight prompt
    // with the reply so Enter can continue the exchange in the fullscreen
    // chat, then show the review popup (when there is one) or the plain reply.
    fn finish_run(&mut self, text: String, review: Option<InputMode>) {
        let prompt = self.last_manager_prompt.take().unwrap_or_default();
        self.last_manager_turn = Some((prompt, text.clone()));
        self.input_mode = review.unwrap_or(InputMode::ManagerReply { text });
    }

    // A failed run: show the error; there is no exchange to continue.
    fn finish_error(&mut self, e: manager::ManagerError) {
        self.last_manager_prompt = None;
        self.last_manager_turn = None;
        self.input_mode = InputMode::ManagerReply {
            text: format!("manager error: {e}"),
        };
    }

    // Enter on a reply/review popup: continue the stashed exchange in the
    // fullscreen planning chat. No-op when there is nothing to continue.
    fn continue_last_chat(&mut self) {
        if let Some((user, reply)) = self.last_manager_turn.take() {
            self.open_plan_from(user, reply);
        }
    }

    // Persist a reviewed manager outcome: write the mutated task back by id
    // and refresh the affected views.
    fn apply_manager_outcome(&mut self, outcome: manager::ManagerOutcome) {
        let id = outcome.task.id;
        if let Some(slot) = self.store.task_mut(id) {
            *slot = outcome.task;
        }
        self.persist();
        self.rebuild_task_list();
        if self.current_id == Some(id) {
            self.refresh_current_subtasks();
        }
        self.refresh_expanded_subs();
    }

    // Persist reviewed captured tasks: add each to the store and refresh.
    fn apply_capture_outcome(&mut self, outcome: manager::CaptureOutcome) {
        for task in outcome.tasks {
            self.store.add(task);
        }
        self.persist();
        self.rebuild_task_list();
    }

    // ---- startup planning chat ----

    // Open the planning conversation: seed it with the leftover task list (and
    // optional check-in context about a task just finished/worked on) and kick
    // off SpongeBob's greeting.
    fn open_plan_after(&mut self, checkin: Option<(String, bool)>) {
        self.plan = Some(PlanState::default());
        self.input_mode = InputMode::Planning;
        let seed = self.leftover_context(checkin.as_ref().map(|(n, f)| (n.as_str(), *f)));
        self.send_plan(seed, false);
    }

    // Open the fullscreen planning chat continuing from a finished manager
    // exchange (a `ManagerReply`'s prompt/reply), so the transcript and the
    // message history sent to the model both carry that context forward.
    fn open_plan_from(&mut self, user: String, reply: String) {
        let mut plan = PlanState {
            messages: ChatHistory::from_exchange(&user, &reply),
            ..PlanState::default()
        };
        plan.transcript.push(Turn {
            who: Speaker::You,
            text: user,
        });
        plan.transcript.push(Turn {
            who: Speaker::Sponge,
            text: reply,
        });
        self.plan = Some(plan);
        self.last_manager_turn = None;
        self.input_mode = InputMode::Planning;
    }

    // A human-readable list of not-done tasks, used to seed the planning chat.
    fn leftover_context(&self, checkin: Option<(&str, bool)>) -> String {
        let mut s = String::new();
        if let Some((name, finished)) = checkin {
            if finished {
                s.push_str(&format!("I just finished '{name}'! "));
            } else {
                s.push_str(&format!(
                    "I just worked a session on '{name}' (not necessarily \
                     done). "
                ));
            }
        }
        s.push_str("My leftover (not-done) tasks:\n");
        let mut any = false;
        for t in &self.store.tasks {
            if t.status == Status::Done {
                continue;
            }
            any = true;
            let prog = if t.status == Status::InProgress {
                " [in progress]"
            } else {
                ""
            };
            let due = t
                .due_str()
                .map(|d| format!(", due {d}"))
                .unwrap_or_default();
            s.push_str(&format!("- {} (#{}){}{}\n", t.name, t.tag, prog, due));
        }
        if !any {
            s.push_str("(none — a clean slate!)\n");
        }
        let instruction = match checkin {
            Some((_, true)) => {
                "\nAs SpongeBob, congratulate me on finishing it, briefly note \
                 what's left, and ask what I want to tackle next."
            }
            Some((_, false)) => {
                "\nAs SpongeBob, ask how that work session went, briefly note \
                 what's left, and ask what I want to tackle next."
            }
            None => {
                "\nGreet me as SpongeBob, briefly note what's carried over, and \
                 ask what I want to get done today."
            }
        };
        s.push_str(instruction);
        s
    }

    // Send a turn to the planning chat off-thread. `show_user` controls whether
    // the text is echoed into the transcript (false for the hidden seed).
    fn send_plan(&mut self, text: String, show_user: bool) {
        if let Some(plan) = &mut self.plan {
            if show_user {
                plan.transcript.push(Turn {
                    who: Speaker::You,
                    text: text.clone(),
                });
            }
            plan.busy = true;
            let backend = Arc::clone(&self.manager);
            let tx = self.manager_tx.clone();
            let tags = self.cfg.tags.clone();
            let messages = plan.messages.clone();
            tokio::spawn(async move {
                let msg = ManagerMsg::Chat(backend.chat(messages, &text, &tags).await);
                let _ = tx.send(msg);
            });
        }
    }

    // A planning-chat turn finished: show the reply and add any captured tasks
    // live (they persist immediately; the transcript is the record).
    fn handle_chat_result(&mut self, result: ChatResult) {
        let mut added = false;
        if let Some(plan) = &mut self.plan {
            plan.busy = false;
            match result {
                Ok(outcome) => {
                    plan.messages = outcome.messages;
                    if !outcome.reply.is_empty() {
                        plan.transcript.push(Turn {
                            who: Speaker::Sponge,
                            text: outcome.reply,
                        });
                    }
                    for task in outcome.captured {
                        plan.added.push(task.name.clone());
                        self.store.add(task);
                        added = true;
                    }
                }
                Err(e) => plan.transcript.push(Turn {
                    who: Speaker::Sponge,
                    text: format!("Oh barnacles! Somethin' went wrong: {e}"),
                }),
            }
        }
        if added {
            self.persist();
            self.rebuild_task_list();
        }
    }

    // ---- pomodoro tick ----

    // Advance the pomodoro state machine on each 250ms tick.
    pub fn handle_tick(&mut self) {
        // Auto-dismiss the check-in offer once its deadline passes, regardless
        // of pause state.
        if let InputMode::CheckInOffer { deadline, .. } = self.input_mode {
            if Instant::now() >= deadline {
                self.input_mode = InputMode::Normal;
            }
        }
        // The timer is paused while planning (offer or active conversation).
        if self.app.paused || matches!(self.input_mode, InputMode::PlanOffer | InputMode::Planning)
        {
            return;
        }
        match self.phase {
            // working: when the timer fills, count the session and start a break
            Mode::Work => {
                if self.app.update(self.phase_minutes()) {
                    self.play_sound();
                    notify(&self.fx, "Time's up — take a break!");
                    if let Some(id) = self.current_id {
                        self.record_completion(id);
                    }
                    self.app.current_subtasks.clear();
                    if self.min_break_ctr == self.cfg.maxno_min_breaks {
                        self.phase = Mode::LongBreak;
                        self.app.current = CurrentPanel::LongBreak;
                    } else {
                        self.phase = Mode::ShortBreak;
                        self.app.current = CurrentPanel::ShortBreak;
                    }
                    // Offer a task-aware check-in at the work-period boundary
                    // (the task isn't assumed done — just worked on).
                    if let Some(name) = self
                        .current_id
                        .and_then(|id| self.store.task(id).map(|t| t.name.clone()))
                    {
                        self.input_mode = self.check_in_offer_mode(name, false);
                    }
                }
            }

            // break: when it fills, advance the cycle counter and reroll
            Mode::ShortBreak | Mode::LongBreak => {
                if self.app.update(self.phase_minutes()) {
                    self.play_sound();
                    if matches!(self.phase, Mode::ShortBreak) {
                        self.min_break_ctr += 1;
                    } else {
                        self.min_break_ctr = 0;
                    }
                    self.phase = Mode::Work;
                    self.reroll(true);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_new_task_name_due_recur_any_order() {
        let t = parse_new_task("water plants, 3d, 2050-01-01 10:00", "home").unwrap();
        assert_eq!(t.tag, "home");
        assert_eq!(t.name, "water plants");
        assert_eq!(t.recur.as_deref(), Some("3d"));
        assert_eq!(t.due_str().as_deref(), Some("2050-01-01 10:00"));

        // name alone is fine; trailing empty fields are ignored
        let t = parse_new_task("just a name, ,", "job").unwrap();
        assert_eq!(t.name, "just a name");
        assert!(t.due.is_none());
        assert!(t.recur.is_none());
    }

    #[test]
    fn parse_new_task_rejects_empty_name_and_junk_fields() {
        assert!(parse_new_task("", "job").is_err());
        assert!(parse_new_task("  , 3d", "job").is_err());
        let err = parse_new_task("name, tomorrow", "job").unwrap_err();
        assert!(err.contains("tomorrow"));
    }
}
