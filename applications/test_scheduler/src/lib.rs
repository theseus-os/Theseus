#![no_std]

extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use app_io::println;
use cpu::{cpus, CpuId};
use rand::seq::SliceRandom;
use sync_block::RwLock;
use task::TaskRef;

pub fn main(_args: Vec<String>) -> isize {
    println!("testing pinned");
    test_pinned();
    println!("testing unpinned");
    test_unpinned();
    0
}

// Spawn a bunch of pinned tasks, and then each pinned task randomly blocks and
// unblocks other tasks than are pinned to the same CPU.
//
// The tasks must be pinned to the same CPU to avoid a deadlock where two tasks
// on different CPUs block each other at the same time and then yield.
pub fn test_pinned() {
    static TASKS: RwLock<Vec<(CpuId, Vec<TaskRef>)>> = RwLock::new(Vec::new());
    static READY: AtomicBool = AtomicBool::new(false);

    let tasks = cpus()
        .map(|cpu| {
            (
                cpu.clone(),
                (0..100)
                    .map(move |id| {
                        spawn::new_task_builder(pinned_worker, cpu)
                            .name(format!("test-scheduler-pinned-{cpu}-{id}"))
                            .pin_on_cpu(cpu)
                            .block()
                            .spawn()
                            .expect("failed to spawn task")
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();

    *TASKS.write() = tasks
        .iter()
        .map(|(cpu, task_iter)| (*cpu, task_iter.iter().map(|task| (*task).clone()).collect()))
        .collect();

    for (_, task_list) in tasks.iter() {
        for task in task_list {
            task.unblock().unwrap();
        }
    }

    READY.store(true, Ordering::Release);

    for (_, task_list) in tasks {
        for task in task_list {
            task.join().unwrap();
        }
    }

    // We have to drop the tasks so that the `test-scheduler` crate can be dropped.
    *TASKS.write() = Vec::new();

    fn pinned_worker(pinned_cpu: CpuId) {
        let mut rng = random::init_rng::<rand::rngs::SmallRng>().unwrap();
        while !READY.load(Ordering::Acquire) {}

        let locked = TASKS.read();
        let tasks = &locked.iter().find(|(cpu, _)| *cpu == pinned_cpu).unwrap().1;
        for _ in 0..100 {
            assert_eq!(
                cpu::current_cpu(),
                pinned_cpu,
                "pinned worker migrated cores"
            );

            let random_task = tasks.choose(&mut rng).unwrap();

            let chose_self =
                task::with_current_task(|current_task| random_task == current_task).unwrap();
            if chose_self {
                continue;
            }

            let _ = random_task.block_no_log();
            task::schedule();
            let _ = random_task.unblock_no_log();
        }
    }
}

/// Spawn a bunch of unpinned tasks, and then block and unblock random tasks
/// from the main thread.
pub fn test_unpinned() {
    const NUM_TASKS: usize = 500;

    static READY: AtomicBool = AtomicBool::new(false);
    static NUM_RUNNING: AtomicUsize = AtomicUsize::new(NUM_TASKS);

    let tasks = (0..NUM_TASKS)
        .map(move |id| {
            spawn::new_task_builder(unpinned_worker, ())
                .name(format!("test-scheduler-unpinned-{id}"))
                .block()
                .spawn()
                .expect("failed to spawn task")
        })
        .collect::<Vec<_>>();

    for task in tasks.iter() {
        task.unblock().unwrap();
    }

    READY.store(true, Ordering::Release);

    // Cause some mayhem.
    let mut rng = random::init_rng::<rand::rngs::SmallRng>().unwrap();
    while NUM_RUNNING.load(Ordering::Relaxed) != 0 {
        let random_task = tasks.choose(&mut rng).unwrap();
        let _ = random_task.block_no_log();
        // Let the worker tasks on this core run.
        task::schedule();
        let _ = random_task.unblock_no_log();
    }

    for task in tasks {
        task.join().unwrap();
    }

    fn unpinned_worker(_: ()) {
        while !READY.load(Ordering::Acquire) {}

        for _ in 0..1000 {
            task::schedule();
        }

        NUM_RUNNING.fetch_sub(1, Ordering::Relaxed);
    }
}
