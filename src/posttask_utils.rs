use notify_rust::Notification;
//use rodio::decoder::DecoderError;
//use rodio::Sink;
use soloud::*;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

// Play a given sound given a file path to that sound and
// a preexisting rodio sink
pub fn playsound(filepath: &Path, volume: f64) -> Result<(), Box <dyn Error>> {
    let mut sl = Soloud::default()?;
    sl.set_global_volume(volume as f32);

    //let mut wav = audio::Wav::default();

    //wav.load(&std::path::Path::new(filepath))?;

    //sl.play(&wav); // calls to play are non-blocking, so we put the thread to sleep
    let mut wav = audio::Wav::default();
    wav.load(filepath).unwrap();
    sl.play(&wav);
    while sl.voice_count() > 0 {
       std::thread::sleep(std::time::Duration::from_millis(100));
    }

    Ok(())
}

pub fn finishnotif() -> Result<(), Box<dyn Error>> {
    Notification::new()
        .summary("Rusty-Krab-Manager")
        .body("Time is up!")
        .icon("clock")
        .show()?;
    Ok(())
}

pub fn nextupnotif(nexttask: &str) -> Result<(), Box<dyn Error>> {
    Notification::new()
        .summary("Rusty-Krab-Manager")
        .body(&format!("Now work on: {}", nexttask))
        .icon("clock")
        .show()?;
    Ok(())
}
