#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate apic;
extern crate task;


use core::ops::DerefMut;
use irq_safety::{disable_interrupts};
use task::{Task, TaskRef, get_my_current_task, RunQueue};
use apic::get_my_apic_id;


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

    if let Some(selected_next_task) = select_next_task(apic_id) {
        next_task = selected_next_task.write().deref_mut();  // as *mut Task;
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
                                            .write().deref_mut() as *mut Task; 
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



/// this defines the scheduler policy.
/// returns None if there is no schedule-able task
fn select_next_task(apic_id: u8) -> Option<TaskRef>  {

    let mut runqueue_locked = match RunQueue::get_runqueue(apic_id) {
        Some(rq) => rq.write(),
        _ => {
            error!("BUG: select_next_task(): couldn't get runqueue for core {}", apic_id); 
            return None;
        }
    };
    
    let mut idle_task_index: Option<usize> = None;
    let mut chosen_task_index: Option<usize> = None;

    for (i, task) in runqueue_locked.iter().enumerate() {
        let t = task.read();

        // we skip the idle task, and only choose it if no other tasks are runnable
        if t.is_an_idle_task() {
            idle_task_index = Some(i);
            continue;
        }

        // must be runnable
        if !t.is_runnable() {
            continue;
        }

        // if this task is pinned, it must not be pinned to a different core
        if let Some(pinned) = t.pinned_core {
            if pinned != apic_id {
                // with per-core runqueues, this should never happen!
                error!("select_next_task() (AP {}) found a task pinned to a different core: {:?}", apic_id, *t);
                return None;
            }
        }
            
        // found a runnable task!
        chosen_task_index = Some(i);
        // debug!("select_next_task(): AP {} chose Task {:?}", apic_id, *t);
        break; 
    }

    // idle task is a backup iff no other task has been chosen
    chosen_task_index
        .or(idle_task_index)
        .and_then(|index| runqueue_locked.move_to_end(index))
}
