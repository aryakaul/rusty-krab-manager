//! Weighted-random task selection — the app's core "what should I work on next"
//! logic, preserved from the original. It picks a tag by its (day-specific,
//! redistributed) weight, then a task within that tag by an optional due-date
//! probability. The LLM manager never touches this: it only edits a task's
//! subtasks/status/fields, never which task is chosen.

use crate::rand_utils::roll_die;
use crate::store::Store;
use crate::task::Task;
use crate::ui::{CurrentPanel, TaskRow};
use std::collections::HashMap;
use uuid::Uuid;

type Grouped<'a> = HashMap<String, Vec<&'a Task>>;

// The result of rolling a task: what the current-task panel shows, the chosen
// task's subtasks, the ALL TASKS rows, the stats weight table, and the chosen
// task's id/name.
pub struct Selection {
    pub current: CurrentPanel,
    pub current_subtasks: Vec<(String, bool)>,
    pub items: Vec<TaskRow>,
    pub weighttable: Vec<Vec<String>>,
    pub chosen_id: Option<Uuid>,
    // The chosen task's name, for notifications (None for the all-done
    // sentinel).
    pub chosen_name: Option<String>,
}

// Roll a task from the store using the weighted-random model: pick a tag by its
// (redistributed) weight, then a task within that tag by the due-date / uniform
// pdf. Returns a sentinel selection when nothing is selectable.
pub fn choose_task(
    store: &Store,
    tags: &[String],
    initial_tag_weights: &[f64],
    use_due_dates: &[bool],
) -> Selection {
    let grouped = store.selectable_by_tag(tags);
    let relative_tag_weights = update_tagweights(&grouped, initial_tag_weights, tags);
    let weighttable = create_weighttable(&grouped, tags, &relative_tag_weights, use_due_dates);
    let items = grouped_to_rows(&grouped, tags);

    // No selectable tasks left: show a sentinel rather than crashing.
    if relative_tag_weights.iter().sum::<f64>() <= 0.0 {
        return Selection {
            current: CurrentPanel::AllDone,
            current_subtasks: Vec::new(),
            items,
            weighttable,
            chosen_id: None,
            chosen_name: None,
        };
    }

    // First pick a tag, then a task within it.
    let tag_roll = roll_die(&relative_tag_weights);
    let tasks_in_tag = &grouped[&tags[tag_roll]];
    let task_pdf = task_pdf(tasks_in_tag, use_due_dates[tag_roll]);
    let chosen = tasks_in_tag[roll_die(&task_pdf)];

    Selection {
        current: CurrentPanel::Task {
            tag: chosen.tag.clone(),
            name: chosen.name.clone(),
            due: chosen.due_display(),
        },
        current_subtasks: chosen.subtask_view(),
        items,
        weighttable,
        chosen_id: Some(chosen.id),
        chosen_name: Some(chosen.name.clone()),
    }
}

// Probability distribution over a tag's tasks. With due-date weighting, tasks
// due sooner get more weight; undated tasks are treated as far-future (lowest
// priority). Without it, the distribution is uniform.
pub fn task_pdf(tasks: &[&Task], use_due: bool) -> Vec<f64> {
    if tasks.is_empty() {
        return Vec::new();
    }
    if !use_due {
        let uniform = 1.0 / tasks.len() as f64;
        return vec![uniform; tasks.len()];
    }

    // Minutes-until-due for dated tasks, clamped to at least 1 — which also
    // hands an overdue-but-selectable recurring task the top weight. Undated
    // tasks compete as if due at the farthest dated horizon (i.e. like the
    // least-urgent dated task) rather than being starved to ~0%.
    let dated: Vec<Option<i64>> = tasks
        .iter()
        .map(|t| t.minutes_until_due().map(|m| m.max(1)))
        .collect();
    let biggest = dated.iter().flatten().copied().max().unwrap_or(1);
    let mins: Vec<i64> = dated.into_iter().map(|m| m.unwrap_or(biggest)).collect();

    let mut pdf: Vec<f64> = Vec::with_capacity(mins.len());
    let mut sum = 0.0;
    for m in mins {
        let value = biggest as f64 / m as f64;
        pdf.push(value);
        sum += value;
    }
    for prob in &mut pdf {
        *prob /= sum;
    }
    pdf
}

