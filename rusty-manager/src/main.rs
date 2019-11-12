use rand::Rng;
//use std::time::{SystemTime};
//use chrono::{DateTime, Duration};
use chrono::prelude::*;



fn make_cdf(pdf:Vec<f64>) -> Vec::<f64> {
    let sum: f64 = pdf.iter().sum();
    if sum as f32 != 1.0 {
        println!("{}", sum as f32);
        panic!("Probability distribution does not sum to 1!");
    }
    let mut cdf: Vec<f64> = Vec::with_capacity(pdf.len());
    cdf.push(pdf[0]);
    for idx in 1..pdf.len() {
        cdf.push(cdf[idx-1]+pdf[idx]);
    }
    return cdf
}

fn roll_die(pdf: Vec<f64>) -> usize {
    let mut rng = rand::thread_rng();
    let x = rng.gen::<f64>();
    let cdf: Vec<f64> = make_cdf(pdf);
    let index = cdf.iter().position(|&r| x < r).unwrap();
    return index;
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
        pdf.push(biggest/due[i] as f64);
    }
    let sum: f64 = pdf.iter().sum();
    for i in 0..pdf.len() {
        pdf[i] = pdf[i]/sum;
        //println!("{}", pdf[i]);
    }
    return pdf;
}

/* Define 'Assignment' object
 */
pub struct Assignment {
    name: String,
    tag: String,
    due_time: String,
    complete: bool
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
    pub fn mark_complete(&mut self) {
        self.complete = true;
    }
    pub fn convert_due_date(&self) -> DateTime<Local> {
        let convert_due_date = Local.datetime_from_str(&self.due_time, "%Y-%m-%d %H:%M");
        return convert_due_date.unwrap();
    }
}


/* Read in the list of assignments. Return hash map where 
 * key is the tag, and value is a Vector of Assignments
 */



/* Get the amount of time until a given assignment is due
 * Get it in minutes
 */
fn find_timeuntildue(due_date: DateTime<Local>) -> i64 {
    let curr_local: DateTime<Local> = Local::now(); 
    let duration = due_date.signed_duration_since(curr_local).num_minutes();
    //println!("{}", duration);
    return duration
}


fn turn_assignmentvector_into_pdf(assign: Vec<Assignment>) -> Vec<f64> {
    let mut min_till_due: Vec<i64> = Vec::new();
    for i in 0..assign.len() {
        min_till_due.push(find_timeuntildue(assign[i].convert_due_date()));
    }
    return turn_timetilldue_into_pdf(min_till_due);
}

fn main() {
    //let xs: [f64; 2] = [0.3,0.7];

    let mut assignments: Vec<Assignment> = Vec::new();
    let bmif_assign = Assignment {
        name: String::from("BMIF Problem Set"),
        tag: String::from("school"),
        due_time: String::from("2019-11-12 23:59"),
        complete: false,
    };
    assignments.push(bmif_assign);
    let gen_assign = Assignment {
        name: String::from("Genetics Problem Set"),
        tag: String::from("school"),
        due_time: String::from("2019-11-19 09:00"),
        complete: false,
    };
    assignments.push(gen_assign);
    let x = turn_assignmentvector_into_pdf(assignments);
    for i in 0..x.len() {
        println!("{}", x[i]);
    }
    println!("{}", roll_die(x));
    //println!("{}", roll_die(pdf));
}
