#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use app_io::println;
use time::{now, Duration, Monotonic};

pub fn main(args: Vec<String>) -> isize {
    let guard = irq_safety::hold_interrupts();
    let mut options = getopts::Options::new();
    options
        .optflag("h", "help", "Display this message")
        .optflag("l", "least-busy", "Get the least busy core")
        .optopt("c", "core", "Get <core>'s runqueue", "<core>")
        .optopt("n", "num", "Perform <num> iterations", "<num>");

    let matches = match options.parse(args) {
        Ok(matches) => matches,
        Err(e) => {
            println!("{}", e);
            print_usage(options);
            return 1;
        }
    };

    let least_busy = matches.opt_present("l");
    let core = matches.opt_get::<u8>("c").expect("failed to parse core");

    if least_busy && core.is_some() {
        panic!("both the least-busy and core flags can't be specified");
    }

    let num = matches
        .opt_get_default("n", 1_000_000)
        .expect("failed to parse num");

    let duration = if least_busy {
        run(
            |_| {
                runqueue::get_least_busy_core();
            },
            num,
        )
    } else if let Some(core) = core {
        run(
            |_| {
                runqueue::get_runqueue(core);
            },
            num,
        )
    } else {
        let cpu_count = cpu::cpu_count();
        run(
            |count| {
                runqueue::get_runqueue((count % cpu_count) as u8);
            },
            num,
        )
    };
    drop(guard);

    println!("time: {:#?}", duration);

    0
}

fn run(f: impl Fn(u32), num: u32) -> Duration {
    let start = now::<Monotonic>();
    for i in 0..num {
        f(i);
    }
    now::<Monotonic>().duration_since(start)
}

fn print_usage(options: getopts::Options) {
    let brief = alloc::format!("Usage: {} [OPTIONS]", env!("CARGO_CRATE_NAME"));
    println!("{}", options.usage(&brief));
}
