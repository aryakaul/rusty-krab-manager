use rodio::Sink;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;

/*
 * Play a given sound given a file path to that sound and
 * a preexisting rodio sink
 */
pub fn playsound(filepath: &str, sink: &Sink) -> Result<(), Box<dyn Error>> {
    let file = File::open(filepath)?;
    let source = rodio::Decoder::new(BufReader::new(file))?;
    sink.append(source);
    sink.play();
    Ok(())
}

/*
 * Initialize the first audio sink to be
 * used in the application
 */
pub fn initialize_audio_sink() -> Sink {
    let device = rodio::default_output_device().unwrap();
    let sink = Sink::new(&device);
    sink.set_volume(0.5);
    return sink;
}
