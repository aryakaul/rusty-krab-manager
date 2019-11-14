mod rand_utils;
mod assignment_utils;

fn main() {
    let mut task_list: Vec<assignment_utils::Assignment> = Vec::new();
    let bmif_assign = assignment_utils::Assignment {
        name: String::from("BMIF Problem Set"),
        tag: String::from("school"),
        due_time: String::from("2019-11-12 23:59"),
        complete: false,
    };
    task_list.push(bmif_assign);
    let gen_assign = assignment_utils::Assignment {
        name: String::from("Genetics Problem Set"),
        tag: String::from("school"),
        due_time: String::from("2019-11-19 09:00"),
        complete: false,
    };
    task_list.push(gen_assign);
    let x = assignment_utils::turn_assignmentvector_into_pdf(task_list, true);
    println!("{}", rand_utils::roll_die(x));

}
