use notify_rust::Notification;
use rodio::decoder::DecoderError;
use rodio::Sink;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::error::Error;

// Play a given sound given a file path to that sound and
// a preexisting rodio sink
pub fn playsound(filepath: &Path, sink: &Sink) -> Result<(), DecoderError> {
    let file = File::open(filepath).unwrap();
    let source = rodio::Decoder::new(BufReader::new(file))?;
    sink.append(source);
    sink.play();
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
