#![no_std]

extern crate alloc;

use core::{
    convert::TryInto,
    sync::atomic::{AtomicU64, Ordering},
};
use alloc::{string::String, sync::Arc, vec::Vec};
use app_io::println;
use time::{now, Instant, Monotonic};
use cpu::{current_cpu, CpuId};

pub fn main(args: Vec<String>) -> isize {
    let mut options = getopts::Options::new();
    options
        .optflag("h", "help", "Display this message")
        .optopt("c", "cpu", "Spawn all tasks on CPU with ID <cpu>", "<cpu>")
        .optopt("t", "tasks", "Spawn <num> tasks", "<num>")
        .optopt("y", "yield", "Yield <num> times in each thread", "<num>")
        .optflag(
            "m",
            "mixed",
            "Run the mixed CPU-bound/interactive workload benchmark instead of the plain \
             yield benchmark. This is the mode that distinguishes scheduling policies like \
             MLFQ, which are meant to favour interactive tasks over CPU-bound ones, from \
             policies like round-robin, which treat every task identically.",
        )
        .optopt(
            "",
            "cpu-bound",
            "Number of CPU-bound tasks to spawn in mixed mode",
            "<num>",
        )
        .optopt(
            "i",
            "interactive",
            "Number of interactive tasks to spawn in mixed mode",
            "<num>",
        )
        .optopt(
            "",
            "cpu-iterations",
            "Work units each CPU-bound task performs in one uninterrupted burst in mixed mode",
            "<num>",
        )
        .optopt(
            "",
            "bursts",
            "Number of work/yield bursts each interactive task performs in mixed mode",
            "<num>",
        )
        .optopt(
            "",
            "work-per-burst",
            "Work units each interactive task performs per burst, before yielding, in mixed mode",
            "<num>",
        );

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

    if matches.opt_present("m") {
        let cpu_bound = matches
            .opt_get_default("cpu-bound", 4)
            .expect("failed to parse --cpu-bound");
        let interactive = matches
            .opt_get_default("i", 16)
            .expect("failed to parse --interactive");
        let cpu_iterations = matches
            .opt_get_default("cpu-iterations", 20_000_000)
            .expect("failed to parse --cpu-iterations");
        let bursts = matches
            .opt_get_default("bursts", 200)
            .expect("failed to parse --bursts");
        let work_per_burst = matches
            .opt_get_default("work-per-burst", 20_000)
            .expect("failed to parse --work-per-burst");

        run_mixed(cpu_bound, interactive, cpu_iterations, bursts, work_per_burst, cpu);
        return 0;
    }

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

/// Runs a mixed workload of CPU-bound and interactive tasks and reports each
/// group's completion-latency distribution separately.
///
/// This is the benchmark that actually differentiates scheduling policies:
/// a scheduler that favours interactive tasks (like MLFQ, which doesn't
/// demote a task that yields before its quantum is exhausted) should show
/// meaningfully lower interactive-task latency than round-robin under the
/// same CPU-bound load, without the CPU-bound tasks' total throughput
/// collapsing.
///
/// Run the same command under different scheduler policies to compare them,
/// e.g.:
/// ```sh
/// make run THESEUS_CONFIG=mlfq_scheduler
/// # in the Theseus shell:
/// scheduler_eval -m --cpu-bound 4 --interactive 16
/// ```
pub fn run_mixed(
    cpu_bound: usize,
    interactive: usize,
    cpu_iterations: usize,
    bursts: usize,
    work_per_burst: usize,
    cpu: CpuId,
) {
    let cpu_results: Vec<Arc<AtomicU64>> = (0..cpu_bound).map(|_| Arc::new(AtomicU64::new(0))).collect();
    let interactive_results: Vec<Arc<AtomicU64>> =
        (0..interactive).map(|_| Arc::new(AtomicU64::new(0))).collect();

    // Captured before any task is spawned so it can be handed to every task
    // as a common reference point; each task records its own completion time
    // relative to this instant, giving us a per-task latency rather than
    // just an aggregate makespan.
    let start = now::<Monotonic>();

    let mut cpu_tasks = Vec::with_capacity(cpu_bound);
    for result in cpu_results.iter().cloned() {
        cpu_tasks.push(
            spawn::new_task_builder(cpu_bound_worker, (start, result, cpu_iterations))
                .pin_on_cpu(cpu)
                .block()
                .spawn()
                .expect("failed to spawn CPU-bound task"),
        );
    }

    let mut interactive_tasks = Vec::with_capacity(interactive);
    for result in interactive_results.iter().cloned() {
        interactive_tasks.push(
            spawn::new_task_builder(interactive_worker, (start, result, bursts, work_per_burst))
                .pin_on_cpu(cpu)
                .block()
                .spawn()
                .expect("failed to spawn interactive task"),
        );
    }

    for task in cpu_tasks.iter().chain(interactive_tasks.iter()) {
        task.unblock().expect("failed to unblock task");
    }

    for task in cpu_tasks.into_iter().chain(interactive_tasks) {
        // See the comment on the equivalent loop above: this inlines `join`
        // so we yield rather than busy-wait while workers finish.
        while !task.has_exited() {
            scheduler::schedule();
        }
        while task.is_running() {
            scheduler::schedule();
        }
        task.join().expect("failed to join task");
    }

    let makespan = now::<Monotonic>() - start;

    print_stats("cpu-bound", &cpu_results);
    print_stats("interactive", &interactive_results);
    println!("makespan: {makespan:#?}");
    log::info!("scheduler_eval mixed-mode makespan: {makespan:#?}");
}

/// Busy-works for `iterations` steps without ever yielding voluntarily, so
/// it only gives up the CPU when preempted. Under MLFQ this is exactly the
/// behaviour that gets a task demoted to a lower priority level.
fn cpu_bound_worker((start, result, iterations): (Instant, Arc<AtomicU64>, usize)) {
    let mut acc: u64 = 0;
    for i in 0..iterations {
        acc = core::hint::black_box(acc.wrapping_add(i as u64).wrapping_mul(2_654_435_761));
    }
    core::hint::black_box(acc);
    record_latency(start, &result);
}

/// Performs `bursts` short bursts of work, voluntarily yielding after each
/// one, simulating a task that's frequently blocked on I/O. Under MLFQ, a
/// task that yields before its quantum is exhausted keeps its priority
/// level instead of being demoted, which is what should make this worker's
/// completion latency stay low even under heavy CPU-bound competition.
fn interactive_worker((start, result, bursts, work_per_burst): (Instant, Arc<AtomicU64>, usize, usize)) {
    for _ in 0..bursts {
        let mut acc: u64 = 0;
        for i in 0..work_per_burst {
            acc = core::hint::black_box(acc.wrapping_add(i as u64));
        }
        core::hint::black_box(acc);
        scheduler::schedule();
    }
    record_latency(start, &result);
}

fn record_latency(start: Instant, result: &AtomicU64) {
    let elapsed = (now::<Monotonic>() - start).as_micros() as u64;
    result.store(elapsed, Ordering::Release);
}

fn print_stats(label: &str, results: &[Arc<AtomicU64>]) {
    if results.is_empty() {
        return;
    }

    let mut latencies_us: Vec<u64> = results.iter().map(|r| r.load(Ordering::Acquire)).collect();
    latencies_us.sort_unstable();

    let n = latencies_us.len();
    let sum: u64 = latencies_us.iter().sum();
    let avg = sum / n as u64;
    let min = latencies_us[0];
    let max = latencies_us[n - 1];
    let p50 = latencies_us[n / 2];

    let line = alloc::format!(
        "{label:<12} n={n:<4} avg={avg:>9}us  p50={p50:>9}us  min={min:>9}us  max={max:>9}us"
    );
    println!("{line}");
    // Also emit via the `log` crate so results are visible in the serial
    // debug log even when there's no interactive console attached (e.g.
    // when driven headlessly via `bench_headless`).
    log::info!("scheduler_eval: {line}");
}
