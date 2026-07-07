//! Config loading. `readin_settings` parses the TOML config file into a flat
//! `ConfigOptions` (tags, per-weekday tag weights, timings, sound, store path,
//! and the `[manager]` section), selecting the current day's weights. The
//! manager API key is never read here — only `$ANTHROPIC_API_KEY`.

use chrono::{Datelike, Local};
use std::error::Error;
use std::path::PathBuf;

pub struct ConfigOptions {
    // Single human-editable TOML task store. Defaults to <config dir>/tasks.toml.
    pub store_path: PathBuf,
    pub sound_path: PathBuf,
    pub sound_volume: f64,
    pub tags: Vec<String>,
    pub use_due_dates: Vec<bool>,
    pub initial_tag_weights: Vec<f64>,
    pub min_break_time: i64,
    pub max_break_time: i64,
    pub task_time: i64,
    pub maxno_min_breaks: i64,
    // LLM manager (Phase 2/4). API keys are read from the environment at
    // runtime ($ANTHROPIC_API_KEY / $OPENAI_API_KEY), never from this file.
    pub manager_enabled: bool,
    // Backend: "claude" (default), "ollama", "llamacpp", "openai"/"local".
    pub manager_provider: String,
    // Model name; None falls back to a per-provider default in main.
    pub manager_model: Option<String>,
    // OpenAI-compatible endpoint; None uses the provider's preset default.
    pub manager_base_url: Option<String>,
}

