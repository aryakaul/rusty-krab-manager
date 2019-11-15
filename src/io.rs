#[path = "assignment_utils.rs"]
mod assignment_utils;
use std::io::{BufRead, BufReader};
use std::fs::File;
use std::collections::HashMap;

pub fn readin_tasks(filepath: &str) -> HashMap <&str, Vec<assignment_utils::Assignment>> {
    let reader = BufReader::new(File::open(filepath).expect("Cannot open given file"));
    let mut tag_to_taskvectors = HashMap::new();
    for line in reader.lines() {
        if let Ok(task_to_parse) = line {
            let task_vec: Vec<&str> = task_to_parse.split("\t").collect();
            //let task_vec: Vec<&str> = String::(line.unwrap().to_string()).split("\t").collect();
            let tag = task_vec[0];
            let name = task_vec[1];
            let due_date = task_vec[2];
            let new_assign = assignment_utils::Assignment {
                name: String::from(name),
                tag: String::from(tag),
                due_time: String::from(due_date),
            };
            if !tag_to_taskvectors.contains_key(tag) {
                let task_vector: Vec<assignment_utils::Assignment> = Vec::new();
                tag_to_taskvectors.insert(tag, task_vector);
            }
            let curr_vector = tag_to_taskvectors.get_mut(tag).unwrap();
            curr_vector.push(new_assign);
            //tag_to_taskvectors.insert(tag, curr_vector.to_vec());
        }
    }
    return tag_to_taskvectors
}
