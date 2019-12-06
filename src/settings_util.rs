use chrono::{Datelike, Local};
use std::error::Error;

pub fn readin_settings(
    config_path: &str,
) -> Result<(String, Vec<String>, Vec<bool>, Vec<f64>, i64, i64, i64, i64), Box<dyn Error>> {
    // Read in configuration
    let mut settings = config::Config::new();
    settings.merge(config::File::with_name(config_path))?;
    let task_path = settings.get_str("task_filepath")?;
    let tags = settings.get_array("tags")?;
    let tags: Vec<String> = tags.into_iter().map(|i| i.into_str().unwrap()).collect();
    let use_due_dates = settings.get_array("use_due_dates")?;
    let use_due_dates: Vec<bool> = use_due_dates
        .into_iter()
        .map(|i| i.into_bool().unwrap())
        .collect();
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
    let tag_weights: Vec<f64>;
    if curr_day == 1 {
        tag_weights = weights_mon;
    } else if curr_day == 2 {
        tag_weights = weights_tue;
    } else if curr_day == 3 {
        tag_weights = weights_wed;
    } else if curr_day == 4 {
        tag_weights = weights_thu;
    } else if curr_day == 5 {
        tag_weights = weights_fri;
    } else if curr_day == 6 {
        tag_weights = weights_sat;
    } else {
        tag_weights = weights_sun;
    }
    let min_break_time = settings.get_int("short_break_time")?;
    let max_break_time = settings.get_int("long_break_time")?;
    let task_time = settings.get_int("task_time")?;
    let maxno_min_breaks = settings.get_int("maxno_short_breaks")?;

    return Ok((
        task_path,
        tags,
        use_due_dates,
        tag_weights,
        min_break_time,
        max_break_time,
        task_time,
        maxno_min_breaks,
    ));
}
