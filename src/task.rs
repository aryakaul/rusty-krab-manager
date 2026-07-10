//! The `Task` data model and its serde representation. A task has a tag, name,
//! optional due date, status, an ordered list of `Subtask`s, and a timestamped
//! `history` log. This is what the store persists and what the LLM manager
//! and the UI both read/mutate. `parse_due` / `DUE_FORMAT` define the friendly
//! `"YYYY-MM-DD HH:MM"` date format used everywhere.

use chrono::{DateTime, Duration, Local, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Human-friendly due-date format used across the app and the store.
pub const DUE_FORMAT: &str = "%Y-%m-%d %H:%M";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Todo,
    InProgress,
    Done,
}

// A unit of work the LLM manager will create/check off in later phases.
// Addressed by name (LLM) or position (TUI), so it carries no id.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Subtask {
    pub name: String,
    #[serde(default)]
    pub done: bool,
}

// A timestamped progress note. Phase 2's LLM writes here when it checks in.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub at: DateTime<Local>,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    // Drives weighted-random selection; must match a configured tag to be used.
    pub tag: String,
    pub name: String,
    // Optional now: an undated task never goes overdue. Stored as the friendly
    // DUE_FORMAT string (see `due_fmt`, used by the archive and
    // legacy-store serde — the live tasks.md store has its own
    // `@due`/`@every` rendering in mdstore.rs).
    #[serde(default, skip_serializing_if = "Option::is_none", with = "due_fmt")]
    pub due: Option<DateTime<Local>>,
    #[serde(default)]
    pub status: Status,
    // Recurrence interval like "12h" / "3d" / "2w". A recurring task never
    // expires: completing it reschedules the due date instead of finishing
    // it, and an overdue recurring task just nags at top weight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recur: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subtasks: Vec<Subtask>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<HistoryEntry>,
}

// Serde adapter for `due`: stores/reads the friendly DUE_FORMAT string
// ("YYYY-MM-DD HH:MM"), matching what the user types in the add-task popup.
pub(crate) mod due_fmt {
    use super::{parse_due, DUE_FORMAT};
    use chrono::{DateTime, Local};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        due: &Option<DateTime<Local>>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        match due {
            Some(dt) => s.serialize_str(&dt.format(DUE_FORMAT).to_string()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<DateTime<Local>>, D::Error> {
        let raw = Option::<String>::deserialize(d)?;
        Ok(raw.and_then(|s| parse_due(&s)))
    }
}

impl Task {
    pub fn new(tag: String, name: String, due: Option<DateTime<Local>>) -> Self {
        Self {
            id: Uuid::new_v4(),
            tag,
            name,
            due,
            status: Status::Todo,
            recur: None,
            subtasks: Vec::new(),
            history: Vec::new(),
        }
    }

    // The parsed recurrence interval, when `recur` is set and valid.
    pub fn recurrence(&self) -> Option<Duration> {
        self.recur.as_deref().and_then(parse_recur)
    }

    pub fn minutes_until_due(&self) -> Option<i64> {
        self.due
            .map(|d| d.signed_duration_since(Local::now()).num_minutes())
    }

    pub fn is_overdue(&self) -> bool {
        self.minutes_until_due().is_some_and(|m| m < 0)
    }

    // Eligible for the random roll: not done and not past due. Recurring
    // tasks never expire — overdue just means "really do it now".
    pub fn is_selectable(&self) -> bool {
        self.status != Status::Done && (!self.is_overdue() || self.recurrence().is_some())
    }

    // A short, human-friendly due string: "no due date", "overdue", or a
    // coarse "in N min/hr/days/wk/mo/yr". Easier to parse at a glance than a
    // raw timestamp.
    pub fn due_relative(&self) -> String {
        match self.minutes_until_due() {
            None => "no due date".to_string(),
            Some(m) if m < 0 => "overdue".to_string(),
            Some(m) if m < 60 => format!("in {m} min"),
            Some(m) if m < 1440 => format!("in {} hr", m / 60),
            Some(m) => {
                let days = m / 1440;
                if days < 14 {
                    format!("in {days} days")
                } else if days < 60 {
                    format!("in {} wk", days / 7)
                } else if days < 365 {
                    format!("in {} mo", days / 30)
                } else {
                    format!("in {} yr", days / 365)
                }
            }
        }
    }

    // The absolute due date in the friendly DUE_FORMAT, when one is set.
    pub fn due_str(&self) -> Option<String> {
        self.due.map(|d| d.format(DUE_FORMAT).to_string())
    }

    // What the UI shows in due columns: `due_relative`, with a ↻ marker for
    // recurring tasks.
    pub fn due_display(&self) -> String {
        match self.recurrence() {
            Some(_) => format!("{} ↻", self.due_relative()),
            None => self.due_relative(),
        }
    }

    pub fn log(&mut self, note: impl Into<String>) {
        self.history.push(HistoryEntry {
            at: Local::now(),
            note: note.into(),
        });
    }

    // Subtask mutators used by the LLM manager. Subtasks are addressed by name
    // (case-insensitive) since that is what the model sees.
    pub fn add_subtask(&mut self, name: String) {
        self.subtasks.push(Subtask { name, done: false });
    }

    // The `(name, done)` pairs the current-task panel / expanded list render.
    pub fn subtask_view(&self) -> Vec<(String, bool)> {
        self.subtasks
            .iter()
            .map(|s| (s.name.clone(), s.done))
            .collect()
    }

    // Flip a subtask's done state by position. Used by the in-TUI Space toggle,
    // which addresses subtasks by their (ordered) row rather than by name.
    pub fn toggle_subtask(&mut self, index: usize) -> bool {
        match self.subtasks.get_mut(index) {
            Some(s) => {
                s.done = !s.done;
                true
            }
            None => false,
        }
    }

    pub fn set_subtask_done(&mut self, name: &str, done: bool) -> bool {
        match self
            .subtasks
            .iter_mut()
            .find(|s| s.name.eq_ignore_ascii_case(name.trim()))
        {
            Some(s) => {
                s.done = done;
                true
            }
            None => false,
        }
    }

    pub fn remove_subtask(&mut self, name: &str) -> bool {
        let before = self.subtasks.len();
        let n = name.trim();
        self.subtasks.retain(|s| !s.name.eq_ignore_ascii_case(n));
        self.subtasks.len() != before
    }

    pub fn set_status(&mut self, status: Status) {
        self.status = status;
    }
}

// Case-insensitive lookup of a tag in the configured set, returning the
// canonical (configured) casing. The one tag validator, shared by the manager
// tools and the add-task popup so their semantics can't drift.
pub fn find_tag<'a>(tags: &'a [String], requested: &str) -> Option<&'a String> {
    tags.iter().find(|t| t.eq_ignore_ascii_case(requested))
}

