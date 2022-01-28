use chrono::{Datelike, Local};
use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};

// Take the settings file and convert it
// to a series of raw values to be used

pub struct ConfigOptions {
    pub task_path: PathBuf,
    pub sound_path: PathBuf,
    pub sound_volume: f64,
    pub tags: Vec<String>,
    pub use_due_dates: Vec<bool>,
    pub initial_tag_weights: Vec<f64>,
    pub min_break_time: i64,
    pub max_break_time: i64,
    pub task_time: i64,
    pub maxno_min_breaks: i64,
}

pub fn readin_settings(config_path: &str) -> Result<ConfigOptions, Box<dyn Error>> {
    // Read in configuration
    let mut settings = config::Config::new();
    settings.merge(config::File::with_name(config_path))?;

    // get the paths to the task file and sound file
    let task_path = settings.get("task_filepath")?;
    assert!(
        Path::new(&task_path).exists(),
        "task filepath does not exist"
    );

    let sound_path = settings.get("sound.file")?;
    assert!(
        Path::new(&sound_path).exists(),
        "Sound filepath does not exist"
    );
    let sound_volume = settings.get_float("sound.volume")?;

    // get the vector of tags
    let tags = settings.get_array("tags")?;
    let tags: Vec<String> = tags.into_iter().map(|i| i.into_str().unwrap()).collect();
    let taglen = tags.len();

    // get boolean vector of whether to use due dates or not
    let use_due_dates = settings.get_array("use_due_dates")?;
    let use_due_dates: Vec<bool> = use_due_dates
        .into_iter()
        .map(|i| i.into_bool().unwrap())
        .collect();

    assert!(
        taglen == use_due_dates.len(),
        "use_due_dates vector length does not match number of tags in config"
    );

    // get weights tags for all days of the week
    let mut weights_map = HashMap::new();
    for day in ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] {
        let path = format!("weights.{}", day);
        let weights: Vec<f64> = settings
            .get_array(&path)?
            .into_iter()
            .map(|i| i.into_float().unwrap())
            .collect();
        weights_map.insert(day, weights);
    }

    let curr_day: u32 = Local::now().weekday().number_from_monday();
    let tag_weights = match curr_day {
        1 => weights_map.remove("mon"),
        2 => weights_map.remove("tue"),
        3 => weights_map.remove("wed"),
        4 => weights_map.remove("thu"),
        5 => weights_map.remove("fri"),
        6 => weights_map.remove("sat"),
        7 => weights_map.remove("sun"),
        _ => unreachable!(),
    }
    .unwrap();
    assert!(
        taglen == tag_weights.len(),
        "current day tag weights do not match number of tags in config"
    );

    let error_margin = f64::EPSILON;
    let tag_weights_sum: f64 = tag_weights.iter().sum();
    assert!(
        (tag_weights_sum - 1.0).abs() <= error_margin,
        "current day tag weights do not sum to 1. they sum to {}",
        tag_weights_sum
    );

    let min_break_time = settings.get_int("short_break_time")?;
    let max_break_time = settings.get_int("long_break_time")?;
    let task_time = settings.get_int("task_time")?;
    let maxno_min_breaks = settings.get_int("maxno_short_breaks")?;

    Ok(ConfigOptions {
        task_path,
        sound_path,
        sound_volume,
        tags,
        use_due_dates,
        initial_tag_weights: tag_weights,
        min_break_time,
        max_break_time,
        task_time,
        maxno_min_breaks,
    })
}
