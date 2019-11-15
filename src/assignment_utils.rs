//
// THESE ARE ALL FUNCTIONS RELATED TO THE ASSIGNMENT
// STRUCTURE
//

use chrono::prelude::*;

/* Define 'Assignment' object */
pub struct Assignment {
    pub name: String,
    pub tag: String,
    pub due_time: String,
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
        return convert_due_date.unwrap();
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
fn find_timeuntildue(due_date: DateTime<Local>) -> i64 {
    let curr_local: DateTime<Local> = Local::now();
    let duration = due_date.signed_duration_since(curr_local).num_minutes();
    return duration;
}

/*
 * Turn a vector containing all assignments, and return a Vec<f64>
 * that is your probability density function for each assignment
 * The index tracks the same assignment
 */
pub fn turn_assignmentvector_into_pdf(assign: Vec<Assignment>, use_due: bool) -> Vec<f64> {
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
