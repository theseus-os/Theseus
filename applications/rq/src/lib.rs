#![no_std]

extern crate alloc;

use alloc::{
    fmt::Write,
    string::{String, ToString},
    vec::Vec,
};

use app_io::{print, println};
use getopts::Options;

pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");

    let matches = match opts.parse(args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{} \n", _f);
            return -1;
        }
    };

    if matches.opt_present("h") {
        return print_usage(opts);
    }
    let bootstrap_cpu = cpu::bootstrap_cpu();

    for (cpu, task_list) in task::scheduler::tasks() {
        let core_type = if Some(cpu) == bootstrap_cpu {
            "Boot CPU"
        } else {
            "Secondary CPU"
        };

        println!("\n{} (CPU: {})", core_type, cpu);

        let mut runqueue_contents = String::new();
        for task in task_list.iter() {
            writeln!(
                runqueue_contents,
                "    {} ({}) {}",
                task.name,
                task.id,
                if task.is_running() { "*" } else { "" }
            )
            .expect("Failed to write to runqueue_contents");
        }
        print!("{}", runqueue_contents);
    }

    println!("");
    0
}

fn print_usage(opts: Options) -> isize {
    let mut brief = "Usage: rq \n \n".to_string();

    brief.push_str(
        "Prints each CPU's ID, the tasks on its runqueue ('*' identifies the currently running \
         task), and whether it is the boot CPU or not",
    );

    println!("{} \n", opts.usage(&brief));

    0
}
