//! The ratatui view layer: the `App` view-model and the stateless `draw_*`
//! functions that render each screen and popup. Widgets read from `App`
//! (populated by `Session`) and never mutate app logic. `Mode`/`Screen` are the
//! small enums the renderer switches on.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, Gauge, List, ListItem, Paragraph, Row, Table,
    TableState, Wrap,
};
use ratatui::Frame;
use uuid::Uuid;

// Which pomodoro phase the timer is currently in. Drives the gauge title/color.
#[derive(Clone, Copy)]
pub enum Mode {
    Work,
    ShortBreak,
    LongBreak,
}

// Which full-screen view is showing: the task dashboard, the help table, or the
// selection-probability stats. Toggled by the `h`/`s` keys.
#[derive(Clone, Copy, PartialEq)]
pub enum Screen {
    Tasks,
    Help,
    Stats,
}

// What the timer gauge needs to render time-remaining and pomodoro progress.
pub struct TimerView {
    pub mode: Mode,
    pub phase_minutes: i64,
    // 1-based work-session index within the current cycle, and the count of
    // sessions before a long break (for the "🍅 2 of 3" indicator).
    pub pom_index: i64,
    pub pom_total: i64,
}

// One ALL TASKS row: the task's id plus its display columns.
pub struct TaskRow {
    pub id: Uuid,
    pub tag: String,
    pub name: String,
    pub due: String,
    pub progress: String,
}

// What the CURRENT TASK panel is showing: the rolled task, a break, or the
// all-done sentinel. Owning this as data (rather than pre-rendered strings)
// keeps all copy and layout in `draw_current_task`.
#[derive(Default)]
pub enum CurrentPanel {
    Task {
        tag: String,
        name: String,
        due: String,
    },
    ShortBreak,
    LongBreak,
    #[default]
    AllDone,
}

// A scrollable string table with a wrapping selection: the stats (weights)
// and help screens.
pub struct SelectableTable {
    state: TableState,
    items: Vec<Vec<String>>,
}

impl SelectableTable {
    pub fn new(items: Vec<Vec<String>>) -> Self {
        Self {
            state: TableState::default(),
            items,
        }
    }

    pub fn next(&mut self) {
        self.state
            .select(wrap_next(self.state.selected(), self.items.len()));
    }

    pub fn previous(&mut self) {
        self.state
            .select(wrap_prev(self.state.selected(), self.items.len()));
    }
}

// The help screen's rows. Manager (LLM) bindings are listed only when the
// manager is available, matching the keys that actually work.
pub fn help_items(manager_available: bool) -> Vec<Vec<String>> {
    let rows: &[(&str, &str, bool)] = &[
        ("k", "scroll up in ALL TASKS table", false),
        ("j", "scroll down in ALL TASKS table", false),
        (
            "a",
            "add a new task (pick a tag, then name/due/recur)",
            false,
        ),
        ("A", "add a subtask to the highlighted task", false),
        (
            "Enter",
            "expand/collapse the selected task's subtasks",
            false,
        ),
        ("j/k", "when expanded, move into the subtask list", false),
        ("Space", "toggle the highlighted subtask done/undone", false),
        ("d", "delete the highlighted task (asks to confirm)", false),
        ("m", "message the manager about the highlighted task", true),
        (
            "D",
            "decompose the highlighted task into subtasks (LLM)",
            true,
        ),
        ("N", "capture new task(s) from plain text (LLM)", true),
        (
            "r",
            "reroll the given task without marking as complete",
            false,
        ),
        ("c", "complete the given task and select a new one", false),
        ("f", "fast forward current task bar to completion", false),
        ("0", "rewind current task bar to 0", false),
        ("p", "toggle pause", false),
        ("s", "access stats menu", false),
        ("q", "quit rusty-krab-manager", false),
        ("h", "toggle help menu", false),
    ];
    rows.iter()
        .filter(|(_, _, needs_manager)| manager_available || !needs_manager)
        .map(|(key, desc, _)| vec![key.to_string(), desc.to_string()])
        .collect()
}

