mod assignment_utils;
use assignment_utils::{
    hashmap_to_taskvector, readin_tasks, taskvector_to_stringvect, turn_assignmentvector_into_pdf,
};
mod rand_utils;
use rand_utils::roll_die;
mod ui;
use ui::{draw_current_task, draw_gauge, draw_task_table, App};
use ui::event::{Event, Events};
mod fileops_utils;
use std::error::Error;
use std::io;
use termion::event::Key;
use tui::backend::TermionBackend;
use tui::layout::{Constraint, Direction, Layout};
use termion::input::MouseTerminal;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use tui::Terminal;
use chrono::{Datelike, Local};

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
    let mut settings = config::Config::new();
    settings.merge(config::File::with_name(
        "/home/luak/projects/git/rusty-manager/example/config",
    ))?;
    let task_path = settings.get_str("task_filepath")?;
    let tags = settings.get_array("tags")?;
    let tags: Vec<String> = tags.into_iter().map(|i| i.into_str().unwrap()).collect();
    let use_due_dates = settings.get_array("use_due_dates")?;
    let use_due_dates: Vec<bool> = use_due_dates
        .into_iter()
        .map(|i| i.into_bool().unwrap())
        .collect();
    let weights_mon = settings.get_array("weights.mon")?;
    let weights_mon: Vec<f64> = weights_mon
        .into_iter()
        .map(|i| i.into_float().unwrap())
        .collect();
    let weights_tue = settings.get_array("weights.tue")?;
    let weights_tue: Vec<f64> = weights_tue
        .into_iter()
        .map(|i| i.into_float().unwrap())
        .collect();
    let weights_wed = settings.get_array("weights.wed")?;
    let weights_wed: Vec<f64> = weights_wed
        .into_iter()
        .map(|i| i.into_float().unwrap())
        .collect();
    let weights_thu = settings.get_array("weights.thu")?;
    let weights_thu: Vec<f64> = weights_thu
        .into_iter()
        .map(|i| i.into_float().unwrap())
        .collect();
    let weights_fri = settings.get_array("weights.fri")?;
    let weights_fri: Vec<f64> = weights_fri
        .into_iter()
        .map(|i| i.into_float().unwrap())
        .collect();
    let weights_sat = settings.get_array("weights.sat")?;
    let weights_sat: Vec<f64> = weights_sat
        .into_iter()
        .map(|i| i.into_float().unwrap())
        .collect();
    let weights_sun = settings.get_array("weights.sun")?;
    let weights_sun: Vec<f64> = weights_sun
        .into_iter()
        .map(|i| i.into_float().unwrap())
        .collect();
    let min_break_time = settings.get_int("short_break_time")?;
    let max_break_time = settings.get_int("long_break_time")?;
    let task_time = settings.get_int("task_time")?;
    let maxno_min_breaks = settings.get_int("maxno_short_breaks")?;

    let curr_day: u32 = Local::now().weekday().number_from_monday();
    let mut tag_weights: Vec<f64>;
    if curr_day ==1 {
        tag_weights = weights_mon;
    } else if curr_day == 2 {
        tag_weights = weights_tue;
    } else if curr_day == 3 {
        tag_weights = weights_wed;
    } else if curr_day == 4 {
        tag_weights = weights_thu;
    } else if curr_day == 5 {
        tag_weights = weights_fri;
    } else if curr_day == 6 {
        tag_weights = weights_sat;
    } else {
        tag_weights = weights_sun;
    }
    
    let mut min_break_ctr = 0;
    let (curr_task, items_to_list) =
        choose_task(&task_path, &tags, &mut tag_weights, &use_due_dates);

    // Terminal initialization
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    //let events = ui::Events::new();
    let events = Events::new();
    let mut app = App::new();
    app.current_task = curr_task;
    app.items = items_to_list;

    let mut its_task_time = true;
    let mut its_min_break_time = false;
    let mut its_max_break_time = false;
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
