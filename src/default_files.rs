use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use tinytemplate::TinyTemplate;

// Create files required for operation if they don't exist, such as:
// 1. Config      ($CONFIG/rusty-krab-manager/config.toml)
// 2. "Ocean man" ($CONFIG/rusty-krab-manager/ocean_man.mp3)
// 3. Tasks       ($HOME/rusty-krab-manager-tasks.txt)
pub fn create_default_files() {
    let mut config_subdir = dirs::config_dir().unwrap();
    config_subdir.push("rusty-krab-manager");
    let mut config_filepath = config_subdir.clone();
    config_filepath.push("config.toml");
    let mut sound_filepath = config_subdir.clone();
    sound_filepath.push("ocean_man.mp3");
    let mut task_filepath = config_subdir.clone();
    task_filepath.push("example_tasks.csv");

    create_default_file(
        &config_filepath,
        fill_config(DefaultConfigData {
            sound_filepath: sound_filepath.to_str().unwrap().to_string(),
            task_filepath: task_filepath.to_str().unwrap().to_string(),
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
    create_default_file(
        &task_filepath,
        include_bytes!(concat!(
            "..",
            path_separator!(),
            "assets",
            path_separator!(),
            "tasks"
        )),
    );
}

fn create_default_file(filepath: &PathBuf, contents: &[u8]) {
    if filepath.exists() {
        return;
    }

    let mut file = File::create(filepath).unwrap();
    file.write_all(contents).unwrap();
}

#[derive(Serialize)]
struct DefaultConfigData {
    task_filepath: String,
    sound_filepath: String,
}

fn fill_config(data: DefaultConfigData) -> String {
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