// Shared chrome for the stats/help tables: bold header, rounded titled block,
// yellow-bold selection.
fn draw_string_table(
    f: &mut Frame,
    table: &mut SelectableTable,
    area: Rect,
    title: &str,
    header: &[&'static str],
    widths: &[Constraint],
) {
    let rows = table.items.iter().map(|i| {
        let cells = i.iter().map(|c| Cell::from(c.as_str()));
        Row::new(cells).style(Style::default().fg(Color::White))
    });
    let widget = Table::new(rows, widths.iter().copied())
        .header(
            Row::new(header.to_vec())
                .style(Style::default().add_modifier(Modifier::BOLD))
                .bottom_margin(1),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title.to_string())
                .border_type(BorderType::Rounded),
        )
        .row_highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    f.render_stateful_widget(widget, area, &mut table.state);
}

pub fn draw_weights(f: &mut Frame, table: &mut SelectableTable, area: Rect) {
    draw_string_table(
        f,
        table,
        area,
        "TAG WEIGHT TABLE",
        &["Tag", "Task", "TagProb", "DueProb", "TotalProb"],
        &[
            Constraint::Percentage(20),
            Constraint::Percentage(40),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(20),
        ],
    );
}

pub fn draw_help(f: &mut Frame, table: &mut SelectableTable, area: Rect) {
    draw_string_table(
        f,
        table,
        area,
        "HELP TABLE",
        &["Keypress", "Description"],
        &[Constraint::Percentage(20), Constraint::Percentage(80)],
    );
}

// What the ALL TASKS highlight currently sits on. The table is a flat list of
// task rows plus, when a task is expanded, that task's subtask rows; the cursor
// can land on either kind.
pub enum Cursor {
    Task { index: usize, id: Uuid },
    Sub { parent_id: Uuid, sub_index: usize },
}

// Define the current TUI application
// and its variables
#[derive(Default)]
pub struct App {
    state: TableState,
    pub items: Vec<TaskRow>,
    pub progress: f64,
    pub current: CurrentPanel,
    // Subtasks of the current task: (name, done).
    pub current_subtasks: Vec<(String, bool)>,
    // The expanded task (by id) and its subtasks, rendered inline beneath it.
    // None means no task is expanded. Kept in sync with the store by `Session`.
    pub expanded: Option<Uuid>,
    pub expanded_subs: Vec<(String, bool)>,
    pub paused: bool,
    // Completed-today counts per configured tag.
    pub completed: Vec<(String, i64)>,
}

impl App {
    // Row index of the expanded task within `items`, if it is still present.
    pub fn expanded_pos(&self) -> Option<usize> {
        self.expanded
            .and_then(|id| self.items.iter().position(|r| r.id == id))
    }

    // Number of rendered rows: every task, plus the expanded task's subtasks.
    fn flat_len(&self) -> usize {
        self.items.len() + self.expanded_pos().map_or(0, |_| self.expanded_subs.len())
    }

    // Resolve the highlighted flat row to the task or subtask it represents.
    pub fn cursor(&self) -> Option<Cursor> {
        let i = self.state.selected()?;
        match self.expanded_pos() {
            None => {
                let id = self.items.get(i)?.id;
                Some(Cursor::Task { index: i, id })
            }
            Some(p) => {
                let k = self.expanded_subs.len();
                if i <= p {
                    let id = self.items.get(i)?.id;
                    Some(Cursor::Task { index: i, id })
                } else if i <= p + k {
                    let parent_id = self.items.get(p)?.id;
                    Some(Cursor::Sub {
                        parent_id,
                        sub_index: i - p - 1,
                    })
                } else {
                    let index = i - k;
                    let id = self.items.get(index)?.id;
                    Some(Cursor::Task { index, id })
                }
            }
        }
    }

    // The highlighted task (id + name), but only when the cursor is on a task
    // row (not one of its subtasks). Used by task-level actions like delete.
    pub fn selected_task(&self) -> Option<(Uuid, String)> {
        match self.cursor()? {
            Cursor::Task { index, id } => {
                let name = self.items.get(index)?.name.clone();
                Some((id, name))
            }
            Cursor::Sub { .. } => None,
        }
    }

