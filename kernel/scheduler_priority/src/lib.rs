//! This crate picks the next task on token based scheduling policy
//! At the begining of each scheduling epoch a set of tokens is distributed among tasks
//! depending on their priority.
//! tokens assigned to each task = (prioirty of each task / prioirty of all tasks)*length_of_epoch
//! Each time a task is picked tokens_assigned to the task is decremented by 1
//! A task is executed only if it has tokens remaining
//! When all tokens of all runnable task are exhausted a new scheduling epoch is initiated 


#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate task;
extern crate runqueue_priority;



use task::TaskRef;
use runqueue_priority::RunQueue;

// A data structure to transfer data from select_next_task_priority
// to select_next_task
struct NextTaskResult{
    taskref : Option<TaskRef>,
    idle_task : bool,
}

pub fn set_priority(task: &TaskRef, priority: u8) -> Result<(), &'static str> {
    RunQueue::set_priority(task, priority)
}

pub fn get_priority(task: &TaskRef) -> Option<u8> {
    RunQueue::get_priority(task)
}

/// this defines the priority scheduler policy.
/// returns None if there is no schedule-able task
pub fn select_next_task(apic_id: u8) -> Option<TaskRef>  {
    let taskref_with_result = select_next_task_priority(apic_id); 
    match taskref_with_result {
        // A task has been selected
        Some(task) => {
            // If the selected task is idle task we begin a new scheduling epoch
            if task.idle_task == true {
                assign_tokens(apic_id);
                select_next_task_priority(apic_id).and_then(|m| m.taskref)
            }
            // If the selected task is not idle we return the taskref
            else {
                task.taskref
            }
        }

        // If no task is picked we pick a new scheduling epoch
        None    => {
            assign_tokens(apic_id);
            select_next_task_priority(apic_id).and_then(|m| m.taskref)
        }
    }
}

/// this defines the priority scheduler policy.
/// returns None if there is no schedule-able task
/// Otherwise returns a task with a flag indicating whether its an idle task
fn select_next_task_priority(apic_id: u8) -> Option<NextTaskResult>  {

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

        // if the task has no remaining tokens we ignore the task
        if taskref.tokens_remaining == 0{
            continue;
        }
            
        // found a runnable task!
        chosen_task_index = Some(i);
        idle_task = false;
        // debug!("select_next_task(): AP {} chose Task {:?}", apic_id, *t);
        break; 
    }

    // We then reduce the number of tokens of the task by one
    let modified_tokens = {
        let chosen_task = chosen_task_index.and_then(|index| runqueue_locked.get_priority_task_ref(index));
        chosen_task.map(|m| m.tokens_remaining).unwrap_or(1) - 1
    };

    chosen_task_index
        .or(idle_task_index)
        .and_then(|index| runqueue_locked.update_and_move_to_end(index, modified_tokens))
        .map(|index| NextTaskResult {
            taskref : Some(index),
            idle_task  : idle_task, 
        })
}


/// This assigns tokens between tasks
/// Returns true if successful
/// Tokens are assigned based on  (prioirty of each task / prioirty of all tasks)
fn assign_tokens(apic_id: u8) -> bool  {

    let mut runqueue_locked = match RunQueue::get_runqueue(apic_id) {
        Some(rq) => rq.write(),
        _ => {
            error!("BUG: assign_tokens(): couldn't get runqueue for core {}", apic_id); 
            return false;
        }
    };
    

    // We begin with total priorities = 1 to avoid division by zero 
    let mut total_priorities = 1;

    // This loop calculates the total priorities of the runqueue
    for (_i, taskref) in runqueue_locked.iter().enumerate() {
        let t = taskref.lock();

        // we skip the idle task, it contains zero tokens as it is picked last
        if t.is_an_idle_task {
            continue;
        }

        // we assign tokens only to runnable tasks
        if !t.is_runnable() {
            continue;
        }

        // if this task is pinned, it must not be pinned to a different core
        if let Some(pinned) = t.pinned_core {
            if pinned != apic_id {
                // with per-core runqueues, this should never happen!
                error!("select_next_task() (AP {}) found a task pinned to a different core: {:?}", apic_id, *t);
                return false;
            }
        }
            
        // found a runnable task!
        // We add its priority
        // debug!("assign_tokens(): AP {} Task {:?} priority {}", apic_id, *t, taskref.priority);
        total_priorities = total_priorities + 1 + taskref.priority as u32;
        
        
        
        // debug!("assign_tokens(): AP {} chose Task {:?}", apic_id, *t);
        // break; 
    }

    // We keep each epoch for 100 tokens by default
    // However since this granularity could miss low priority tasks when 
    // many concurrent tasks are running, we increase the epoch in such cases
    let epcoh = if total_priorities < 100 {
        100
    }
    else {
        total_priorities
    };

    let mut _i = 0;
    let len = runqueue_locked.runqueue_length();

    // We iterate through each task in runqueue
    // We dont use iterator as items are modified in the process
    while _i < len {
        let task_tokens;
        {
            let taskref = runqueue_locked.get_priority_task_ref(_i).unwrap();
            let t = taskref.lock();

            // we give zero tokens to the idle tasks
            if t.is_an_idle_task {
                _i = _i+1;
                continue;
            }

            // we give zero tokens to none runnable tasks
            if !t.is_runnable() {
                _i = _i+1;
                continue;
            }

            // if this task is pinned, it must not be pinned to a different core
            if let Some(pinned) = t.pinned_core {
                if pinned != apic_id {
                    // with per-core runqueues, this should never happen!
                    error!("select_next_task() (AP {}) found a task pinned to a different core: {:?}", apic_id, *t);
                    return false;
                }
            }
            task_tokens = epcoh * (taskref.priority as u32 + 1) / total_priorities;
        }
        
        {
            let task = runqueue_locked.get_priority_task_ref_as_mut(_i);
            task.map(|m| m.tokens_remaining = task_tokens);
        }
        
        _i = _i+1;
        // debug!("assign_tokens(): AP {} chose Task {:?}", apic_id, *t);
        // break; 
    }

    return true;
}