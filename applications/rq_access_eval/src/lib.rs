#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use app_io::println;
use cpu::CpuId;
use scheduler::{RunqueueTrait, SchedulerPolicy};
use time::{now, Duration, Monotonic};

pub fn main(args: Vec<String>) -> isize {
    let guard = irq_safety::hold_interrupts();
    let mut options = getopts::Options::new();
    options
        .optflag("h", "help", "Display this message")
        .optflag("l", "least-busy", "Get the least busy CPU")
        .optopt("c", "core", "Get <CPU>'s runqueue", "<CPU>")
        .optopt("n", "num", "Perform <NUM> iterations", "<NUM>");

    let matches = match options.parse(args) {
        Ok(matches) => matches,
        Err(e) => {
            println!("{}", e);
            print_usage(options);
            return 1;
        }
    };

    let least_busy = matches.opt_present("l");
    let cpu = matches.opt_get::<u32>("c").expect("failed to parse CPU");

    if least_busy && cpu.is_some() {
        panic!("both the least-busy and CPU flags can't be specified");
    }

    let num = matches
        .opt_get_default("n", 1_000_000)
        .expect("failed to parse num");

    let duration = if least_busy {
        run(
            |_| {
                scheduler::with_scheduler(|sched| {
                    let rq = sched.runqueue_iter()
                        .min_by(|x, y| x.len().cmp(&y.len()));
                    drop(rq);
                });
            },
            num,
        )
    } else if let Some(cpu) = cpu {
        let cpu_id = CpuId::try_from(cpu).expect("specified CPU did not exist");
        run(
            |_| {
                scheduler::with_scheduler(|sched| {
                    let rq = sched.get_runqueue(cpu_id.into());
                    drop(rq);
                });
            },
            num,
        )
    } else {
        let cpu_count = cpu::cpu_count();
        run(
            |count| {
                scheduler::with_scheduler(|sched| {
                    let rq = sched.get_runqueue(
                    CpuId::try_from(count % cpu_count)
                        .expect("CPU IDs aren't sequential")
                        .into()
                    );
                    drop(rq);
                });
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