    // Drop any inline subtask expansion.
    pub fn clear_expansion(&mut self) {
        self.expanded = None;
        self.expanded_subs.clear();
    }

    // Collapse any expansion and move the highlight onto the given task row.
    pub fn collapse_onto(&mut self, task_id: Uuid) {
        self.clear_expansion();
        if let Some(p) = self.items.iter().position(|r| r.id == task_id) {
            self.state.select(Some(p));
        }
    }

    // Keep the highlight in bounds after the list changes, dropping a stale
    // expansion whose task is gone.
    pub fn clamp_selection(&mut self) {
        if self
            .expanded
            .is_some_and(|id| !self.items.iter().any(|r| r.id == id))
        {
            self.clear_expansion();
        }
        let n = self.flat_len();
        if n == 0 {
            self.state.select(None);
        } else {
            let i = self.state.selected().unwrap_or(0).min(n - 1);
            self.state.select(Some(i));
        }
    }

    // Function to update the app.
    // This runs every 250 milliseconds and returns
    // true when the app hits 100%
    pub fn update(&mut self, minutes: i64) -> bool {
        self.progress += (250.0 / 60000.0) / minutes as f64;
        if self.progress > 1.0 {
            self.progress = 0.0;
            return true;
        }
        false
    }

    pub fn next(&mut self) {
        self.state
            .select(wrap_next(self.state.selected(), self.flat_len()));
    }

    pub fn previous(&mut self) {
        self.state
            .select(wrap_prev(self.state.selected(), self.flat_len()));
    }
}

// Draw the timer gauge: a mode-aware title (work / break + pomodoro progress),
// a distinct color per phase, and the time remaining as MM:SS on the bar.
pub fn draw_gauge(f: &mut Frame, app: &App, area: Rect, view: &TimerView) {
    let remaining =
        (((1.0 - app.progress) * view.phase_minutes as f64 * 60.0).round() as i64).max(0);
    let label = format!("{:02}:{:02}", remaining / 60, remaining % 60);

    let (mut title, color) = match view.mode {
        Mode::Work => (
            format!("WORK · 🍅 {} of {}", view.pom_index, view.pom_total),
            Color::Yellow,
        ),
        Mode::ShortBreak => ("SHORT BREAK ☕".to_string(), Color::Cyan),
        Mode::LongBreak => ("LONG BREAK 🌴".to_string(), Color::Green),
    };
    let color = if app.paused {
        title = format!("PAUSED — {title}");
        Color::DarkGray
    } else {
        color
    };

    let gauge = Gauge::default()
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .gauge_style(Style::default().fg(color))
        .ratio(app.progress)
        .label(Span::styled(
            label,
            Style::default().add_modifier(Modifier::BOLD),
        ));
    f.render_widget(gauge, area);
}

