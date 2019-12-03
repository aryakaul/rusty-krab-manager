mod assignment_utils;
use assignment_utils::{readin_tasks,turn_assignmentvector_into_pdf, hashmap_to_taskvector};

mod rand_utils;
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

fn main() -> Result<(), Box<dyn Error>> {
    // Terminal initialization
    /*
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    let events = ui::Events::new();
    let mut app = ui::App::new();
    */
    let mut settings = config::Config::new();
    settings
        .merge(config::File::with_name("/home/luak/projects/git/rusty-manager/example/config"))?;
    //println!("HI");
    let tags = settings.get_array("tags")?;
    let use_due_dates = settings.get_array("use_due_dates")?;
    let weights_mon = settings.get_array("weights.mon")?;
    let weights_tue = settings.get_array("weights.tue")?;
    let weights_wed = settings.get_array("weights.wed")?;
    let weights_thu = settings.get_array("weights.thu")?;
    let weights_fri = settings.get_array("weights.fri")?;
    let weights_sat = settings.get_array("weights.sat")?;
    let weights_sun = settings.get_array("weights.sun")?;
    println!("{:?}", tags);
    println!("{:?}", use_due_dates);
    println!("{:?}", weights_mon);
    println!("{:?}", weights_tue);
    println!("{:?}", weights_wed);
    println!("{:?}", weights_thu);
    println!("{:?}", weights_fri);
    println!("{:?}", weights_sat);
    println!("{:?}", weights_sun);
    //println!("{:?}",
    //         settings.try_into::<HashMap<String, String>>().unwrap());
    /*
    let task_path = "/home/luak/projects/git/rusty-manager/example/tasks";

    // read in initial hashmap
    let tag_to_vector_map = readin_tasks(&task_path);
    //let task_list = turn_assignmentvector_into_pdf(vect, true);
    let string_task_vec = hashmap_to_taskvector(tag_to_vector_map);
    app.items = string_task_vec;
    
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
                    let tag_to_vector_map = readin_tasks(task_path);
                    let string_task_vec = hashmap_to_taskvector(tag_to_vector_map);
                    app.items = string_task_vec;
                    //let task_list = turn_assignmentvector_into_pdf(vect, true);
                }
            }
        };
    } */
    Ok(())
}
