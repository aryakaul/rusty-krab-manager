//! First-run scaffolding. `config_dir` resolves the XDG config directory; on a
//! fresh install `create` writes a default `config.toml` (from the bundled
//! template), the notification sound, and an example `tasks.md` generated from
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
// 3. Task store  ($CONFIG/rusty-krab-manager/tasks.md)
pub fn create() {
    let config_subdir = config_dir();
    std::fs::create_dir_all(&config_subdir).expect("could not create config dir");

    let config_filepath = config_subdir.join("config.toml");
    let sound_filepath = config_subdir.join("ocean_man.mp3");
    let store_filepath = config_subdir.join("tasks.md");

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
    scaffold_store(&config_subdir);
}

// Seed the single Markdown store with a format demo (subtasks + due dates).
// Don't seed the example store when a legacy tasks.toml is present:
// the migration (main's resolve_store) must find tasks.md missing,
// otherwise the example would block it and the user's real tasks
// would be silently ignored.
fn scaffold_store(config_subdir: &Path) {
    if !config_subdir.join("tasks.toml").exists() {
        create_default_file(
            &config_subdir.join("tasks.md"),
            example_store_md().as_bytes(),
        );
    }
}

// An example task store demonstrating the format (sections, subtasks,
// due dates). Generated from the model so it never drifts from the
// types.
fn example_store_md() -> String {
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

    Store::seed(tasks).to_markdown()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_scaffold_skipped_when_legacy_toml_present() {
        let dir = std::env::temp_dir().join(format!("rkm-scaffold-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        // A legacy tasks.toml blocks the example scaffold so the
        // migration in main's resolve_store can run.
        std::fs::write(dir.join("tasks.toml"), "[[tasks]]\ntag=\"t\"\nname=\"a\"\n").unwrap();
        scaffold_store(&dir);
        assert!(!dir.join("tasks.md").exists());

        // Without a legacy store, the example is seeded as before.
        std::fs::remove_file(dir.join("tasks.toml")).unwrap();
        scaffold_store(&dir);
        assert!(dir.join("tasks.md").exists());

        std::fs::remove_dir_all(&dir).ok();
    }
}