// Draw the persistent one-line keybinding hint bar at the bottom of the
// screen — or, while `notice` is set, a transient message in its place
// (save errors, parse warnings, dropped actions).
pub fn draw_footer(f: &mut Frame, area: Rect, manager_available: bool, notice: Option<&str>) {
    if let Some(msg) = notice {
        let p = Paragraph::new(Line::from(Span::styled(
            format!(" {msg}"),
            Style::default().fg(Color::Yellow),
        )));
        f.render_widget(p, area);
        return;
    }
    let key = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let word = Style::default().fg(Color::DarkGray);

    let mut hints: Vec<(&str, &str)> = Vec::new();
    if manager_available {
        hints.push(("m", "anage"));
        hints.push(("D", "ecompose"));
        hints.push(("N", "ew✨"));
    }
    hints.extend([
        ("c", "omplete"),
        ("r", "eroll"),
        ("a", "dd"),
        ("A", "+sub"),
        ("⏎", "subs"),
        ("spc", "✓sub"),
        ("d", "elete"),
        ("p", "ause"),
        ("s", "tats"),
        ("h", "elp"),
        ("q", "uit"),
    ]);

    let mut spans = vec![Span::raw(" ")];
    for (k, w) in hints {
        spans.push(Span::styled("[", word));
        spans.push(Span::styled(k, key));
        spans.push(Span::styled("]", word));
        spans.push(Span::styled(w, word));
        spans.push(Span::raw("  "));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

// Draw the task table to showcase what tasks
// the rusty-krab-manager has read from the given
// task list
// The table is a flat list: every task row, and beneath the expanded task its
// indented, ordered subtask rows. An expanded task is flagged with a leading
// "v"; the highlight (cursor) can sit on a task or a subtask.
pub fn draw_task_table(f: &mut Frame, app: &mut App, area: Rect) {
    let selected_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let normal_style = Style::default().fg(Color::White);
    let child_style = Style::default().fg(Color::DarkGray);
    let widths = [
        Constraint::Percentage(18),
        Constraint::Percentage(48),
        Constraint::Percentage(24),
        Constraint::Percentage(10),
    ];

    let expanded_pos = app.expanded_pos();
    let selected = app.state.selected();
    let mut rows: Vec<Row> = Vec::new();
    let mut flat = 0usize;
    for (idx, item) in app.items.iter().enumerate() {
        let expanded_here = expanded_pos == Some(idx);
        // Leading indicator: an expanded task shows "v" (and never also
        // ">"); an unexpanded highlighted row shows ">".
        let prefix = if expanded_here {
            "v "
        } else if selected == Some(flat) {
            "> "
        } else {
            "  "
        };
        let cells = vec![
            Cell::from(format!("{prefix}{}", item.tag)),
            Cell::from(item.name.as_str()),
            Cell::from(item.due.as_str()),
            Cell::from(item.progress.as_str()),
        ];
        rows.push(Row::new(cells).style(normal_style));
        flat += 1;

        if expanded_here {
            let subs = subtask_rows(&app.expanded_subs, child_style, selected, flat);
            flat += subs.len();
            rows.extend(subs);
        }
    }

    let task_table = Table::new(rows, widths)
        .header(
            Row::new(vec!["Tag", "Name", "Due Date", "Subtasks"])
                .style(Style::default().add_modifier(Modifier::BOLD))
                .bottom_margin(1),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("ALL TASKS")
                .border_type(BorderType::Rounded),
        )
        .row_highlight_style(selected_style)
        .column_spacing(1);

    f.render_stateful_widget(task_table, area, &mut app.state);
}

// Indented child rows for an expanded task's ordered subtask list (numbered,
// with the first pending one flagged "next"). Empty tasks show a placeholder.
fn subtask_rows(
    subtasks: &[(String, bool)],
    style: Style,
    selected: Option<usize>,
    start: usize,
) -> Vec<Row<'static>> {
    let prefix = |row: usize| if selected == Some(row) { "> " } else { "  " };
    if subtasks.is_empty() {
        return vec![Row::new(vec![
            Cell::from(prefix(start).to_string()),
            Cell::from("  (no subtasks — press A to add)"),
        ])
        .style(style)];
    }
    let next = subtasks.iter().position(|(_, done)| !done);
    subtasks
        .iter()
        .enumerate()
        .map(|(i, (name, done))| {
            let mark = if *done { "☑" } else { "☐" };
            let nxt = if next == Some(i) { "  ← next" } else { "" };
            let text = format!("  {}. {} {}{}", i + 1, mark, name, nxt);
            Row::new(vec![
                Cell::from(prefix(start + i).to_string()),
                Cell::from(text),
            ])
            .style(style)
        })
        .collect()
}

// Draw the current task (or break / all-done) panel. A work task lays out a
// clear hierarchy (name, tag, due); breaks and the all-done sentinel render
// their own message. All copy lives here, keyed off `CurrentPanel`.
pub fn draw_current_task(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    if app.paused {
        lines.push(Line::from(Span::styled(
            "⏸ PAUSED",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
    }

    match &app.current {
        CurrentPanel::Task { tag, name, due } => {
            lines.push(Line::from(Span::styled(
                "NOW WORKING ON",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                name.as_str(),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            if !tag.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("#{tag}"),
                    Style::default().fg(Color::Cyan),
                )));
            }
            if !due.is_empty() {
                lines.push(Line::from(Span::styled(
                    due.as_str(),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
        CurrentPanel::ShortBreak => {
            lines.push(Line::from("TAKE A SHORT BREAK ☕"));
        }
        CurrentPanel::LongBreak => {
            lines.push(Line::from("TAKE A LONG BREAK 🌴"));
        }
        CurrentPanel::AllDone => {
            lines.push(Line::from("All tasks complete! 🎉"));
        }
    }

    // Append the subtask checklist, if any. Subtasks are ordered: they are
    // numbered, and the first not-yet-done one is marked as "next" to make the
    // intended order obvious.
    if !app.current_subtasks.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Subtasks",
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )));
        let next = app.current_subtasks.iter().position(|(_, done)| !done);
        for (i, (name, done)) in app.current_subtasks.iter().enumerate() {
            let is_next = next == Some(i);
            let (mark, style) = if *done {
                ("☑ ", Style::default().fg(Color::Green))
            } else if is_next {
                (
                    "☐ ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ("☐ ", Style::default())
            };
            let mut spans = vec![
                Span::styled(format!("{}. ", i + 1), Style::default().fg(Color::DarkGray)),
                Span::styled(mark, style),
                Span::styled(name.as_str(), style),
            ];
            if is_next {
                spans.push(Span::styled("  ← next", Style::default().fg(Color::Yellow)));
            }
            lines.push(Line::from(spans));
        }
    }

    let task_paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title("CURRENT TASK")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(task_paragraph, area);
}

// The mascot's yellow.
const SPONGE_YELLOW: Color = Color::Rgb(255, 221, 51);

// Word-wrap `text` to `width` columns (never splitting a word). Always returns
// at least one line.
fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if cur.is_empty() {
            cur = word.to_string();
        } else if cur.chars().count() + 1 + word.chars().count() <= width {
            cur.push(' ');
            cur.push_str(word);
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

// A rounded speech bubble around `text`. All lines are padded to one width so
// the box stays rectangular.
fn speech_bubble(text: &str) -> Vec<Line<'static>> {
    let wrapped = wrap_words(text, 40);
    let inner = wrapped
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0)
        .max(8);
    let style = Style::default().fg(Color::White);
    let bar = "─".repeat(inner + 2);
    let mut lines = vec![Line::from(Span::styled(format!("╭{bar}╮"), style))];
    for l in &wrapped {
        let pad = " ".repeat(inner - l.chars().count());
        let row = format!("│ {l}{pad} │");
        lines.push(Line::from(Span::styled(row, style)));
    }
    lines.push(Line::from(Span::styled(format!("╰{bar}╯"), style)));
    lines
}

// The SpongeBob mascot, rendered in braille dot art, drawn beneath its
// speech bubble's tail. Every row is padded to one width so centering keeps
// the picture coherent.
fn sponge_lines() -> Vec<Line<'static>> {
    let s = Style::default()
        .fg(SPONGE_YELLOW)
        .add_modifier(Modifier::BOLD);
    [
        r"⠀⠀⡴⢫⠉⠊⣩⡉⠀⠉⠙⠒⠒⠙⠒⠒⠒⠒⠒⢢⠀⠀⠀⠀⠀⠀",
        r"⢠⢊⡴⡼⡀⠘⠔⠊⡲⠜⠒⠯⡀⡸⠦⠫⢴⠊⠃⢸⠀⠀⠀⠀⠀⠀",
        r"⡋⠘⠿⠃⢸⠀⠉⡞⠀⠀⣴⣦⣹⢠⣶⣄⠀⢳⠀⢸⠀⠀⠀⠀⠠⢚",
        r"⢣⠀⢠⣄⢸⠀⠀⡳⠤⢄⡛⢛⡴⡘⠛⢋⡠⢾⡀⢸⠀⠀⢠⢒⣧⢧",
        r"⠈⢣⠹⢫⢼⡀⠀⠈⢲⣒⠋⠁⠀⠩⠭⢥⠔⠊⠀⣸⠀⠀⠈⣮⡱⠟",
        r"⠀⠸⡆⠸⡾⢸⠀⠀⢸⣿⣿⣏⣹⣿⣉⡏⢁⠔⡤⡇⠀⠀⡸⡸⠀⠀",
        r"⠀⠀⠸⣄⣰⢬⠀⡀⠸⣛⡻⡻⢿⣿⠀⠀⠈⣋⢠⢛⣶⣔⡽⠀⠀⠀",
        r"⠀⠀⣼⠀⢣⣈⡂⢮⠃⠈⣉⡙⠉⠁⣀⠀⠈⣉⣸⠒⠋⠀⠀⠀⠀⠀",
        r"⠀⠀⠘⣷⣯⣄⣿⣁⣉⣛⣤⣜⣿⣟⣤⣛⣉⣈⡇⠀⠀⠀⠀⠀⠀⠀",
        r"⠀⠀⠀⡯⣿⣿⣿⣿⣿⣻⣻⣻⣿⣻⣟⣿⣿⣿⡃⠀⠀⠀⠀⠀⠀⠀",
        r"⠀⠀⢰⣇⣼⡜⢿⣿⡿⠋⠉⠉⠙⣿⠟⠋⠈⠀⠀⠀⠀⠀⠀⠀⠀⠀",
        r"⠀⠀⠈⠛⠋⠀⢸⣸⠀⠀⠀⠀⠀⣿⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀",
        r"⠀⠀⠀⠀⠀⠀⣸⣸⠀⠀⠀⠀⢠⣿⣀⣀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀",
        r"⠀⠀⠀⠀⠀⠈⢿⣿⣿⣷⠀⠀⠸⢿⣿⣿⠇⠀⠀⠀⠀⠀⠀⠀⠀⠀",
    ]
    .into_iter()
    .map(|l| Line::from(Span::styled(l, s)))
    .collect()
}

// Draw the SpongeBob mascot speaking `speech` in a bubble, with `extra`
// (change list / action hints) plain beneath it. Used for all manager output.
pub fn draw_sponge_popup(f: &mut Frame, speech: &str, extra: &str) {
    let mut lines = speech_bubble(speech);
    lines.push(Line::from(Span::styled(
        "      ╲",
        Style::default().fg(Color::White),
    )));
    lines.extend(sponge_lines());
    if !extra.is_empty() {
        lines.push(Line::from(""));
        for l in extra.lines() {
            lines.push(Line::from(l.to_string()));
        }
    }

    let content_h = lines.len() as u16;
    let para = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: false });
    render_popup(f, (70, 60), "SpongeBob 🧽", para, content_h);
}

// Who said a given line in the planning transcript.
pub enum Speaker {
    You,
    Sponge,
}

// One line of the planning transcript.
pub struct Turn {
    pub who: Speaker,
    pub text: String,
}

// Draw the full-screen planning conversation: scrolling transcript, an input
// line, and a hint/status footer.
pub fn draw_plan(
    f: &mut Frame,
    area: Rect,
    transcript: &[Turn],
    input: &str,
    added: &[String],
    busy: bool,
) {
    f.render_widget(Clear, area);
    let block = Block::default()
        .title("Plan your day with SpongeBob 🧽")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SPONGE_YELLOW));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Reserve a side column for a persistent SpongeBob mascot, anchored to
    // its bottom-right corner, when the terminal is wide enough to fit the
    // braille art (26 cells) without crowding the chat.
    let side_width: u16 = 28;
    let show_sponge = inner.width >= side_width + 20;
    let chat_area = if show_sponge {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(10), Constraint::Length(side_width)])
            .split(inner);
        let sponge = sponge_lines();
        let sponge_h = (sponge.len() as u16).min(cols[1].height);
        let sponge_rect = Rect {
            x: cols[1].x,
            y: cols[1].y + cols[1].height.saturating_sub(sponge_h),
            width: cols[1].width,
            height: sponge_h,
        };
        f.render_widget(
            Paragraph::new(sponge).alignment(Alignment::Center),
            sponge_rect,
        );
        cols[0]
    } else {
        inner
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(chat_area);

    // Pre-wrap the transcript so we can scroll precisely to the bottom.
    let width = (rows[0].width as usize).max(10);
    let mut lines: Vec<Line> = Vec::new();
    for turn in transcript {
        let (label, style) = match turn.who {
            Speaker::You => (
                "You",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Speaker::Sponge => (
                "SpongeBob",
                Style::default()
                    .fg(SPONGE_YELLOW)
                    .add_modifier(Modifier::BOLD),
            ),
        };
        let wrapped = wrap_words(&turn.text, width.saturating_sub(7).max(8));
        for (i, w) in wrapped.iter().enumerate() {
            if i == 0 {
                lines.push(Line::from(vec![
                    Span::styled(format!("{label}: "), style),
                    Span::raw(w.clone()),
                ]));
            } else {
                lines.push(Line::from(format!("       {w}")));
            }
        }
        lines.push(Line::from(""));
    }
    if busy {
        lines.push(Line::from(Span::styled(
            "🧽 cooking up a reply…",
            Style::default().fg(SPONGE_YELLOW),
        )));
    }
    let visible = rows[0].height as usize;
    let offset = lines.len().saturating_sub(visible) as u16;
    f.render_widget(Paragraph::new(lines).scroll((offset, 0)), rows[0]);

    let input_line = if busy {
        Line::from(Span::styled("…", Style::default().fg(Color::DarkGray)))
    } else {
        Line::from(format!("> {input}▌"))
    };
    f.render_widget(Paragraph::new(input_line), rows[1]);

    let mut hint = String::from("Enter = send   ·   Esc = done");
    if !added.is_empty() {
        hint.push_str(&format!("   ·   added today: {}", added.len()));
    }
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            hint,
            Style::default().fg(Color::DarkGray),
        ))),
        rows[2],
    );
}

