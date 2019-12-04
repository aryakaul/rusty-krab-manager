mod assignment_utils;
use assignment_utils::{readin_tasks,turn_assignmentvector_into_pdf, hashmap_to_taskvector, taskvector_to_stringvect};

mod rand_utils;
use rand_utils::{roll_die};
mod ui;
use ui::{draw_gauge, draw_task_table, draw_current_task, Event};
mod fileops_utils;
use std::io;
use std::error::Error;
use tui::backend::TermionBackend;
use tui::layout::{Constraint, Direction, Layout};
use termion::event::Key;
//use termion::event::Key;
use termion::input::MouseTerminal;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use tui::Terminal;
use std::collections::HashMap;

use config::Config;
use chrono::{Local, Weekday, Datelike};

fn main() -> Result<(), Box<dyn Error>> {
    // Terminal initialization
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    let events = ui::Events::new();
    let mut app = ui::App::new();

    // READ IN CONFIGRUATION
    // st
    let mut settings = config::Config::new();
    settings
        .merge(config::File::with_name("/home/luak/projects/git/rusty-manager/example/config"))?;
    let task_path = settings.get_str("task_filepath")?;
    let tags = settings.get_array("tags")?;
    let tags: Vec<String> = tags.into_iter().map(|i| i.into_str().unwrap()).collect();
    let use_due_dates = settings.get_array("use_due_dates")?;
    let use_due_dates: Vec<bool> = use_due_dates.into_iter().map(|i| i.into_bool().unwrap()).collect();
    let weights_mon = settings.get_array("weights.mon")?;
    let weights_mon: Vec<f64> = weights_mon.into_iter().map(|i| i.into_float().unwrap()).collect();
    let weights_tue = settings.get_array("weights.tue")?;
    let weights_tue: Vec<f64> = weights_tue.into_iter().map(|i| i.into_float().unwrap()).collect();
    let weights_wed = settings.get_array("weights.wed")?;
    let weights_wed: Vec<f64> = weights_wed.into_iter().map(|i| i.into_float().unwrap()).collect();
    let weights_thu = settings.get_array("weights.thu")?;
    let weights_thu: Vec<f64> = weights_thu.into_iter().map(|i| i.into_float().unwrap()).collect();
    let weights_fri = settings.get_array("weights.fri")?;
    let weights_fri: Vec<f64> = weights_fri.into_iter().map(|i| i.into_float().unwrap()).collect();
    let weights_sat = settings.get_array("weights.sat")?;
    let weights_sat: Vec<f64> = weights_sat.into_iter().map(|i| i.into_float().unwrap()).collect();
    let weights_sun = settings.get_array("weights.sun")?;
    let weights_sun: Vec<f64> = weights_sun.into_iter().map(|i| i.into_float().unwrap()).collect();
    /*
    println!("{:?}", task_path);
    println!("{:?}", tags);
    println!("{:?}", use_due_dates);
    println!("{:?}", weights_mon);
    println!("{:?}", weights_tue);
    println!("{:?}", weights_wed);
    println!("{:?}", weights_thu);
    println!("{:?}", weights_fri);
    println!("{:?}", weights_sat);
    println!("{:?}", weights_sun);
    */
    let curr_day: u32 = Local::now().weekday().number_from_monday();
    let mut tag_weights = weights_mon;
    if curr_day == 2 { tag_weights = weights_tue; }
    else if curr_day == 3 { tag_weights = weights_wed; }
    else if curr_day == 4 { tag_weights = weights_thu; }
    else if curr_day == 5 { tag_weights = weights_fri; }
    else if curr_day == 6 { tag_weights = weights_sat; }
    else { tag_weights = weights_sun; }
    //println!("{}", curr_day);
    // en

    // read in tasks
    let tag_to_vector_map = readin_tasks(&task_path, &tags);
   
    // update tag weights when no tasks in task_file
    // st
    let mut xi: f64 = 0.0;
    let mut ctr = 0;
    for (tag, assign_vec) in &tag_to_vector_map {
        if assign_vec.len() == 0 {
            let tag_loc = tags.iter().position(|z| &z == &tag).unwrap();
            xi += tag_weights[tag_loc];
            tag_weights[tag_loc] = 0.0;
        } else {
            ctr += 1 
        }
    }
    let to_add = xi / ctr as f64;
    for i in 0..tags.len() {
        if tag_weights[i] != 0.0 {
            tag_weights[i] += to_add;
        }
    }
    // en


    // roll a assignment 
    // st{
    let tag_roll = roll_die(tag_weights);
    let chosen_tag = &tags[tag_roll];
    let assignvector = tag_to_vector_map.get(chosen_tag).unwrap();
    let assignvector_pdf = turn_assignmentvector_into_pdf(&assignvector, use_due_dates[tag_roll]);
    let chosen_assign = &assignvector[roll_die(assignvector_pdf)];
    //println!("{:?}", assign_string);
    // }en
    
    // initialize app
    let assign_string = taskvector_to_stringvect(chosen_assign); 
    app.current_task = assign_string;
    let string_alltask_vec = hashmap_to_taskvector(tag_to_vector_map);
    app.items = string_alltask_vec;
    
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
                let check = app.update(1);
                if check {
                    //let tag_to_vector_map = readin_tasks(task_path);
                    //app.items = string_task_vec;
                    //let task_list = turn_assignmentvector_into_pdf(vect, true);
                }
            }
        };
    } 
    Ok(())
}
