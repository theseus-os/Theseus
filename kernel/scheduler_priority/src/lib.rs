#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate apic;
extern crate task;
extern crate runqueue;
extern crate runqueue_priority;


use core::ops::DerefMut;
use irq_safety::{disable_interrupts};
use apic::get_my_apic_id;
use task::{Task, TaskRef, get_my_current_task};
use runqueue::RunQueueTrait;
use runqueue_priority::RunQueue;

struct next_task_result{
    taskref : Option<TaskRef>,
    idle_task : bool,
}



/// this defines the priority scheduler policy.
/// returns None if there is no schedule-able task
pub fn select_next_task(apic_id: u8) -> Option<TaskRef>  {
    let taskref_with_result = select_next_task_p2(apic_id); 
    match taskref_with_result {
        // The division was valid
        Some(x) => {
            //debug!("Some coe running: {}", apic_id);
            
            if(x.idle_task == true){
                assign_weights(apic_id);
                select_next_task_p2(apic_id).and_then(|m| m.taskref)
            }
            else {
                x.taskref
            }
        }
        // The division was invalid
        None    => {
            assign_weights(apic_id);
            select_next_task_p2(apic_id).and_then(|m| m.taskref)
        }
    }
    //taskref_with_result.and_then(|m| m.taskref)
}

/// this defines the round robin scheduler policy.
/// returns None if there is no schedule-able task
fn select_next_task_p2(apic_id: u8) -> Option<next_task_result>  {

    let mut runqueue_locked = match RunQueue::get_runqueue(apic_id) {
        Some(rq) => rq.write(),
        _ => {
            error!("BUG: select_next_task(): couldn't get runqueue for core {}", apic_id); 
            return None;
        }
    };
    
    let mut idle_task_index: Option<usize> = None;
    let mut chosen_task_index: Option<usize> = None;
    let mut idle_task = true;

    for (i, taskref) in runqueue_locked.iter().enumerate() {
        let t = taskref.lock();

        // we skip the idle task, and only choose it if no other tasks are runnable
        if t.is_an_idle_task {
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

        if taskref.get_weight() == 0{
            continue;
        }
            
        // found a runnable task!
        chosen_task_index = Some(i);
        idle_task = false;
        // debug!("select_next_task(): AP {} chose Task {:?}", apic_id, *t);
        break; 
    }

    // idle task is a backup iff no other task has been chosen
    // chosen_task_index
    //     .or(idle_task_index)
    //     .and_then(|index| runqueue_locked.move_to_end(index))

    let modified_weight = {
        let chosen_task = chosen_task_index.and_then(|index| runqueue_locked.get_priority_task_ref(index));
        chosen_task.map(|m| m.get_weight()).unwrap_or(1) - 1
    };

    chosen_task_index
        .or(idle_task_index)
        .and_then(|index| runqueue_locked.update_and_move_to_end(index, modified_weight))
        .map(|index| next_task_result {
            taskref : Some(index),
            idle_task  : idle_task, 
        })
}


/// This defines the priority scheduler policy.
/// Task with the minimum weighted run time is picked
/// Returns None if there is no schedule-able task
fn assign_weights(apic_id: u8) -> bool  {

    let mut runqueue_locked = match RunQueue::get_runqueue(apic_id) {
        Some(rq) => rq.write(),
        _ => {
            error!("BUG: select_next_task(): couldn't get runqueue for core {}", apic_id); 
            return false;
        }
    };
    
    let mut idle_task_index: Option<usize> = None;
    let mut chosen_task_index: Option<usize> = None;
    let mut total_priorities = 1;

    for (i, taskref) in runqueue_locked.iter().enumerate() {
        let t = taskref.lock();

        // we skip the idle task, and only choose it if no other tasks are runnable
        if t.is_an_idle_task {
            idle_task_index = Some(i);
            continue;
        }

        // must be runnable
        if !t.is_runnable() {
            continue;
        }

        //if let Some(priority) = t.priority {
        //	if priority < -1 {
        //        continue;
        //    }
        //}

        // if this task is pinned, it must not be pinned to a different core
        if let Some(pinned) = t.pinned_core {
            if pinned != apic_id {
                // with per-core runqueues, this should never happen!
                error!("select_next_task() (AP {}) found a task pinned to a different core: {:?}", apic_id, *t);
                return false;
            }
        }
            
        // found a runnable task!
        // Mark it as the running task if it has he minimum weighted run time
        total_priorities = total_priorities + t.priority.unwrap_or(0) as u32;
        
        
        
        //debug!("select_next_task(): AP {} chose Task {:?}", apic_id, *t);
        //break; 
    }
    if(apic_id == 1){
        debug!("total priorities(): AP {} priorities {}", apic_id, total_priorities);
    }
    //for (i, taskref) in runqueue_locked.iter().enumerate() {

    let epcoh = if total_priorities < 100 {
                100
            }
            else {
                total_priorities
            };

    let mut i = 0;
    let mut len = 0;
    {
        len = runqueue_locked.runqueue_length();
    }

    while i < len {
        let mut task_weight = 1;
        {
            let taskref = runqueue_locked.get_priority_task_ref(i).unwrap();
            let t = taskref.lock();

            // we skip the idle task, and only choose it if no other tasks are runnable
            if t.is_an_idle_task {
                idle_task_index = Some(i);
                i = i+1;
                continue;
            }

            // must be runnable
            if !t.is_runnable() {
                i = i+1;
                continue;
            }

            //if let Some(priority) = t.priority {
            //	if priority < -1 {
            //        continue;
            //    }
            //}

            // if this task is pinned, it must not be pinned to a different core
            if let Some(pinned) = t.pinned_core {
                if pinned != apic_id {
                    // with per-core runqueues, this should never happen!
                    error!("select_next_task() (AP {}) found a task pinned to a different core: {:?}", apic_id, *t);
                    return false;
                }
            }
            task_weight = 100 * t.priority.unwrap() as u32 / total_priorities;
        }
        // found a runnable task!
        // Mark it as the running task if it has he minimum weighted run time
        
        {
            let mut mut_task = runqueue_locked.get_priority_task_ref_as_mut(i);
            mut_task.map(|m| m.update_weight(task_weight));
            //mut_task.map(|m| m.update_weight(1000));
        }
        
        i = i+1;
        //debug!("select_next_task(): AP {} chose Task {:?}", apic_id, *t);
        //break; 
    }

    return true;
}