// A text-input popup: optional intro text, the live buffer with a cursor, an
// inline error, and the Enter/Esc hint line. Shared by every input mode.
pub fn draw_input_popup(
    f: &mut Frame,
    title: &str,
    intro: &str,
    buffer: &str,
    error: Option<&str>,
    submit: &str,
) {
    let mut body = String::new();
    if !intro.is_empty() {
        body.push_str(intro);
        body.push_str("\n\n");
    }
    body.push_str(buffer);
    body.push_str("▌\n");
    if let Some(e) = error {
        body.push_str(&format!("\n⚠ {e}\n"));
    }
    body.push_str(&format!("\nEnter = {submit}   Esc = cancel"));
    draw_popup(f, title, &body);
}

// Draw a centered modal popup (used for add-task input and delete confirm).
pub fn draw_popup(f: &mut Frame, title: &str, body: &str) {
    let content_h = body.lines().count() as u16;
    let para = Paragraph::new(body.to_string()).wrap(Wrap { trim: false });
    render_popup(f, (60, 30), title, para, content_h);
}

// Shared popup chrome: clear a `(pct_x, pct_y)` slice of the screen, draw the
// rounded yellow-bordered `title` block, and render `para` (of `content_h`
// lines) vertically centered inside it.
fn render_popup(f: &mut Frame, pct: (u16, u16), title: &str, para: Paragraph, content_h: u16) {
    let area = centered_rect(pct.0, pct.1, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(title.to_string())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(para, vcenter(inner, content_h));
}

// A sub-rectangle of `inner`, full width but pushed down so `content_h` lines
// sit vertically centered. Keeps the remaining height below the offset so any
// wrapped overflow still has room.
fn vcenter(inner: Rect, content_h: u16) -> Rect {
    let top = inner.height.saturating_sub(content_h) / 2;
    Rect {
        x: inner.x,
        y: inner.y + top,
        width: inner.width,
        height: inner.height - top,
    }
}

// Advance / retreat a wrapping table selection by one row; `None` when empty.
// Shared by the ALL TASKS, stats, and help tables.
fn wrap_next(selected: Option<usize>, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match selected {
        Some(i) if i >= len - 1 => 0,
        Some(i) => i + 1,
        None => 0,
    })
}

