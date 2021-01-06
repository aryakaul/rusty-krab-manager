use super::fileops_utils::lines_from_file;
use chrono::prelude::*;
use std::collections::HashMap;
use std::fmt;
use std::process::exit;

//
// THESE ARE ALL FUNCTIONS RELATED TO THE ASSIGNMENT
// STRUCTURE
//

/* Define 'Assignment' object */
pub struct Assignment {
    pub name: String,
    pub tag: String,
    pub due_time: String,
}

// when I print an Assignment object
//  what happens?
impl fmt::Display for Assignment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "({}, {}, {})", self.name, self.tag, self.due_time)
    }
}

// these are functions related to assignments
impl Assignment {
    // Turn the string due date associated with a task
    // to the DateTime object associated with the chrono function
    // note that we always assume Local timezone.
    pub fn convert_due_date(&self) -> DateTime<Local> {
        let convert_due_date = Local.datetime_from_str(&self.due_time, "%Y-%m-%d %H:%M");
        match convert_due_date {
            Ok(convert_due_date) => convert_due_date,
            _ => panic!("{}", &self.due_time),
        }
    }
}

/* Take all minutes until due from all assignments. Find the
 * largest value. Divide that value by all values.
 * Sum these values. Use that as the denominator for all
 * values. Return this probability distribution.
 */
fn turn_timetilldue_into_pdf(due: Vec<i64>) -> Vec<f64> {
    let mut biggest = 0.0;
    for i in &due {
        if *i as f64 > biggest {
            biggest = *i as f64;
        }
    }
    //println!("{}", biggest);
    //let biggest: f64 = due.iter().max().unwrap();
    //let biggest = 100.0;
    let mut pdf: Vec<f64> = Vec::with_capacity(due.len());
    for i in due {
        pdf.push(biggest / i as f64);
    }
    let sum: f64 = pdf.iter().sum();

    for prob in &mut pdf {
        *prob /= sum;
    }
    pdf
}

/*
 * Get the amount of time until a given assignment is due in minutes
 */
pub fn find_timeuntildue(due_date: DateTime<Local>) -> i64 {
    let curr_local: DateTime<Local> = Local::now();
    let duration = due_date.signed_duration_since(curr_local).num_minutes();
    duration
}

/*
 * Turn a vector containing all assignments, and return a Vec<f64>
 * that is your probability density function for each assignment
 * the index tracks the same assignment
 */
pub fn turn_assignmentvector_into_pdf(assign: &Vec<Assignment>, use_due: bool) -> Vec<f64> {
    if use_due {
        let mut min_till_due: Vec<i64> = Vec::new();
        for i in 0..assign.len() {
            min_till_due.push(find_timeuntildue(assign[i].convert_due_date()));
        }
        turn_timetilldue_into_pdf(min_till_due)
    } else {
        let uniform_prob: f64 = 1.0 / assign.len() as f64;
        return vec![uniform_prob; assign.len()];
    }
}

/*
 * Read in the tasks from the task file path and config tag list
 * Convert these into a hashmap linking each tag to a vector of
 * assignments associated with that tag
 */
pub fn readin_tasks(filepath: &str, tag_list: &Vec<String>) -> HashMap<String, Vec<Assignment>> {
    let lines = lines_from_file(filepath);
    let mut tag_to_taskvectors: HashMap<String, Vec<Assignment>> = HashMap::new();
    for tags in tag_list {
        let task_vector: Vec<Assignment> = Vec::new();
        tag_to_taskvectors.insert(tags.to_string(), task_vector);
    }

    for line in lines {
        let task_vec: Vec<&str> = line.split(",").collect();

        // ignore all lines in todo list that do not have 3
        // fields or that start with '#'
        if task_vec.len() != 3 || task_vec[0].starts_with('#') {
            continue;
        }
        let tag = task_vec[0].trim();
        if !tag_to_taskvectors.contains_key(tag) {
            println!("Tag shown in task list not described in config.");
            panic!("{}", tag);
        };
        let name = task_vec[1].trim();
        let due_date = task_vec[2].trim();
        let new_assign = Assignment {
            name: String::from(name),
            tag: String::from(tag),
            due_time: String::from(due_date),
        };
        //println!("{}", new_assign);
        if find_timeuntildue(new_assign.convert_due_date()) < 0 {
            continue;
        }

        let curr_vector = tag_to_taskvectors.get_mut(tag).unwrap();
        curr_vector.push(new_assign);
    }

    if tag_to_taskvectors.iter().all(|tag| tag.1.is_empty()) {
        eprintln!(
            "The task list is empty, or all tasks in your list are overdue.
Fill the file {} with your tasks.",
            filepath
        );
        exit(1);
    }

    tag_to_taskvectors
}