// Parse a "%Y-%m-%d %H:%M" local datetime; returns None if unparseable or
// ambiguous. Replaces the deprecated chrono `datetime_from_str`.
pub fn parse_due(s: &str) -> Option<DateTime<Local>> {
    NaiveDateTime::parse_from_str(s.trim(), DUE_FORMAT)
        .ok()
        .and_then(|naive| Local.from_local_datetime(&naive).single())
}

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

// Parse a recurrence interval: a positive number followed by h (hours),
// d (days), or w (weeks) — e.g. "12h", "3d", "2w". None if invalid.
pub fn parse_recur(s: &str) -> Option<Duration> {
    let s = s.trim();
    let unit = s.chars().last()?;
    let n: i64 = s[..s.len() - unit.len_utf8()].parse().ok()?;
    if n <= 0 {
        return None;
    }
    match unit {
        'h' => Some(Duration::hours(n)),
        'd' => Some(Duration::days(n)),
        'w' => Some(Duration::weeks(n)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_subtask_preserves_order() {
        let mut t = Task::new("school".into(), "thesis".into(), None);
        t.add_subtask("outline".into());
        t.add_subtask("draft".into());
        t.add_subtask("revise".into());
        let names: Vec<&str> = t.subtasks.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["outline", "draft", "revise"]);
    }

    #[test]
    fn due_roundtrips_friendly() {
        // Friendly DUE_FORMAT survives a TOML round-trip.
        let mut t = Task::new("job".into(), "ship".into(), parse_due("2050-01-01 10:00"));
        let toml_str = toml::to_string(&t).unwrap();
        assert!(toml_str.contains("due = \"2050-01-01 10:00\""));
        let back: Task = toml::from_str(&toml_str).unwrap();
        assert_eq!(back.due, t.due);

        // No due serializes to nothing (skipped), and round-trips as None.
        t.due = None;
        let toml_str = toml::to_string(&t).unwrap();
        assert!(!toml_str.contains("due"));
        let back: Task = toml::from_str(&toml_str).unwrap();
        assert!(back.due.is_none());
    }

    #[test]
    fn parse_recur_accepts_h_d_w_and_rejects_junk() {
        assert_eq!(parse_recur("12h"), Some(Duration::hours(12)));
        assert_eq!(parse_recur("3d"), Some(Duration::days(3)));
        assert_eq!(parse_recur(" 2w "), Some(Duration::weeks(2)));
        assert!(parse_recur("").is_none());
        assert!(parse_recur("d").is_none());
        assert!(parse_recur("3x").is_none());
        assert!(parse_recur("0d").is_none());
        assert!(parse_recur("-1d").is_none());
    }

    #[test]
    fn overdue_recurring_task_stays_selectable() {
        let overdue = Local::now() - Duration::hours(2);
        let mut t = Task::new("home".into(), "water plants".into(), Some(overdue));
        assert!(!t.is_selectable()); // non-recurring: expired
        t.recur = Some("3d".into());
        assert!(t.is_selectable()); // recurring: never expires
        assert_eq!(t.due_display(), "overdue ↻");
        t.status = Status::Done;
        assert!(!t.is_selectable()); // but Done still wins
    }

    #[test]
    fn toggle_subtask_flips_by_index_and_bounds_check() {
        let mut t = Task::new("school".into(), "thesis".into(), None);
        t.add_subtask("a".into());
        t.add_subtask("b".into());
        assert!(t.toggle_subtask(1));
        assert!(t.subtasks[1].done);
        assert!(t.toggle_subtask(1));
        assert!(!t.subtasks[1].done);
        assert!(!t.toggle_subtask(9)); // out of range is a no-op
    }

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
}