fn wrap_prev(selected: Option<usize>, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match selected {
        Some(0) | None => len - 1,
        Some(i) => i - 1,
    })
}

// Compute a rectangle centered within `r`, sized as a percentage of it.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

// Draw the counter to keep track of the number
// of tags done
pub fn draw_tag_counter(f: &mut Frame, app: &App, area: Rect) {
    let stuff: Vec<ListItem> = app
        .completed
        .iter()
        .map(|(tag, n)| ListItem::new(Line::from(format!("{tag}: {n}"))))
        .collect();

    let task_ctr = List::new(stuff).block(
        Block::default()
            .borders(Borders::ALL)
            .title("DONE TODAY")
            .border_type(BorderType::Rounded),
    );
    f.render_widget(task_ctr, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Build an App with three tasks; expand the middle one with two subtasks.
    fn app_with_expanded_middle() -> App {
        let ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
        App {
            items: (0..3)
                .map(|i| TaskRow {
                    id: ids[i],
                    tag: format!("tag{i}"),
                    name: format!("t{i}"),
                    due: String::new(),
                    progress: String::new(),
                })
                .collect(),
            expanded: Some(ids[1]),
            expanded_subs: vec![("a".into(), false), ("b".into(), true)],
            ..App::default()
        }
    }

    #[test]
    fn cursor_maps_flat_rows_to_tasks_and_subtasks() {
        let mut app = app_with_expanded_middle();
        // flat layout: 0=t0, 1=t1(expanded), 2=sub a, 3=sub b, 4=t2
        let parent = app.items[1].id;

        app.state.select(Some(0));
        assert!(matches!(app.cursor(), Some(Cursor::Task { index: 0, .. })));

        app.state.select(Some(1));
        assert!(matches!(app.cursor(), Some(Cursor::Task { index: 1, .. })));

        app.state.select(Some(2));
        assert!(matches!(
            app.cursor(),
            Some(Cursor::Sub { parent_id, sub_index: 0 }) if parent_id == parent
        ));

        app.state.select(Some(3));
        assert!(matches!(
            app.cursor(),
            Some(Cursor::Sub { sub_index: 1, .. })
        ));

        // the row after the subtasks is the third task
        app.state.select(Some(4));
        assert!(matches!(app.cursor(), Some(Cursor::Task { index: 2, .. })));
    }

    #[test]
    fn selected_task_is_none_on_a_subtask_row() {
        let mut app = app_with_expanded_middle();
        app.state.select(Some(2)); // a subtask row
        assert!(app.selected_task().is_none());
        app.state.select(Some(1)); // the expanded task row
        assert!(app.selected_task().is_some());
    }

    #[test]
    fn clamp_drops_expansion_when_task_is_gone() {
        let mut app = app_with_expanded_middle();
        // remove the expanded task from the list
        app.items.remove(1);
        app.clamp_selection();
        assert!(app.expanded.is_none());
        assert!(app.expanded_subs.is_empty());
    }

    #[test]
    fn wrap_words_respects_width_and_never_empty() {
        let w = wrap_words("the krusty krab is the finest eatery", 12);
        assert!(w.iter().all(|l| l.chars().count() <= 12));
        assert_eq!(wrap_words("", 10), vec![String::new()]);
    }

    #[test]
    fn speech_bubble_is_rectangular() {
        let lines = speech_bubble("arr, break it down matey");
        let width = |l: &Line| {
            l.spans
                .iter()
                .map(|s| s.content.chars().count())
                .sum::<usize>()
        };
        let first = width(&lines[0]);
        assert!(lines.iter().all(|l| width(l) == first));
        assert!(lines.len() >= 3); // top border, >=1 text row, bottom border
    }
}