// convert the hashmap to a vector of strings
pub fn hashmap_to_taskvector(
    tagmap: HashMap<String, Vec<Assignment>>,
    tag_vector: &Vec<String>,
) -> Vec<Vec<String>> {
    let mut toret = vec![];
    for tags in tag_vector {
        let assign_vec = tagmap.get(tags).unwrap();
        for i in 0..assign_vec.len() {
            let mut new = vec![];
            let curr_assign = &assign_vec[i];
            new.push(curr_assign.tag.clone());
            new.push(curr_assign.name.clone());
            new.push(curr_assign.due_time.clone());
            toret.push(new);
        }
    }
    toret
}

pub fn create_weighttable(
    tagmap: &HashMap<String, Vec<Assignment>>,
    tag_vector: &Vec<String>,
    tag_weights: &Vec<f64>,
    use_dues: &Vec<bool>,
) -> Vec<Vec<String>> {
    let mut toret = vec![];
    for i_tags in 0..tag_vector.len() {
        let tags = &tag_vector[i_tags];
        let assign_vec = tagmap.get(tags).unwrap();
        let assign_pdf = turn_assignmentvector_into_pdf(&assign_vec, use_dues[i_tags]);
        for i in 0..assign_vec.len() {
            let mut new = vec![];
            let curr_assign = &assign_vec[i];
            new.push(curr_assign.tag.clone());
            new.push(curr_assign.name.clone());
            new.push(format!("{:.2}%", tag_weights[i_tags] * 100.0));
            new.push(format!("{:.2}%", assign_pdf[i] * 100.0));
            new.push(format!(
                "{:.2}%",
                (assign_pdf[i] * tag_weights[i_tags] * 100.0)
            ));
            toret.push(new);
        }
    }
    // following code to sort by percentage values
    toret.sort_by(|a, b| {
        b[4][..b[4].find("%").unwrap()]
            .parse::<f32>()
            .unwrap()
            .partial_cmp(&a[4][..a[4].find("%").unwrap()].parse::<f32>().unwrap())
            .unwrap()
    });
    toret
}

// convert a given assigment to a string vector with newline characters
pub fn taskvector_to_stringvect(curr_assign: &Assignment) -> Vec<String> {
    let mut toret: Vec<String> = Vec::with_capacity(3);
    let newline = "\n";
    let mut name = curr_assign.name.clone();
    name.push_str(newline);
    let mut tag = curr_assign.tag.clone();
    tag.push_str(newline);
    let mut due_date = curr_assign.due_time.clone();
    due_date.push_str(newline);
    toret.push(tag);
    toret.push(name);
    toret.push(due_date);
    toret
}

/*
 * Convert the vector of tags from the config file to a hashmap
 * linking each tag to a integer counter
 */
pub fn get_tag_counter_hashmap(tag_vector: &Vec<String>) -> HashMap<String, i64> {
    let mut toret: HashMap<String, i64> = HashMap::new();
    for tags in tag_vector {
        toret.insert(tags.to_string(), 0);
    }
    toret
}

/*
 * Convert the task hashmap counter to a vector of string tuples
 * to be displayed.
 */
pub fn convert_hashmap_to_tuplevector(
    x: &HashMap<String, i64>,
    tag: &Vec<String>,
) -> Vec<(String, String)> {
    let mut toret: Vec<(String, String)> = Vec::new();
    for tags in tag {
        let ctr = x.get(tags).unwrap();
        toret.push((tags.to_string(), ctr.to_string()));
    }
    toret
}

pub fn update_tagweights(
    tag_to_vector_map: &HashMap<String, Vec<Assignment>>,
    initial_tag_weights: &Vec<f64>,
    vector_of_tags: &Vec<String>,
) -> Vec<f64> {
    let mut updated_tag_weights = initial_tag_weights.clone();
    let mut xi: f64 = 0.0;
    let mut ctr = 0;

    for (tag, assign_vec) in tag_to_vector_map {
        let tag_idx = vector_of_tags.iter().position(|z| &z == &tag).unwrap();
        let tag_weight = initial_tag_weights[tag_idx];
        if assign_vec.len() == 0 || tag_weight == 0.0 {
            xi += tag_weight;
            updated_tag_weights[tag_idx] = 0.0;
        } else {
            ctr += 1
        }
    }
    let to_add = xi / ctr as f64;
    for i in 0..vector_of_tags.len() {
        if updated_tag_weights[i] != 0.0 {
            updated_tag_weights[i] += to_add;
        }
    }
    updated_tag_weights
}
