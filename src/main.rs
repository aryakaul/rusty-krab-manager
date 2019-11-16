mod assignment_utils;
use assignment_utils::{readin_tasks,turn_assignmentvector_into_pdf};
mod rand_utils;

fn main() {
    let x = readin_tasks("/home/luak/projects/git/rusty-manager/example/tasks");
    for (tag,vect) in &x {
        println!("{}", tag);
        for y in vect {
            println!("{}", y);
        }
        let task_list = turn_assignmentvector_into_pdf(vect, true);
        println!("{}", rand_utils::roll_die(task_list));
    }
}
