use rodio::Sink;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;

/*
 * Play a given sound given a file path to that sound and
 * a preexisting rodio sink
 */
pub fn playsound(filepath: &str, sink: &Sink) -> Result<(), Box<dyn Error>> {
    let file = File::open(filepath).unwrap();
    let source = rodio::Decoder::new(BufReader::new(file)).unwrap();
    sink.append(source);
    sink.play();
    Ok(())
}
