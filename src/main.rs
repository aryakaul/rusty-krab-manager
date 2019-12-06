mod assignment_utils;
use assignment_utils::{
    hashmap_to_taskvector, readin_tasks, taskvector_to_stringvect, turn_assignmentvector_into_pdf,
};
mod rand_utils;
use rand_utils::roll_die;
mod ui;
use ui::event::{Event, Events};
use ui::{draw_current_task, draw_gauge, draw_task_table, App};
mod fileops_utils;
mod settings_util;
use std::error::Error;
use std::io;
use termion::event::Key;
use termion::input::MouseTerminal;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use tui::backend::TermionBackend;
use tui::layout::{Constraint, Direction, Layout};
use tui::Terminal;
//use chrono::{Datelike, Local};

fn choose_task(
    configured_task_path: &str,
    vector_of_tags: &Vec<String>,
    configured_relative_tag_weights: &mut Vec<f64>,
    configured_use_of_due_dates: &Vec<bool>,
) -> (Vec<String>, Vec<Vec<String>>) {
    // read in tasks
    let tag_to_vector_map = readin_tasks(configured_task_path, &vector_of_tags);

    // update tag weights when no tasks in task_file
    let mut xi: f64 = 0.0;
    let mut ctr = 0;
    for (tag, assign_vec) in &tag_to_vector_map {
        if assign_vec.len() == 0 {
            let tag_loc = vector_of_tags.iter().position(|z| &z == &tag).unwrap();
            xi += configured_relative_tag_weights[tag_loc];
            configured_relative_tag_weights[tag_loc] = 0.0;
        } else {
            ctr += 1
        }
    }
    let to_add = xi / ctr as f64;
    for i in 0..vector_of_tags.len() {
        if configured_relative_tag_weights[i] != 0.0 {
            configured_relative_tag_weights[i] += to_add;
        }
    }

    // roll a assignment
    let tag_roll = roll_die(configured_relative_tag_weights.to_vec());
    let chosen_tag = &vector_of_tags[tag_roll];
    let assignvector = tag_to_vector_map.get(chosen_tag).unwrap();
    let assignvector_pdf =
        turn_assignmentvector_into_pdf(&assignvector, configured_use_of_due_dates[tag_roll]);
    let chosen_assign = &assignvector[roll_die(assignvector_pdf)];

    // generate table string and current task string
    let assign_string = taskvector_to_stringvect(chosen_assign);
    let string_alltask_vec = hashmap_to_taskvector(tag_to_vector_map, &vector_of_tags);
    return (assign_string, string_alltask_vec);
}

fn main() -> Result<(), Box<dyn Error>> {
    // Read in configuration
    let (
        task_path,
        tags,
        use_due_dates,
        mut tag_weights,
        min_break_time,
        max_break_time,
        task_time,
        maxno_min_breaks,
    ) = settings_util::readin_settings("/home/luak/projects/git/rusty-manager/example/config")?;

    // Choose initial task
    let (curr_task, items_to_list) =
        choose_task(&task_path, &tags, &mut tag_weights, &use_due_dates);

    // Terminal initialization
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    let events = Events::new();
    let mut app = App::new();
    app.current_task = curr_task;
    app.items = items_to_list;

    // Initialize starting parameters
    let mut min_break_ctr = 0;
    let mut its_task_time = true;
    let mut its_min_break_time = false;
    let mut its_max_break_time = false;

    // Enter into UI drawing infinite loop
    loop {
        terminal.draw(|mut f| {
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
                .split(f.size());

            let mini_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(75), Constraint::Percentage(25)].as_ref())
                .split(chunks[0]);
            draw_gauge(&mut f, &app, chunks[2]);
            draw_task_table(&mut f, &app, chunks[1]);
            draw_current_task(&mut f, &app, mini_chunks[0]);
        })?;

        match events.next()? {
            Event::Input(input) => match input {
                Key::Char('q') => {
                    break;
                }
                Key::Down | Key::Char('j') => {
                    app.selected += 1;
                    if app.selected > app.items.len() - 1 {
                        app.selected = 0
                    }
                }
                Key::Up | Key::Char('k') => {
                    if app.selected > 0 {
                        app.selected -= 1;
                    } else {
                        app.selected = app.items.len() - 1;
                    }
                }
                _ => {}
            },
            Event::Tick => {
                //app.update(task_time, its_min_break_time);
                if its_task_time {
                    if min_break_ctr == maxno_min_breaks {
                        its_max_break_time = app.update(task_time);
                    } else {
                        its_min_break_time = app.update(task_time);
                    }
                    if its_min_break_time || its_max_break_time {
                        its_task_time = false;
                    };
                } else if its_min_break_time {
                    app.current_task = vec![String::from("TAKE A CHILL PILL")];
                    its_task_time = app.update(min_break_time);
                    //app.update(min_break_time, its_task_time);
                    if its_task_time {
                        min_break_ctr += 1;
                        let (curr_task, items_to_list) =
                            choose_task(&task_path, &tags, &mut tag_weights, &use_due_dates);
                        app.current_task = curr_task;
                        app.items = items_to_list;
                        its_min_break_time = false;
                    }
                } else if its_max_break_time {
                    app.current_task = vec![String::from("TAKE A LONG CHILL PILL")];
                    its_task_time = app.update(max_break_time);
                    if its_task_time {
                        min_break_ctr = 0;
                        let (curr_task, items_to_list) =
                            choose_task(&task_path, &tags, &mut tag_weights, &use_due_dates);
                        app.current_task = curr_task;
                        app.items = items_to_list;
                        its_max_break_time = false;
                    }
                }
            }
        };
    }
    Ok(())
}
