#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate apic;
extern crate task;
extern crate runqueue;
extern crate scheduler_priority;


use core::ops::DerefMut;
use irq_safety::{disable_interrupts};
use apic::get_my_apic_id;
use task::{Task, TaskRef, get_my_current_task};
use runqueue::{RunQueue, RunQueue_trait};



/// This function performs a task switch.
///
/// Interrupts will be disabled while this function runs.
pub fn schedule() -> bool {
    disable_interrupts();

    // let current_taskid: TaskId = CURRENT_TASK.load(Ordering::SeqCst);
    // trace!("schedule [0]: current_taskid={}", current_taskid);

    let current_task: *mut Task;
    let next_task: *mut Task; 

    let apic_id = match get_my_apic_id() {
        Some(id) => id,
        _ => {
            error!("BUG: Couldn't get apic_id in schedule()");
            return false;
        }
    };

    
    
    // same scoping reasons as above: to release the lock around current_task
    {
        current_task = get_my_current_task().expect("schedule(): get_my_current_task() failed")
                                            .lock_mut().deref_mut() as *mut Task; 
    }


    //if current_task == next_task {
        // no need to switch if the chosen task is the same as the current task
    //    return false;
    //}

    // we want mutable references to mutable tasks
    let curr = unsafe {&mut *current_task };

    let curr_priority = curr.priority.unwrap() as u32;

    //We update the runtime of the current running task
    //Ideally this should be curr.weighted_runtime = curr.weighted_runtime + timeslice / (curr_priority);
    //Since timeslice is constant we replaced it with a constant
    curr.weighted_runtime = curr.weighted_runtime + 1000 / (curr_priority + 1);

    if let Some(selected_next_task) = scheduler_priority::select_next_task(apic_id) {
        next_task = selected_next_task.lock_mut().deref_mut();  // as *mut Task;
    }
    else {
        // keep running the same current task
        return false;
    }

    if next_task as usize == 0 {
        // keep the same current task
        return false;
    }

    if current_task == next_task {
        // no need to switch if the chosen task is the same as the current task
        return false;
    }

    // trace!("BEFORE TASK_SWITCH CALL (current={}), interrupts are {}", current_taskid, ::interrupts::interrupts_enabled());

    let (next) = unsafe {&mut *next_task};

    //We update the minimum weighted run time based on the task we picked
    //If minimum weighted run time of run queue is less than weighted run time of next task we update the minimum weighted run time of run queue
    //If weighted run time of next task is less than minimum weighted runtime of runqueue then we update the weighted runtime of next task
    //This is to prevent a task blocked for long period hogging the core 
    {
        let mut runqueue_locked = match RunQueue::get_runqueue(apic_id) {
            Some(rq) => rq.write(),
            _ => {
                debug!("BUG: schedule(): couldn't get runqueue for core {}", apic_id); 
                return false;
            }
        };
        let runqueue_weighted_min_runtime = runqueue_locked.get_weighted_min_runtime();
        if(runqueue_weighted_min_runtime < next.weighted_runtime){
            runqueue_locked.update_weighted_min_runtime(next.weighted_runtime);
        }
        else{
            next.weighted_runtime = runqueue_weighted_min_runtime;
        }

    }

    //We update the number of context switches
    next.times_picked = next.times_picked + 1;

    curr.task_switch(next, apic_id); 

    // let new_current: TaskId = CURRENT_TASK.load(Ordering::SeqCst);
    // trace!("AFTER TASK_SWITCH CALL (current={}), interrupts are {}", new_current, ::interrupts::interrupts_enabled());
 
    true
}
