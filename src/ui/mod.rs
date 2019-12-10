pub mod event;
use tui::backend::Backend;
use tui::layout::{Alignment, Rect};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{Block, Borders, Gauge, Paragraph, Row, Table, Text, Widget, List};
use tui::Frame;

pub fn get_percentage_width(width: u16, percentage: f32) -> u16 {
    let padding = 3;
    let width = width - padding;
    (f32::from(width) * percentage) as u16
}

pub struct App {
    pub items: Vec<Vec<String>>,
    pub selected: usize,
    pub progress: f64,
    pub current_task: Vec<String>,
    pub paused: bool,
    pub completed: Vec<(String, String)>
}

impl App {
    pub fn new() -> App {
        App {
            items: vec![vec![
                String::from("GANG"),
                String::from("GANG"),
                String::from("GANG"),
            ]],
            selected: 0,
            progress: 0.0,
            current_task: vec![
                String::from("Hello\n"),
                String::from("Heyyo!\n"),
                String::from("MEMES\n"),
            ],
            paused: false,
            completed: vec![
                (String::from("GANG"), String::from("0")),
                (String::from("GANG"), String::from("1")),
                (String::from("GANG"), String::from("2")),
                (String::from("GANG"), String::from("3")),
                (String::from("GANG"), String::from("4")),
                (String::from("GANG"), String::from("5")),
            ],
        }
    }
    pub fn update(&mut self, minutes: i64) -> bool {
        self.progress += (250.0 / 60000.0) / minutes as f64;
        if self.progress > 1.0 {
            self.progress = 0.0;
            return true;
        }
        return false;
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
    let header = ["\nTag", "\nName", "\nDue Date"];
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
        .widths(&[
            get_percentage_width(area.width, 0.15),
            get_percentage_width(area.width, 0.55),
            get_percentage_width(area.width, 0.3),
        ])
        .column_spacing(1)
        .render(&mut f, area);
}

pub fn draw_current_task<B>(mut f: &mut Frame<B>, app: &App, area: Rect)
where
    B: Backend,
{
    let mut new_shit = vec![];
    let x = Text::styled(
        "DO THIS SHIT\n\n",
        Style::default().bg(Color::Green).modifier(Modifier::BOLD),
    );
    new_shit.push(x);
    for i in 0..app.current_task.len() {
        new_shit.push(Text::raw(&app.current_task[i]));
    }
    Paragraph::new(new_shit.iter())
        .block(Block::default().title("CURRENT TASK").borders(Borders::ALL))
        .alignment(Alignment::Center)
        .wrap(true)
        .render(&mut f, area);
}

pub fn draw_tag_counter<B>(mut f: &mut Frame<B>, app: &App, area:Rect)
    where 
        B: Backend,
{
    let stuff = app.completed.iter().map(|(tag, ctr)| {
        Text::styled(format!("{}: {}", tag, ctr),
        Style::default())
    });

    List::new(stuff)
        .block(Block::default().borders(Borders::ALL).title("COUNTER"))
        //.start_corner(Corner::BottomRight)
        .render(&mut f, area);
}
