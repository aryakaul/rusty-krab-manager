//! Entry point. Parses CLI args, loads config + the task store, builds the LLM
//! manager backend, initializes the terminal, and runs the async event loop
//! (`select!` over keyboard input, a 250ms tick, and off-thread manager
//! results). All application state and behavior live in [`session::Session`].

mod archive;
mod default_files;
mod effects;
mod manager;
mod mdstore;
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

// Point the session at a Markdown store, migrating a legacy TOML one
// (v2.0 tasks.toml) in place on first run.
fn resolve_store(cfg: &mut settings_util::ConfigOptions) {
    let path = cfg.store_path.clone();
    let is_toml = path.extension().is_some_and(|e| e == "toml");
    let md = if is_toml {
        path.with_file_name("tasks.md")
    } else {
        path.clone()
    };
    // Migrate when the configured file is TOML, or when the md file is
    // missing but a sibling tasks.toml exists.
    let legacy = if is_toml {
        path.clone()
    } else {
        md.with_file_name("tasks.toml")
    };
    if !md.exists() && legacy.exists() {
        match store::migrate_from_toml(&legacy, &md, &archive::archive_path(&md)) {
            Ok(summary) => eprintln!("store: {summary}"),
            Err(e) => eprintln!(
                "store: migration failed ({e}); your original file is untouched — \
                 fix it or move it aside and re-run"
            ),
        }
    }
    if is_toml {
        eprintln!(
            "store: note — config store_filepath points at {}; now using {}",
            path.display(),
            md.display()
        );
    }
    cfg.store_path = md;
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let matches = clap::Command::new("Rusty-Krab-Manager")
        .about("Pomodoro inspired TUI task manager")
        .author("Arya K.")
        .version(env!("CARGO_PKG_VERSION"))
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
    let mut cfg = settings_util::readin_settings(&config)?;
    resolve_store(&mut cfg);

    // Load the Markdown task store; bail early if there's nothing to work on.
    let mut store = Store::load_or_default(&cfg.store_path);
    let mut archive = archive::Archive::load_or_default(&archive::archive_path(&cfg.store_path));
    // [x] marks checked off while the app was closed are completions:
    // archive or reschedule them before deciding whether anything is
    // selectable.
    if store.process_file_completions(&mut archive) {
        if let Err(e) = store.save(&cfg.store_path) {
            eprintln!("warning: could not write task store ({e})");
        }
        if let Err(e) = archive.save(&archive::archive_path(&cfg.store_path)) {
            eprintln!("warning: could not write archive ({e})");
        }
    }
    if !store.has_selectable(&cfg.tags) {
        eprintln!(
            "No selectable tasks found in tasks.md ({}).\nAdd tasks (matching a \
             configured tag) and try again.",
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
    // Restore the terminal before any panic message prints, so a panic
    // can't leave the shell in raw mode/alt screen (and invisible).
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let mut session = Session::new(
        cfg,
        no_sound,
        store,
        archive,
        fx,
        manager,
        manager_available,
        manager_tx,
    );
    session.start();

    let res = run_loop(&mut terminal, &mut session, &mut manager_rx).await;

    // restore the terminal on exit (run_loop errors included)
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    session: &mut Session,
    manager_rx: &mut tokio::sync::mpsc::UnboundedReceiver<ManagerMsg>,
) -> Result<(), Box<dyn Error>> {
    let mut reader = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(250));
    loop {
        terminal.draw(|f| session.draw(f))?;
        let app_event = tokio::select! {
            maybe_event = reader.next() => match maybe_event {
                Some(Ok(CEvent::Key(key))) if key.kind == KeyEventKind::Press => {
                    session::map_key(key)
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
            AppEvent::Quit => return Ok(()),
            AppEvent::Key(code) => {
                if session.handle_key(code) {
                    return Ok(());
                }
            }
            AppEvent::Manager(result) => session.handle_manager(result),
            AppEvent::Tick => session.handle_tick(),
        }
    }
}