// Redistribute the weight of empty / zero-weight tags evenly across the tags
// that still have selectable tasks, so the live weights always sum to 1.
pub fn update_tagweights(
    grouped: &Grouped,
    initial_tag_weights: &[f64],
    tags: &[String],
) -> Vec<f64> {
    let mut updated = initial_tag_weights.to_owned();
    let mut freed = 0.0;
    let mut active = 0;

    for (i, tag) in tags.iter().enumerate() {
        let empty = grouped.get(tag).map_or(true, |v| v.is_empty());
        if empty || initial_tag_weights[i] == 0.0 {
            freed += initial_tag_weights[i];
            updated[i] = 0.0;
        } else {
            active += 1;
        }
    }

    if active > 0 {
        let bonus = freed / active as f64;
        for weight in updated.iter_mut() {
            if *weight != 0.0 {
                *weight += bonus;
            }
        }
    }
    updated
}

// Rows for the stats screen: tag, name, tag-prob, due-prob, total-prob,
// sorted by total probability descending.
pub fn create_weighttable(
    grouped: &Grouped,
    tags: &[String],
    tag_weights: &[f64],
    use_dues: &[bool],
) -> Vec<Vec<String>> {
    // Carry each row's numeric total alongside it so the sort keys off the
    // f64 directly instead of re-parsing the formatted percent string.
    let mut rows: Vec<(f64, Vec<String>)> = Vec::new();
    for (i, tag) in tags.iter().enumerate() {
        let tag_weight = tag_weights[i];
        let Some(tasks) = grouped.get(tag) else {
            continue;
        };
        let pdf = task_pdf(tasks, use_dues[i]);
        for (j, task) in tasks.iter().enumerate() {
            let total = pdf[j] * tag_weight;
            rows.push((
                total,
                vec![
                    task.tag.clone(),
                    task.name.clone(),
                    format!("{:.2}%", tag_weight * 100.0),
                    format!("{:.2}%", pdf[j] * 100.0),
                    format!("{:.2}%", total * 100.0),
                ],
            ));
        }
    }
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    rows.into_iter().map(|(_, row)| row).collect()
}

// All selectable tasks as ALL TASKS rows, in configured-tag order.
pub fn grouped_to_rows(grouped: &Grouped, tags: &[String]) -> Vec<TaskRow> {
    let mut rows = Vec::new();
    for tag in tags {
        let Some(tasks) = grouped.get(tag) else {
            continue;
        };
        for task in tasks {
            rows.push(TaskRow {
                id: task.id,
                tag: task.tag.clone(),
                name: task.name.clone(),
                due: task.due_display(),
                progress: subtask_progress(task),
            });
        }
    }
    rows
}

