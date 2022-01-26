use notify_rust::Notification;
use rodio::decoder::DecoderError;
use rodio::Sink;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

// Play a given sound given a file path to that sound and
// a preexisting rodio sink
pub fn playsound(filepath: &Path, sink: &Sink) -> Result<(), DecoderError> {
    let file = File::open(filepath).unwrap();
    let source = rodio::Decoder::new(BufReader::new(file))?;
    sink.append(source);
    sink.play();
    Ok(())
}

pub fn shownotif() -> Result<(), Box<dyn Error>> {
    Notification::new()
        .summary("Rusty-Krab-Manager")
        .body("Time is up!")
        .icon("clock")
        .show()?;
    Ok(())
}
