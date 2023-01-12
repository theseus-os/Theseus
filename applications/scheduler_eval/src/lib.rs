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
        .opt_get_default("t", 32)
        .expect("failed to parse the number of threads");
    let num_yields = matches
        .opt_get_default("y", 16384)
        .expect("failed to parse the number of yields");

    let mut tasks = Vec::with_capacity(num_threads);
    for _ in 0..num_threads {
        tasks.push(
            spawn::new_task_builder(worker, num_yields)
                // Currently, if the tasks aren't pinned to a core, the workers on the same core as
                // the shell finish significantly slower. The majority of the runtime is taken up by
                // the shell rather than by our workers, invalidating the benchmark. To fix this we
                // pin the workers on core 3, assuming the shell is on some other core. This means
                // the benchmark doesn't incorporate work stealing, but the only reason we're having
                // this problem in the first place is because work stealing isn't implemented so...
                // TODO: Remove this when work stealing is implemented.
                .pin_on_core(3)
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
        // JoinableTaskRef::join is inlined so that we can yield if the worker hasn't
        // exited minimising the impact our task has on the worker tasks.
        // TODO: Call join directly once it is properly implemented.
        // TODO: Remove dependency on scheduler.
        while !task.has_exited() {
            scheduler::schedule();
        }

        while task.is_running() {
            scheduler::schedule();
        }

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
