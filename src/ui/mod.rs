pub mod event;
use tui::backend::Backend;
use tui::layout::{Alignment, Constraint, Rect};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{
    Block, BorderType, Borders, Gauge, List, ListItem, Paragraph, Row, Table, TableState, Wrap,
};
use tui::Frame;

pub struct HelpTable<'a> {
    state: TableState,
    items: Vec<Vec<&'a str>>,
}

pub struct WeightTable {
    state: TableState,
    items: Vec<Vec<String>>,
}

impl<'a> WeightTable {
    pub fn new(weight_table_vec: Vec<Vec<String>>) -> WeightTable {
        WeightTable {
            state: TableState::default(),
            items: weight_table_vec,
        }
    }
    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i > self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
}

pub fn draw_weights<B>(f: &mut Frame<B>, tagweight_table: &mut WeightTable, area: Rect)
where
    B: Backend,
{
    let selected_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let normal_style = Style::default().fg(Color::White);
    let header = [
        "Tag",
        "Task",
        "TagProb",
        "DueProb",
        "TotalProb",
    ];
    let widths = [
        Constraint::Percentage(20),
        Constraint::Percentage(40),
        Constraint::Percentage(10),
        Constraint::Percentage(10),
        Constraint::Percentage(20),
    ];
    let rows = tagweight_table
        .items
        .iter()
        .map(|i| Row::StyledData(i.iter(), normal_style));

    // instantiate the table with the tasks provided in the task list
    let table = Table::new(header.iter(), rows)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("TAG WEIGHT TABLE")
                .border_type(BorderType::Rounded),
        )
        .highlight_style(selected_style)
        .widths(&widths);

    f.render_stateful_widget(table, area, &mut tagweight_table.state);
}

impl<'a> HelpTable<'a> {
    pub fn new() -> HelpTable<'a> {
        HelpTable {
            state: TableState::default(),
            items: vec![
                vec!["k", "scroll up in ALL TASKS table"],
                vec!["j", "scroll down in ALL TASKS table"],
                vec!["r", "reroll the given task without marking as complete"],
                vec!["c", "complete the given task and select a new one"],
                vec!["f", "fast forward current task bar to completion"],
                vec!["p", "toggle pause"],
                vec!["s", "access stats menu"],
                vec!["q", "quit rusty-krab-manager"],
                vec!["h", "toggle help menu"],
            ],
        }
    }
    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i > self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
}

pub fn draw_help<B>(f: &mut Frame<B>, helptable: &mut HelpTable, area: Rect)
where
    B: Backend,
{
    let selected_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let normal_style = Style::default().fg(Color::White);
    let header = ["Keypress", "Description"];
    let widths = [Constraint::Percentage(20), Constraint::Percentage(80)];
    let rows = helptable
        .items
        .iter()
        .map(|i| Row::StyledData(i.iter(), normal_style));

    // instantiate the table with the tasks provided in the task list
    let table = Table::new(header.iter(), rows)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("HELP TABLE")
                .border_type(BorderType::Rounded),
        )
        .highlight_style(selected_style)
        .highlight_symbol(" ")
        .widths(&widths);

    f.render_stateful_widget(table, area, &mut helptable.state);
}

/*
 * Define the current TUI application
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
        .block(
            Block::default()
                .title("TIME LEFT")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .gauge_style(Style::default().fg(Color::Yellow))
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

    let selected_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let normal_style = Style::default().fg(Color::White);
    let header = ["\nTag", "\nName", "\nDue Date"];
    let widths = [
        Constraint::Percentage(20),
        Constraint::Percentage(50),
        Constraint::Percentage(30),
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
    let task_table = Table::new(header.iter(), rows)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("ALL TASKS")
                .border_type(BorderType::Rounded),
        )
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
    let x = Spans::from(Span::styled(
        "DO THIS SHIT",
        Style::default()
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD),
    ));
    new_shit.push(x);
    new_shit.push(Spans::from(Span::raw("")));

    // push whatever the current task is
    for i in 0..app.current_task.len() {
        new_shit.push(Spans::from(Span::raw(&app.current_task[i])));
    }
    let task_paragraph = Paragraph::new(new_shit.clone())
        .block(
            Block::default()
                .title("CURRENT TASK")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
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
    let stuff: Vec<ListItem> = app
        .completed
        .iter()
        .map(|(tag, ctr)| {
            let tagspan = Spans::from(vec![Span::styled(
                tag.to_owned() + ": " + ctr,
                Style::default().add_modifier(Modifier::ITALIC),
            )]);
            //let ctrspan = Spans::from(vec![Span::raw(ctr)]);
            ListItem::new(vec![
                tagspan,
                //ctrspan,
            ])
        })
        .collect();

    let task_ctr = List::new(stuff).block(
        Block::default()
            .borders(Borders::ALL)
            .title("COUNTER")
            .border_type(BorderType::Rounded),
    );
    //.start_corner(Corner::BottomRight);
    f.render_widget(task_ctr, area);
}
