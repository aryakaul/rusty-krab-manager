#[allow(dead_code)]
mod event;

use std::io;

use crate::event::{Event, Events};
use std::error::Error;
use termion::event::Key;
use termion::input::MouseTerminal;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use tui::backend::TermionBackend;
use tui::layout::{Alignment, Constraint, Corner, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{
    Block, Borders, Gauge, List, Paragraph, Row, SelectableList, Table, Text, Widget,
};
use tui::Terminal;


pub fn get_percentage_width(width: u16, percentage: f32) -> u16 {
    let padding = 3;
    let width = width - padding;
    (f32::from(width) * percentage) as u16
}

struct App<'a> {
    items: Vec<Vec<&'a str>>,
    selected: usize,
    progress: f64,
}

impl<'a> App<'a> {
    fn new() -> App<'a> {
        App {
            items: vec![
                vec!["ABCDEFGHIJKLMNOPQRSTUVWXYZABCDABCD", "item12"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
                vec!["item21", "item22"],
            ],
            selected: 0,
            progress: 0.0,
        }
    }
    fn update(&mut self) {
        self.progress += (250.0 / 60000.0);
        if self.progress > 100.0 {
            self.progress = 100.0;
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // Terminal initialization
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let events = Events::new();
    let mut app = App::new();

    let text = [Text::raw("This is a line \n")];
    let text2 = [
        Text::raw("This is a second line \n"),
        Text::raw("This is a third line \n"),
        Text::raw("This is a Fourth line \n"),
    ];

    loop {
        terminal.draw(|mut f| {
            let selected_style = Style::default().fg(Color::Yellow).modifier(Modifier::BOLD);
            let normal_style = Style::default().fg(Color::White);
            let header = ["HEADER1", "HEADER2"];
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
            
            let padding = 5;
            let offset = chunks[1]
                .height
                .checked_sub(padding)
                .and_then(|height| app.selected.checked_sub(height as usize))
                .unwrap_or(0);

            let rows = app.items.iter().skip(offset).enumerate().map(|(i, item)| {
                if Some(i) == app.selected.checked_sub(offset) {
                    Row::StyledData(item.into_iter(), selected_style)
                } else {
                    Row::StyledData(item.into_iter(), normal_style)
                }
            });


            Paragraph::new(text.iter())
                .block(Block::default().title("CURRENT TASK").borders(Borders::ALL))
                .alignment(Alignment::Left)
                .wrap(true)
                .render(&mut f, mini_chunks[0]);

            Paragraph::new(text2.iter())
                .block(
                    Block::default()
                        .title("TASKS COMPLETED")
                        .borders(Borders::ALL),
                )
                .alignment(Alignment::Right)
                .wrap(true)
                .render(&mut f, mini_chunks[1]);

            Table::new(header.into_iter(), rows)
                .block(Block::default().borders(Borders::ALL).title("ALL TASKS"))
                //.widths(&[20, 10])
                .widths(&
                    [get_percentage_width(chunks[1].width, 0.8),
                    get_percentage_width(chunks[1].width, 0.2)])
                .column_spacing(1)
                .render(&mut f, chunks[1]);

            Gauge::default()
                .block(Block::default().title("TIME LEFT").borders(Borders::ALL))
                .style(Style::default().fg(Color::Yellow))
                .ratio(app.progress)
                .render(&mut f, chunks[2]);
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
                let check = app.update();
            }
        };
    }
    Ok(())
}