// "done/total" for a task's subtasks, or "–" when it has none.
fn subtask_progress(task: &Task) -> String {
    if task.subtasks.is_empty() {
        return "–".to_string();
    }
    let done = task.subtasks.iter().filter(|s| s.done).count();
    format!("{}/{}", done, task.subtasks.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Local};

    fn task(tag: &str, name: &str) -> Task {
        Task::new(tag.into(), name.into(), None)
    }

    fn due_task(tag: &str, name: &str, hours: i64) -> Task {
        Task::new(
            tag.into(),
            name.into(),
            Some(Local::now() + Duration::hours(hours)),
        )
    }

    fn grouped<'a>(entries: &[(&str, Vec<&'a Task>)]) -> Grouped<'a> {
        entries
            .iter()
            .map(|(tag, tasks)| (tag.to_string(), tasks.clone()))
            .collect()
    }

    fn tags(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn task_pdf_is_uniform_without_due_weighting() {
        let a = task("school", "a");
        let b = task("school", "b");
        assert_eq!(task_pdf(&[&a, &b], false), vec![0.5, 0.5]);
        assert!(task_pdf(&[], false).is_empty());
        assert!(task_pdf(&[], true).is_empty());
    }

    #[test]
    fn task_pdf_favors_sooner_due_and_undated_matches_horizon() {
        let soon = due_task("school", "soon", 1);
        let later = due_task("school", "later", 24 * 10);
        let undated = task("school", "undated");
        let pdf = task_pdf(&[&soon, &later, &undated], true);
        assert!(pdf[0] > pdf[1]);
        // Undated competes like the least-urgent dated task, not at ~0%.
        assert!((pdf[1] - pdf[2]).abs() < 1e-12);
        assert!(pdf[2] > 0.0);
        assert!((pdf.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn task_pdf_gives_overdue_recurring_top_weight() {
        let mut chore = due_task("school", "chore", -2);
        chore.recur = Some("1d".into());
        let later = due_task("school", "later", 24 * 10);
        let pdf = task_pdf(&[&chore, &later], true);
        assert!(pdf[0] > pdf[1]);
        assert!((pdf.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn update_tagweights_redistributes_empty_tags() {
        let a = task("school", "a");
        let c = task("personal", "c");
        let g = grouped(&[
            ("school", vec![&a]),
            ("job", vec![]),
            ("personal", vec![&c]),
        ]);
        let w = update_tagweights(&g, &[0.5, 0.3, 0.2], &tags(&["school", "job", "personal"]));
        // job's 0.3 is split evenly across the two live tags.
        assert!((w[0] - 0.65).abs() < 1e-9);
        assert_eq!(w[1], 0.0);
        assert!((w[2] - 0.35).abs() < 1e-9);
        assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn update_tagweights_keeps_zero_weight_tags_at_zero() {
        let a = task("school", "a");
        let b = task("job", "b");
        let g = grouped(&[("school", vec![&a]), ("job", vec![&b])]);
        let w = update_tagweights(&g, &[1.0, 0.0], &tags(&["school", "job"]));
        assert_eq!(w, vec![1.0, 0.0]);
    }

    #[test]
    fn update_tagweights_all_empty_is_all_zero() {
        let g = grouped(&[("school", vec![]), ("job", vec![])]);
        let w = update_tagweights(&g, &[0.6, 0.4], &tags(&["school", "job"]));
        assert!(w.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn choose_task_returns_sentinel_when_nothing_selectable() {
        let store = Store::default();
        let sel = choose_task(&store, &tags(&["school"]), &[1.0], &[false]);
        assert!(matches!(sel.current, CurrentPanel::AllDone));
        assert!(sel.chosen_id.is_none());
        assert!(sel.chosen_name.is_none());
        assert!(sel.items.is_empty());
    }

    #[test]
    fn choose_task_picks_the_only_selectable_task() {
        let t = task("school", "thesis");
        let id = t.id;
        let store = Store::seed(vec![t]);
        // job is empty, so its weight redistributes and school always wins.
        let sel = choose_task(
            &store,
            &tags(&["school", "job"]),
            &[0.5, 0.5],
            &[false, false],
        );
        assert_eq!(sel.chosen_id, Some(id));
        assert_eq!(sel.chosen_name.as_deref(), Some("thesis"));
        assert_eq!(sel.items.len(), 1);
    }

    #[test]
    fn grouped_to_rows_orders_by_tag_and_reports_progress() {
        let mut a = task("school", "a");
        a.add_subtask("s1".into());
        a.add_subtask("s2".into());
        a.toggle_subtask(0);
        let b = task("job", "b");
        let g = grouped(&[("school", vec![&a]), ("job", vec![&b])]);
        let rows = grouped_to_rows(&g, &tags(&["job", "school"]));
        // Configured-tag order, not map iteration order.
        assert_eq!(rows[0].name, "b");
        assert_eq!(rows[0].progress, "–");
        assert_eq!(rows[1].progress, "1/2");
    }

    #[test]
    fn weighttable_sorts_by_total_probability() {
        let a = task("school", "a");
        let b = task("job", "b");
        let g = grouped(&[("school", vec![&a]), ("job", vec![&b])]);
        let rows = create_weighttable(&g, &tags(&["school", "job"]), &[0.2, 0.8], &[false, false]);
        assert_eq!(rows[0][1], "b");
        assert_eq!(rows[0][4], "80.00%");
        assert_eq!(rows[1][4], "20.00%");
    }
}