pub fn readin_settings(config_path: &str) -> Result<ConfigOptions, Box<dyn Error>> {
    // Read in configuration
    //let mut settings = config::Config::new();
    //settings.merge(config::File::with_name(config_path))?;

    let settings = config::Config::builder()
        // Add in `./Settings.toml`
        .add_source(config::File::with_name(config_path))
        // Add in settings from the environment (with a prefix of APP)
        // Eg.. `APP_DEBUG=1 ./target/app` would set the `debug` key
        .add_source(config::Environment::with_prefix("APP"))
        .build()?;

    // The TOML store path (optional; defaults under the config dir).
    let store_path = settings
        .get::<String>("store_filepath")
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::default_files::config_dir().join("tasks.toml"));

    // Note: a missing sound *file* is tolerated (the app warns and runs
    // without sound); only a missing `sound.file` config key is an error.
    let sound_path = settings.get("sound.file")?;
    let sound_volume = settings.get_float("sound.volume")?;

    // get the vector of tags
    let tags: Vec<String> = settings
        .get_array("tags")?
        .into_iter()
        .map(|i| i.into_string())
        .collect::<Result<_, _>>()?;
    let taglen = tags.len();

    // get boolean vector of whether to use due dates or not
    let use_due_dates: Vec<bool> = settings
        .get_array("use_due_dates")?
        .into_iter()
        .map(|i| i.into_bool())
        .collect::<Result<_, _>>()?;

    assert!(
        taglen == use_due_dates.len(),
        "use_due_dates vector length does not match number of tags in config"
    );

    // Load every weekday's tag weights (validating each), then pick today's by
    // index. `number_from_monday()` is 1-7, so `curr_day - 1` indexes the array.
    let day_weights: Vec<Vec<f64>> = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
        .iter()
        .map(|day| {
            settings
                .get_array(&format!("weights.{day}"))?
                .into_iter()
                .map(|i| i.into_float())
                .collect::<Result<Vec<f64>, _>>()
        })
        .collect::<Result<_, _>>()?;
    let curr_day = Local::now().weekday().number_from_monday();
    let tag_weights = day_weights[(curr_day - 1) as usize].clone();
    assert!(
        taglen == tag_weights.len(),
        "current day tag weights do not match number of tags in config"
    );

    // Loose tolerance so config authors aren't forced to hit exactly 1.0 in
    // floating point (e.g. 0.2 + 0.7 + 0.1 doesn't sum to a clean 1.0).
    let tag_weights_sum: f64 = tag_weights.iter().sum();
    assert!(
        (tag_weights_sum - 1.0).abs() <= crate::rand_utils::PROB_SUM_TOLERANCE,
        "current day tag weights do not sum to 1. they sum to {}",
        tag_weights_sum
    );

    let min_break_time = settings.get_int("short_break_time")?;
    let max_break_time = settings.get_int("long_break_time")?;
    let task_time = settings.get_int("task_time")?;
    let maxno_min_breaks = settings.get_int("maxno_short_breaks")?;

    // The manager section is optional; absent keys fall back to disabled, the
    // Claude provider, and per-provider default model/endpoint (resolved in
    // main).
    let manager_enabled = settings.get_bool("manager.enabled").unwrap_or(false);
    let manager_provider = settings
        .get::<String>("manager.provider")
        .unwrap_or_else(|_| "claude".to_string());
    let manager_model = settings.get::<String>("manager.model").ok();
    let manager_base_url = settings.get::<String>("manager.base_url").ok();

    Ok(ConfigOptions {
        store_path,
        sound_path,
        sound_volume,
        tags,
        use_due_dates,
        initial_tag_weights: tag_weights,
        min_break_time,
        max_break_time,
        task_time,
        maxno_min_breaks,
        manager_enabled,
        manager_provider,
        manager_model,
        manager_base_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // Identical weights every day so the tests don't depend on the weekday
    // `readin_settings` picks at runtime.
    const BASE: &str = r#"
store_filepath = "/tmp/rkm-test-tasks.toml"
tags = ["school", "job"]
use_due_dates = [true, false]
short_break_time = 1
long_break_time = 5
task_time = 20
maxno_short_breaks = 2

[sound]
file = "/tmp/nope.mp3"
volume = 0.3

[weights]
mon = [0.6, 0.4]
tue = [0.6, 0.4]
wed = [0.6, 0.4]
thu = [0.6, 0.4]
fri = [0.6, 0.4]
sat = [0.6, 0.4]
sun = [0.6, 0.4]
"#;

    fn write_config(body: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("rkm-config-{}.toml", uuid::Uuid::new_v4()));
        fs::write(&path, body).unwrap();
        path
    }

    fn read(body: &str) -> Result<ConfigOptions, Box<dyn Error>> {
        let path = write_config(body);
        let result = readin_settings(path.to_str().unwrap());
        fs::remove_file(&path).ok();
        result
    }

    #[test]
    fn parses_full_config_with_manager_defaults() {
        let cfg = read(BASE).unwrap();
        assert_eq!(cfg.tags, ["school", "job"]);
        assert_eq!(cfg.use_due_dates, [true, false]);
        assert_eq!(cfg.initial_tag_weights, [0.6, 0.4]);
        assert_eq!(cfg.task_time, 20);
        assert_eq!(cfg.min_break_time, 1);
        assert_eq!(cfg.max_break_time, 5);
        assert_eq!(cfg.maxno_min_breaks, 2);
        assert_eq!(cfg.store_path, PathBuf::from("/tmp/rkm-test-tasks.toml"));
        // No [manager] section: disabled, claude provider, no overrides.
        assert!(!cfg.manager_enabled);
        assert_eq!(cfg.manager_provider, "claude");
        assert!(cfg.manager_model.is_none());
        assert!(cfg.manager_base_url.is_none());
    }

    #[test]
    fn parses_manager_section() {
        let body = format!(
            "{BASE}\n[manager]\nenabled = true\nprovider = \"ollama\"\n\
             model = \"llama3.1\"\nbase_url = \"http://localhost:11434/v1\"\n"
        );
        let cfg = read(&body).unwrap();
        assert!(cfg.manager_enabled);
        assert_eq!(cfg.manager_provider, "ollama");
        assert_eq!(cfg.manager_model.as_deref(), Some("llama3.1"));
        assert_eq!(
            cfg.manager_base_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
    }

    #[test]
    fn store_path_defaults_under_config_dir() {
        let body = BASE.replace("store_filepath = \"/tmp/rkm-test-tasks.toml\"\n", "");
        let cfg = read(&body).unwrap();
        assert_eq!(
            cfg.store_path,
            crate::default_files::config_dir().join("tasks.toml")
        );
    }

    #[test]
    #[should_panic(expected = "do not sum to 1")]
    fn rejects_weights_not_summing_to_one() {
        let _ = read(&BASE.replace("[0.6, 0.4]", "[0.6, 0.3]"));
    }

    #[test]
    #[should_panic(expected = "does not match number of tags")]
    fn rejects_mismatched_use_due_dates() {
        let _ = read(&BASE.replace("use_due_dates = [true, false]", "use_due_dates = [true]"));
    }
}
