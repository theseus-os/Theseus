#![no_std]

extern crate alloc;

use core::convert::TryInto;
use alloc::{string::String, vec::Vec};
use app_io::println;
use time::{now, Monotonic};
use cpu::current_cpu;

pub fn main(args: Vec<String>) -> isize {
    let mut options = getopts::Options::new();
    options
        .optflag("h", "help", "Display this message")
        .optopt("c", "cpu", "Spawn all tasks on CPU with ID <cpu>", "<cpu>")
        .optopt("t", "tasks", "Spawn <num> tasks", "<num>")
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

    let cpu_id: Option<u32> = matches.opt_get("c")
        .expect("failed to parse the CPU ID");
    let cpu = cpu_id.map(|id| id.try_into())
        .expect("CPU ID did not correspond to an existing CPU");
    let cpu = cpu.unwrap_or_else(|_| current_cpu());

    let num_tasks = matches
        .opt_get_default("t", 32)
        .expect("failed to parse the number of tasks");
    let num_yields = matches
        .opt_get_default("y", 16384)
        .expect("failed to parse the number of yields");

    let mut tasks = Vec::with_capacity(num_tasks);
    for _ in 0..num_tasks {
        tasks.push(
            spawn::new_task_builder(worker, num_yields)
                // Currently, if the tasks aren't pinned to a core, the workers on the same core as
                // the shell finish significantly slower. The majority of the runtime is taken up by
                // the shell rather than by our workers, invalidating the benchmark. To fix this we
                // pin the workers on core 3, assuming the shell is on some other core. This means
                // the benchmark doesn't incorporate work stealing, but the only reason we're having
                // this problem in the first place is because work stealing isn't implemented so...
                // TODO: Remove this when work stealing is implemented.
                .pin_on_cpu(cpu)
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
        task::schedule();
    }
}
