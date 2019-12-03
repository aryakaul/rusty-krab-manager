use super::fileops_utils::lines_from_file;

//
// THESE ARE ALL FUNCTIONS RELATED TO THE ASSIGNMENT
// STRUCTURE
//

use chrono::prelude::*;
use std::fmt;
use std::collections::HashMap;

/* Define 'Assignment' object */
pub struct Assignment {
    pub name: String,
    pub tag: String,
    pub due_time: String,
}

impl fmt::Display for Assignment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "({}, {}, {})", self.name, self.tag, self.due_time)
    }
}

impl Assignment {
    pub fn change_name(&mut self, new_name: String) {
        self.name = new_name;
    }
    pub fn change_tag(&mut self, new_tag: String) {
        self.tag = new_tag;
    }
    pub fn update_due_date(&mut self, new_due: String) {
        self.due_time = new_due;
    }
    pub fn convert_due_date(&self) -> DateTime<Local> {
        let convert_due_date = Local.datetime_from_str(&self.due_time, "%Y-%m-%d %H:%M");
        match convert_due_date {
            Ok(convert_due_date) => convert_due_date,
            Err(convert_due_date) => panic!("{}", &self.due_time),
        }
    }
    pub fn get_due_date(&self) -> &str {
        return &self.due_time;
    }
}

/* Take all minutes until due from all assignments. Find the
 * largest value. Divide that value by all values.
 * Sum these values. Use that as the denominator for all
 * values. Return this probability distribution.
 */
fn turn_timetilldue_into_pdf(due: Vec<i64>) -> Vec<f64> {
    let biggest = *due.iter().max().unwrap() as f64;
    let mut pdf: Vec<f64> = Vec::with_capacity(due.len());
    for i in 0..due.len() {
        pdf.push(biggest / due[i] as f64);
    }
    let sum: f64 = pdf.iter().sum();
    for i in 0..pdf.len() {
        pdf[i] = pdf[i] / sum;
        //println!("{}", pdf[i]);
    }
    return pdf;
}

/*
 * Get the amount of time until a given assignment is due in minutes
 */
pub fn find_timeuntildue(due_date: DateTime<Local>) -> i64 {
    let curr_local: DateTime<Local> = Local::now();
    let duration = due_date.signed_duration_since(curr_local).num_minutes();
    return duration;
}

/*
 * Turn a vector containing all assignments, and return a Vec<f64>
 * that is your probability density function for each assignment
 * The index tracks the same assignment
 */
pub fn turn_assignmentvector_into_pdf(assign: &Vec<Assignment>, use_due: bool) -> Vec<f64> {
    if use_due {
        let mut min_till_due: Vec<i64> = Vec::new();
        for i in 0..assign.len() {
            min_till_due.push(find_timeuntildue(assign[i].convert_due_date()));
        }
        return turn_timetilldue_into_pdf(min_till_due);
    } else {
        let uniform_prob: f64 = 1.0 / assign.len() as f64;
        return vec![uniform_prob; assign.len()];
    }
}

pub fn readin_tasks(filepath: &str) -> HashMap <String, Vec<Assignment>> {
    let lines = lines_from_file(filepath);
    let mut tag_to_taskvectors: HashMap<String, Vec<Assignment>> = HashMap::new();
    for line in lines {
        let task_vec: Vec<&str> = line.split("\t").collect();
        let tag = task_vec[0];
        let name = task_vec[1];
        let due_date = task_vec[2];
        let new_assign = Assignment {
            name: String::from(name),
            tag: String::from(tag),
            due_time: String::from(due_date),
        };
        //println!("{}", new_assign);
        if find_timeuntildue(new_assign.convert_due_date()) < 0 {
            continue;
        }
        if !tag_to_taskvectors.contains_key(tag) {
            let task_vector: Vec<Assignment> = Vec::new();
            tag_to_taskvectors.insert(tag.to_string(), task_vector);
        };
        
        let curr_vector = tag_to_taskvectors.get_mut(tag).unwrap();
        curr_vector.push(new_assign);
    }
    return tag_to_taskvectors
}

// convert to a vector of strings
pub fn hashmap_to_taskvector(tagmap: HashMap <String, Vec<Assignment>>) -> Vec<Vec<String>> {
    let mut toret = vec![];
    for (tag, assign_vec) in &tagmap {
        for i in 0..assign_vec.len() {
            let mut new = vec![];
            let curr_assign = &assign_vec[i];
            new.push(curr_assign.tag.clone());
            new.push(curr_assign.name.clone());
            new.push(curr_assign.due_time.clone());
            toret.push(new);
        }
    }
    return toret
}
