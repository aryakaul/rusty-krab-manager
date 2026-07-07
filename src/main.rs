//! Entry point. Parses CLI args, loads config + the task store, builds the LLM
//! manager backend, initializes the terminal, and runs the async event loop
//! (`select!` over keyboard input, a 250ms tick, and off-thread manager
//! results). All application state and behavior live in [`session::Session`].

mod default_files;
mod effects;
mod manager;
mod rand_utils;
mod selection;
mod session;
mod settings_util;
mod store;
mod task;
mod ui;

use clap::{ArgAction, ArgMatches};
use crossterm::event::{Event as CEvent, EventStream, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use session::{AppEvent, ManagerMsg, Session};
use std::error::Error;
use std::io;
use std::process::exit;
use std::sync::Arc;
use std::time::Duration;
use store::Store;

#[macro_use]
extern crate pathsep;

// Resolve the config file path: the explicit `-c/--config` value, or the
// default `config.toml` under the config dir (scaffolding defaults on first
// run).
fn load_or_create_configuration_file(args: &ArgMatches) -> io::Result<String> {
    if let Some(c) = args.get_one::<String>("config") {
        return Ok(c.to_string());
    }
    // create() makes the config dir and scaffolds any missing default files.
    default_files::create();
    let fullpath = default_files::config_dir().join("config.toml");
    Ok(fullpath.to_str().unwrap().to_string())
}

// Build the manager backend from config (or --no-llm). Returns the backend and
// whether the manager is available (which gates every in-TUI LLM trigger). A
// disabled/misconfigured/offline setup yields the no-op `NoneBackend`.
fn build_manager(
    cfg: &settings_util::ConfigOptions,
    no_llm: bool,
) -> (Arc<dyn manager::ManagerBackend>, bool) {
    let none =
        || -> (Arc<dyn manager::ManagerBackend>, bool) { (Arc::new(manager::NoneBackend), false) };
    if no_llm {
        eprintln!("manager: disabled (--no-llm classic mode)");
        return none();
    }
    if !cfg.manager_enabled {
        return none();
    }

    // Normalize the provider; anything unrecognized falls back to Claude.
    let provider = match cfg.manager_provider.as_str() {
        p @ ("claude" | "ollama" | "llamacpp" | "openai" | "local") => p,
        other => {
            eprintln!("manager: unknown provider '{other}', defaulting to Claude");
            "claude"
        }
    };

    if provider == "claude" {
        let model = cfg
            .manager_model
            .clone()
            .unwrap_or_else(|| "claude-haiku-4-5".to_string());
        return match manager::ClaudeBackend::new(model) {
            Ok(backend) => {
                eprintln!("manager: Claude backend ready ({})", backend.model());
                (Arc::new(backend), true)
            }
            Err(e) => {
                eprintln!("manager: disabled ({e})");
                none()
            }
        };
    }

    // OpenAI-compatible providers: resolve endpoint + model from presets/config.
    let (preset_url, default_model): (Option<&str>, Option<&str>) = match provider {
        "ollama" => (Some("http://localhost:11434/v1"), Some("llama3.1")),
        "llamacpp" => (Some("http://localhost:8080/v1"), Some("local")),
        _ => (None, None), // openai | local
    };
    let base_url = cfg
        .manager_base_url
        .clone()
        .or_else(|| preset_url.map(str::to_string));
    let model = cfg
        .manager_model
        .clone()
        .or_else(|| default_model.map(str::to_string));
    match (base_url, model) {
        (Some(url), Some(model)) => {
            let backend = manager::OpenAiBackend::new(model, url.clone());
            eprintln!(
                "manager: OpenAI-compatible backend ready ({} @ {})",
                backend.model(),
                url
            );
            (Arc::new(backend), true)
        }
        _ => {
            eprintln!(
                "manager: disabled (provider '{}' needs manager.base_url and \
                 manager.model)",
                cfg.manager_provider
            );
            none()
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let matches = clap::Command::new("Rusty-Krab-Manager")
        .about("Pomodoro inspired TUI task manager")
        .author("Arya K.")
        .version("X.X")
        .arg(
            clap::Arg::new("config")
                .short('c')
                .long("config")
                .value_name("FILE")
                .help("Path for a config file")
                .action(ArgAction::Set),
        )
        .arg(
            clap::Arg::new("no-sound")
                .short('m')
                .long("no-sound")
                .visible_alias("mute")
                .help("Disable sound playback (useful for debugging)")
                .action(ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("no-llm")
                .long("no-llm")
                .help("Disable the LLM manager (classic offline mode)")
                .action(ArgAction::SetTrue),
        )
        .get_matches();

    let no_sound = matches.get_flag("no-sound");
    let no_llm = matches.get_flag("no-llm");
    let config = load_or_create_configuration_file(&matches)?;
    let cfg = settings_util::readin_settings(&config)?;

    // Load the single TOML task store; bail early if there's nothing to work on.
    let store = Store::load_or_default(&cfg.store_path);
    if !store.has_selectable(&cfg.tags) {
        eprintln!(
            "No selectable tasks found in {}.\nAdd tasks (matching a configured \
             tag) and try again.",
            cfg.store_path.display()
        );
        exit(1);
    }

    // Warn (but keep running) if the configured sound file is missing, unless
    // sound is disabled anyway via --no-sound.
    if !no_sound && !cfg.sound_path.exists() {
        eprintln!(
            "warning: sound file {} does not exist; continuing without sound",
            cfg.sound_path.display()
        );
    }

    // Build the LLM manager backend from config (or --no-llm). `manager_available`
    // gates the in-TUI triggers; when false we keep a no-op backend.
    let (manager, manager_available) = build_manager(&cfg, no_llm);

    // Manager runs happen off the UI thread; results return over this channel
    // (a third arm of the event loop's `select!`).
    let (manager_tx, mut manager_rx) = tokio::sync::mpsc::unbounded_channel::<ManagerMsg>();

    // spawn the background effects worker (notifications + sound)
    let fx = effects::spawn();

    // Terminal initialization for UI. Mouse capture stays off: nothing
    // consumes mouse events, and capturing them turns every wheel scroll
    // into a stream of redraw wakeups.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    // async event sources: terminal input + a 250ms tick
    let mut reader = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(250));

    let mut session = Session::new(
        cfg,
        no_sound,
        store,
        fx,
        manager,
        manager_available,
        manager_tx,
    );
    session.start();

    // Enter into UI drawing infinite loop
    loop {
        terminal.draw(|f| session.draw(f))?;

        // keybindings + timer tick + manager results, whichever fires first
        let app_event = tokio::select! {
            maybe_event = reader.next() => match maybe_event {
                Some(Ok(CEvent::Key(key))) if key.kind == KeyEventKind::Press => {
                    AppEvent::Key(key.code)
                }
                _ => continue,
            },
            maybe_result = manager_rx.recv() => match maybe_result {
                Some(result) => AppEvent::Manager(result),
                None => continue,
            },
            _ = ticker.tick() => AppEvent::Tick,
        };

        match app_event {
            AppEvent::Key(code) => {
                if session.handle_key(code) {
                    break;
                }
            }
            AppEvent::Manager(result) => session.handle_manager(result),
            AppEvent::Tick => session.handle_tick(),
        }
    }

    // restore the terminal on exit
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
