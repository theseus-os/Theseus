#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate apic;
extern crate task;
extern crate runqueue;
extern crate scheduler_round_robin;


use core::ops::DerefMut;
use irq_safety::{disable_interrupts};
use apic::get_my_apic_id;
use task::{Task, get_my_current_task};



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
    
    // same scoping reasons as above: to release the lock around current_task
    {
        current_task = get_my_current_task().expect("schedule(): get_my_current_task() failed")
                                            .lock_mut().deref_mut() as *mut Task; 
    }

    if current_task == next_task {
        // no need to switch if the chosen task is the same as the current task
        return false;
    }

    // we want mutable references to mutable tasks
    let (curr, next) = unsafe { (&mut *current_task, &mut *next_task) };

    // trace!("BEFORE TASK_SWITCH CALL (current={}), interrupts are {}", current_taskid, ::interrupts::interrupts_enabled());

    curr.task_switch(next, apic_id); 

    // let new_current: TaskId = CURRENT_TASK.load(Ordering::SeqCst);
    // trace!("AFTER TASK_SWITCH CALL (current={}), interrupts are {}", new_current, ::interrupts::interrupts_enabled());
 
    true
}

