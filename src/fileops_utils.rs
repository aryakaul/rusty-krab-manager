use std::{
    fs::File,
    io::{prelude::*, BufReader},
    path::Path,
};

// Read in a file and convert that into a vector of
// strings to be parsed
pub fn lines_from_file(filename: &Path) -> Vec<String> {
    let file = File::open(filename).expect("no such file");
    let buf = BufReader::new(file);
    buf.lines()
        .map(|l| l.expect("Could not parse line"))
        .collect()
}
