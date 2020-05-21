pub mod event;
use tui::backend::Backend;
use tui::layout::{Alignment, Constraint, Rect};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{Block, Borders, Gauge, List, Paragraph, Row, Table, Text};
use tui::Frame;

/*
 * Allow one to use percentages dynamically within
 * the TUI framework
 */
pub fn get_percentage_width(width: u16, percentage: f32) -> u16 {
    let padding = 3;
    let width = width - padding;
    (f32::from(width) * percentage) as u16
}

/*
 * Define te current TUI application
 * and its variables
 */
pub struct App {
    pub items: Vec<Vec<String>>,
    pub selected: usize,
    pub progress: f64,
    pub current_task: Vec<String>,
    pub paused: bool,
    pub completed: Vec<(String, String)>,
}

impl App {
    /*
     * Instantiate the default application.
     * If all goes according to plan none of these
     * values are actually used. Used for debugging.
     */
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

    /*
     * Function to update the app.
     * This runs every 250 milliseconds and returns
     * true when the app hits 100%
     */
    pub fn update(&mut self, minutes: i64) -> bool {
        self.progress += (250.0 / 60000.0) / minutes as f64;
        if self.progress > 1.0 {
            self.progress = 0.0;
            return true;
        }
        return false;
    }
}

/*
 * Draw the gauge used to showcase the remaining
 * amount of time left to do whatever.
 */
pub fn draw_gauge<B>(f: &mut Frame<B>, app: &App, area: Rect)
where
    B: Backend,
{
    let gauge = Gauge::default()
        .block(Block::default().title("TIME LEFT").borders(Borders::ALL))
        .style(Style::default().fg(Color::Yellow))
        .ratio(app.progress);
    f.render_widget(gauge, area);
}

/*
 * Draw the task table to showcase what tasks
 * the rusty-krab-manager has read from the given
 * task list
 */
pub fn draw_task_table<B>(f: &mut Frame<B>, app: &App, area: Rect)
where
    B: Backend,
{
    // set basic values
    let padding = 5;
    let offset = area
        .height
        .checked_sub(padding)
        .and_then(|height| app.selected.checked_sub(height as usize))
        .unwrap_or(0);

    let selected_style = Style::default().fg(Color::Yellow).modifier(Modifier::BOLD);
    let normal_style = Style::default().fg(Color::White);
    let header = ["\nTag", "\nName", "\nDue Date"];
    let widths = [
        Constraint::Length(get_percentage_width(area.width, 0.15)),
        Constraint::Length(get_percentage_width(area.width, 0.55)),
        Constraint::Length(get_percentage_width(area.width, 0.55)),
    ];

    // code snippet based on spotify-tui. essentially allows
    // scrollable tables
    let rows = app.items.iter().skip(offset).enumerate().map(|(i, item)| {
        if Some(i) == app.selected.checked_sub(offset) {
            Row::StyledData(item.into_iter(), selected_style)
        } else {
            Row::StyledData(item.into_iter(), normal_style)
        }
    });

    // instantiate the table with the tasks provided in the task list
    let task_table = Table::new(header.into_iter(), rows)
        .block(Block::default().borders(Borders::ALL).title("ALL TASKS"))
        .widths(&widths)
        .column_spacing(1);

    f.render_widget(task_table, area);
}

/*
 * Draw the current task that has been selected.
 */
pub fn draw_current_task<B>(f: &mut Frame<B>, app: &App, area: Rect)
where
    B: Backend,
{
    let mut new_shit = vec![];
    let x = Text::styled(
        "DO THIS SHIT\n\n",
        Style::default().bg(Color::Green).modifier(Modifier::BOLD),
    );
    new_shit.push(x);

    // push whatever the current task is
    for i in 0..app.current_task.len() {
        new_shit.push(Text::raw(&app.current_task[i]));
    }
    let task_paragraph = Paragraph::new(new_shit.iter())
        .block(Block::default().title("CURRENT TASK").borders(Borders::ALL))
        .alignment(Alignment::Center)
        .wrap(true);
    f.render_widget(task_paragraph, area);
}

/*
 * Draw the counter to keep track of the number
 * of tags done
 */
pub fn draw_tag_counter<B>(f: &mut Frame<B>, app: &App, area: Rect)
where
    B: Backend,
{
    let stuff = app
        .completed
        .iter()
        .map(|(tag, ctr)| Text::styled(format!("{}: {}", tag, ctr), Style::default()));

    let task_ctr = List::new(stuff).block(Block::default().borders(Borders::ALL).title("COUNTER"));
    //.start_corner(Corner::BottomRight);
    f.render_widget(task_ctr, area);
}
