#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use app_io::println;
use time::{now, Monotonic};

pub fn main(args: Vec<String>) -> isize {
    let mut options = getopts::Options::new();
    options
        .optflag("h", "help", "Display this message")
        .optopt("t", "threads", "Spawn <num> threads", "<num>")
        .optopt("y", "yield", "Yield <num> times in each thread", "<num>");

    let matches = match options.parse(args) {
        Ok(matches) => matches,
        Err(e) => {
            println!("{}", e);
            print_usage(options);
            return 1;
        }
    };

    if matches.opt_present("h") {
        print_usage(options);
        return 0;
    }

    let num_threads = matches
        .opt_get_default("t", 8)
        .expect("failed to parse the number of threads");
    let num_yields = matches
        .opt_get_default("y", 512)
        .expect("failed to parse the number of yields");

    let mut tasks = Vec::with_capacity(num_threads);
    for _ in 0..num_threads {
        tasks.push(
            spawn::new_task_builder(worker, num_yields)
                .block()
                .spawn()
                .expect("failed to spawn task"),
        );
    }

    let start = now::<Monotonic>();
    for task in tasks.iter() {
        task.unblock().expect("failed to unblock task");
    }

    for task in tasks {
        task.join().expect("failed to join task");
    }
    let end = now::<Monotonic>();

    println!("time: {:#?}", end - start);

    0
}

fn print_usage(options: getopts::Options) {
    let brief = alloc::format!("Usage: {} [OPTIONS]", env!("CARGO_CRATE_NAME"));
    println!("{}", options.usage(&brief));
}

fn worker(num_yields: u32) {
    for _ in 0..num_yields {
        scheduler::schedule();
    }
}
