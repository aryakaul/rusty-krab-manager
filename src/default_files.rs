//! First-run scaffolding. `config_dir` resolves the XDG config directory; on a
//! fresh install `create` writes a default `config.toml` (from the bundled
//! template), the notification sound, and an example `tasks.toml` generated from
//! the model so the demo store never drifts from the types.

use crate::store::Store;
use crate::task::{parse_due, Subtask, Task};
use serde::Serialize;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use tinytemplate::TinyTemplate;

// Resolve the app config directory: $XDG_CONFIG_HOME/rusty-krab-manager,
// falling back to ~/.config/rusty-krab-manager. Used on all platforms so the
// location is consistent (notably ~/.config on macOS rather than the platform
// default of ~/Library/Application Support).
pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            dirs::home_dir()
                .expect("could not determine home directory")
                .join(".config")
        });
    base.join("rusty-krab-manager")
}

// Create files required for operation if they don't exist, such as:
// 1. Config      ($CONFIG/rusty-krab-manager/config.toml)
// 2. "Ocean man" ($CONFIG/rusty-krab-manager/ocean_man.mp3)
// 3. Task store  ($CONFIG/rusty-krab-manager/tasks.toml)
pub fn create() {
    let config_subdir = config_dir();
    std::fs::create_dir_all(&config_subdir).expect("could not create config dir");

    let config_filepath = config_subdir.join("config.toml");
    let sound_filepath = config_subdir.join("ocean_man.mp3");
    let store_filepath = config_subdir.join("tasks.toml");

    create_default_file(
        &config_filepath,
        fill_config(&DefaultConfigData {
            sound_filepath: sound_filepath.to_str().unwrap().to_string(),
            store_filepath: store_filepath.to_str().unwrap().to_string(),
        })
        .as_bytes(),
    );
    create_default_file(
        &sound_filepath,
        include_bytes!(concat!(
            "..",
            path_separator!(),
            "assets",
            path_separator!(),
            "ocean_man.mp3"
        )),
    );
    // Seed the single TOML store with a schema demo (subtasks + due dates).
    create_default_file(&store_filepath, example_store_toml().as_bytes());
}

// An example task store demonstrating the schema (subtasks, due dates,
// history). Generated from the model so it never drifts from the types.
fn example_store_toml() -> String {
    let mut homestead = Task::new(
        "personal".to_string(),
        "Build a defensible homestead 🏡".to_string(),
        parse_due("2050-01-01 11:11"),
    );
    homestead.subtasks = vec![
        Subtask {
            name: "Dig the moat".to_string(),
            done: false,
        },
        Subtask {
            name: "Raise the ramparts".to_string(),
            done: false,
        },
    ];

    let tasks = vec![
        Task::new(
            "school".to_string(),
            "Problem Set 📜".to_string(),
            parse_due("2050-10-12 10:00"),
        ),
        Task::new(
            "job".to_string(),
            "Code the thing 🖥️".to_string(),
            parse_due("2050-12-12 10:00"),
        ),
        homestead,
    ];

    Store { tasks }
        .to_toml()
        .expect("example store should serialize")
}

fn create_default_file(filepath: &Path, contents: &[u8]) {
    if filepath.exists() {
        return;
    }

    let mut file = File::create(filepath).unwrap();
    file.write_all(contents).unwrap();
}

#[derive(Serialize)]
struct DefaultConfigData {
    store_filepath: String,
    sound_filepath: String,
}

fn fill_config(data: &DefaultConfigData) -> String {
    const CONFIG_TEMPLATE: &str = include_str!(concat!(
        "..",
        path_separator!(),
        "assets",
        path_separator!(),
        "config.template.toml"
    ));
    let mut template = TinyTemplate::new();
    template.add_template("config", CONFIG_TEMPLATE).unwrap();
    template.render("config", &data).unwrap()
}
