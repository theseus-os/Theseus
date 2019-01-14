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



/// this defines the priority scheduler policy.
/// returns None if there is no schedule-able task
pub fn select_next_task(apic_id: u8) -> Option<TaskRef>  {
    select_next_task_priority(apic_id)
}



/// This defines the priority scheduler policy.
/// Task with the minimum weighted run time is picked
/// Returns None if there is no schedule-able task
fn select_next_task_priority(apic_id: u8) -> Option<TaskRef>  {

    let mut runqueue_locked = match RunQueue::get_runqueue(apic_id) {
        Some(rq) => rq.write(),
        _ => {
            error!("BUG: select_next_task(): couldn't get runqueue for core {}", apic_id); 
            return None;
        }
    };
    
    let mut idle_task_index: Option<usize> = None;
    let mut chosen_task_index: Option<usize> = None;
    let mut minimum_weighted_run_time = 0;

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
                return None;
            }
        }
            
        // found a runnable task!
        // Mark it as the running task if it has he minimum weighted run time
        chosen_task_index = match chosen_task_index{
        	None => {
        		minimum_weighted_run_time = taskref.get_weight();
        		Some(i)
        	},
        	Some(chosen_task_index) => {
        		if(taskref.get_weight() < minimum_weighted_run_time){
        			minimum_weighted_run_time = taskref.get_weight();
        			Some(i)
        		}
        		else{
        			Some(chosen_task_index) 
        		}
 
        	}
        }
        
        //debug!("select_next_task(): AP {} chose Task {:?}", apic_id, *t);
        //break; 
    }

    // idle task is a backup iff no other task has been chosen
    
    
    let modified_weight = {
        let chosen_task = chosen_task_index.and_then(|index| runqueue_locked.get_priority_task_ref(index));
        let task_priority = chosen_task.map(|m| m.lock().priority.unwrap() as u32);
        chosen_task.map(|m| m.get_weight()).unwrap_or(0) + 1000 / (task_priority.unwrap_or(0) + 1)
    };

    chosen_task_index
        .or(idle_task_index)
        .and_then(|index| runqueue_locked.update_and_move_to_end(index, modified_weight))

    // chosen_task_index
    //     .or(idle_task_index)
    //     .and_then(|index| {
    //         let modified_weight = runqueue_locked.get_priority_task_ref(index).map(|m| m.get_weight()).unwrap_or(0);
    //         runqueue_locked.update_and_move_to_end(index, modified_weight)
    //     })
}
