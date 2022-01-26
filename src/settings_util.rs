use chrono::{Datelike, Local};
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
    let weights_mon = settings.get_array("weights.mon")?;
    let weights_mon: Vec<f64> = weights_mon
        .into_iter()
        .map(|i| i.into_float().unwrap())
        .collect();
    let weights_tue = settings.get_array("weights.tue")?;
    let weights_tue: Vec<f64> = weights_tue
        .into_iter()
        .map(|i| i.into_float().unwrap())
        .collect();
    let weights_wed = settings.get_array("weights.wed")?;
    let weights_wed: Vec<f64> = weights_wed
        .into_iter()
        .map(|i| i.into_float().unwrap())
        .collect();
    let weights_thu = settings.get_array("weights.thu")?;
    let weights_thu: Vec<f64> = weights_thu
        .into_iter()
        .map(|i| i.into_float().unwrap())
        .collect();
    let weights_fri = settings.get_array("weights.fri")?;
    let weights_fri: Vec<f64> = weights_fri
        .into_iter()
        .map(|i| i.into_float().unwrap())
        .collect();
    let weights_sat = settings.get_array("weights.sat")?;
    let weights_sat: Vec<f64> = weights_sat
        .into_iter()
        .map(|i| i.into_float().unwrap())
        .collect();
    let weights_sun = settings.get_array("weights.sun")?;
    let weights_sun: Vec<f64> = weights_sun
        .into_iter()
        .map(|i| i.into_float().unwrap())
        .collect();
    let curr_day: u32 = Local::now().weekday().number_from_monday();
    let tag_weights = match curr_day {
        1 => weights_mon,
        2 => weights_tue,
        3 => weights_wed,
        4 => weights_thu,
        5 => weights_fri,
        6 => weights_sat,
        _ => weights_sun,
    };
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
