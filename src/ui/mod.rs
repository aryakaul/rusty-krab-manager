mod event;

use std::io;
//use std::error::Error;
use termion::event::Key;
//use termion::input::MouseTerminal;
//use termion::raw::IntoRawMode;
//use termion::screen::AlternateScreen;
use tui::backend::{Backend};
use tui::layout::{Alignment, Rect};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{
    Block, Borders, Gauge, Paragraph, Row, Table, Widget, Text
};
use tui::{Frame};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use termion::input::TermRead;

pub fn get_percentage_width(width: u16, percentage: f32) -> u16 {
    let padding = 3;
    let width = width - padding;
    (f32::from(width) * percentage) as u16
}

pub enum Event<I> {
    Input(I),
    Tick,
}

/// A small event handler that wrap termion input and tick events. Each event
/// type is handled in its own thread and returned to a common `Receiver`
pub struct Events {
    rx: mpsc::Receiver<Event<Key>>,
    input_handle: thread::JoinHandle<()>,
    tick_handle: thread::JoinHandle<()>,
}

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub exit_key: Key,
    pub tick_rate: Duration,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            exit_key: Key::Char('q'),
            tick_rate: Duration::from_millis(250),
        }
    }
}

impl Events {
    pub fn new() -> Events {
        Events::with_config(Config::default())
    }

    pub fn with_config(config: Config) -> Events {
        let (tx, rx) = mpsc::channel();
        let input_handle = {
            let tx = tx.clone();
            thread::spawn(move || {
                let stdin = io::stdin();
                for evt in stdin.keys() {
                    match evt {
                        Ok(key) => {
                            if let Err(_) = tx.send(Event::Input(key)) {
                                return;
                            }
                            if key == config.exit_key {
                                return;
                            }
                        }
                        Err(_) => {}
                    }
                }
            })
        };
        let tick_handle = {
            let tx = tx.clone();
            thread::spawn(move || {
                let tx = tx.clone();
                loop {
                    tx.send(Event::Tick).unwrap();
                    thread::sleep(config.tick_rate);
                }
            })
        };
        Events {
            rx,
            input_handle,
            tick_handle,
        }
    }

    pub fn next(&self) -> Result<Event<Key>, mpsc::RecvError> {
        self.rx.recv()
    }
}

pub struct App<'a> {
    //pub items: Vec<Vec<&'a str>>,
    pub items: Vec<Vec<String>>,
    pub selected: usize,
    pub progress: f64,
    //pub current_task: Vec<&'a str>,
    pub current_task: Vec<&'a str>,
    //pub current_task: Vec<&str>,
}

impl<'a> App<'a> {
    pub fn new() -> App<'a> {
        App {
            items: vec![
                vec![String::from("GANG"), String::from("GANG"), String::from("GANG")]
            ],
            selected: 0,
            progress: 0.0,
            current_task: vec!["Hello\n","Heyyo!\n","MEMES\n"],
        }
    }
    pub fn update(&mut self, minutes: u64) -> bool {
        self.progress += (250.0 / 600000.0) / minutes as f64;
        if self.progress > 100.0 {
            self.progress = 100.0;
            return true
        }
        return false
    }
}

pub fn draw_gauge<B>(mut f: &mut Frame<B>, app: &App, area: Rect) 
where 
        B: Backend,
{
    Gauge::default()
        .block(Block::default().title("TIME LEFT").borders(Borders::ALL))
        .style(Style::default().fg(Color::Yellow))
        .ratio(app.progress)
        .render(&mut f, area);
}

pub fn draw_task_table<B>(mut f: &mut Frame<B>, app: &App, area: Rect)
where 
    B: Backend,
{
    let header = ["Tag", "Name", "Due Date"];
    let padding = 5;
    let offset = area 
        .height
        .checked_sub(padding)
        .and_then(|height| app.selected.checked_sub(height as usize))
        .unwrap_or(0);

    let selected_style = Style::default().fg(Color::Yellow).modifier(Modifier::BOLD);
    let normal_style = Style::default().fg(Color::White);
    let rows = app.items.iter().skip(offset).enumerate().map(|(i, item)| {
        if Some(i) == app.selected.checked_sub(offset) {
            Row::StyledData(item.into_iter(), selected_style)
        } else {
            Row::StyledData(item.into_iter(), normal_style)
        }
    });
    Table::new(header.into_iter(), rows)
        .block(Block::default().borders(Borders::ALL).title("ALL TASKS"))
        .widths(&
            [get_percentage_width(area.width, 0.2),
            get_percentage_width(area.width, 0.4),
            get_percentage_width(area.width, 0.4)])
        .column_spacing(1)
        .render(&mut f, area);
}

pub fn draw_current_task<B>(mut f: &mut Frame<B>, app: &App, area: Rect)
    where
        B: Backend,
{
    let mut new_shit = vec![];
    for i in 0..app.current_task.len() {
        new_shit.push(Text::raw(app.current_task[i]))
    }
    Paragraph::new(new_shit.iter())
        .block(Block::default().title("CURRENT TASK").borders(Borders::ALL))
        .alignment(Alignment::Left)
        .wrap(true)
        .render(&mut f, area)
}
