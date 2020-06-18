//! This crate picks the next task on token based scheduling policy.
//! At the begining of each scheduling epoch tasks are added to different queues with
//! a set of tokens distributed among tasks depending on their priority.
//! Tasks with higher priority will be added to higher priority queue with higher token count.
//! Each time a task is picked, the token count of the task is decremented by 1.
//! A task is executed only if it has tokens remaining. 
//! When a task run out of it's tokens it will be added to a lower priority queue with a 
//! new set of tokens.
//! When all runnable tasks gets pushed to the lowest priority queue a new scheduling epoch 
//! is initiated and tasks will be redistributed among the queues.
//! The overal distribution is such that task with priority 40 will be scheduled 40 times 
//! a task of priority 1 will be scheduled (provided other conditions doesn't change).
//! In addition this crate offers the interfaces to set and get priorities  of each task.


#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate task;
extern crate runqueue_priority;

use task::TaskRef;
use runqueue_priority::{RunQueue, MAX_PRIORITY, QUEUE_COUNT, DEFAULT_TOKENS};


/// A data structure to transfer data from select_next_task_priority
/// to select_next_task
struct NextTaskResult{
    taskref : Option<TaskRef>,
    idle_task : bool,
}

/// Helper data struct to store the location of the task with the queue 
#[derive(Copy, Clone)]
struct TaskLocation{
    index : usize,
    queue : usize
}

/// Changes the priority of the given task with the given priority level.
/// Priority values must be between 40 (maximum priority) and 0 (minimum prriority).
pub fn set_priority(task: &TaskRef, priority: u8) -> Result<(), &'static str> {
    let priority = core::cmp::min(priority, MAX_PRIORITY);
    RunQueue::set_priority(task, priority)
}

/// Returns the priority of the given task.
pub fn get_priority(task: &TaskRef) -> Option<u8> {
    RunQueue::get_priority(task)
}

/// This defines the priority scheduler policy.
/// Returns None if there is no schedule-able task.
pub fn select_next_task(apic_id: u8) -> Option<TaskRef>  {
    let priority_taskref_with_result = select_next_task_priority(apic_id); 
    match priority_taskref_with_result {
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
/// Returns None if there is no schedule-able task.
/// Otherwise returns a task with a flag indicating whether its an idle task.
fn select_next_task_priority(apic_id: u8) -> Option<NextTaskResult>  {

    let mut runqueue_locked = match RunQueue::get_runqueue(apic_id) {
        Some(rq) => rq.write(),
        _ => {
            #[cfg(not(loscd_eval))]
            error!("BUG: select_next_task_priority(): couldn't get runqueue for core {}", apic_id); 
            return None;
        }
    };
    
    let mut idle_task_location: Option<TaskLocation> = None;
    let mut chosen_task_location: Option<TaskLocation> = None;
    let mut idle_task = true;

    for queue_id in 0..runqueue_priority::QUEUE_COUNT {
        for (i, priority_taskref) in runqueue_locked.queue[queue_id].iter().enumerate() {
            let t = priority_taskref.lock();

            // we skip the idle task, and only choose it if no other tasks are runnable
            if t.is_an_idle_task {
                idle_task_location = Some(TaskLocation{index : i, queue : queue_id});
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
            chosen_task_location = Some(TaskLocation{index : i, queue : queue_id});

            // If that task is in the last queue we still treat it like an idle task for this function.
            // This is because a task get into the last queue only when it is run out of tokens.
            // Therefore a token redistribution needs to be inititated if such task is picked.
            if queue_id < QUEUE_COUNT - 1 {
                idle_task = false;
            }

            break; 
        }

        if chosen_task_location.is_some() {
            break;
        }
    }

    // We then reduce the number of tokens of the task by one.
    // If it has run out of tokens we push it to the next queue with DEFAULT_TOKENS
    let (modified_tokens, new_queue) = {
        let chosen_task = chosen_task_location.and_then(|location| runqueue_locked.queue[location.queue].get(location.index));
        match chosen_task.map(|m| m.tokens_remaining){
            Some(0) => (DEFAULT_TOKENS, chosen_task_location.map_or(2, |location| core::cmp::min(location.queue + 1, QUEUE_COUNT -1))),
            Some(x) => (x.saturating_sub(1), chosen_task_location.map_or(2, |location| location.queue)),
            None => (0, QUEUE_COUNT - 1)
        }
    };

    chosen_task_location
        .or(idle_task_location)
        .and_then(|location| runqueue_locked.update_and_move_to_queue(location.index, location.queue, new_queue, modified_tokens))
        .map(|taskref| NextTaskResult {
            taskref : Some(taskref),
            idle_task  : idle_task, 
        })
}


/// This assigns tokens between tasks.
/// Returns true if successful.
/// Tokens are assigned based on priority.
fn assign_tokens(apic_id: u8) -> bool  {

    let mut runqueue_locked = match RunQueue::get_runqueue(apic_id) {
        Some(rq) => rq.write(),
        _ => {
            #[cfg(not(loscd_eval))]
            error!("BUG: assign_tokens(): couldn't get runqueue for core {}", apic_id); 
            return false;
        }
    };

    // We first move all the tasks to the last queue
    for queue_id in 0..(QUEUE_COUNT -1) {
        while runqueue_locked.queue[queue_id].len() > 0 {
            runqueue_locked.update_and_move_to_queue(0, queue_id, QUEUE_COUNT - 1, 0);
        }
    }
    
    // We run the following loop until all the possible tasks in last queue are moved to a different queue
    let mut task_moved = true;
    while task_moved {
        task_moved = false;
        let mut chosen_task_location: Option<TaskLocation> = None;

        for (i, priority_taskref) in runqueue_locked.queue[QUEUE_COUNT - 1].iter().enumerate() {
            let t = priority_taskref.lock();

            // we skip the idle task, idle tasks always stay in last queue
            if t.is_an_idle_task {
                continue;
            }
                
            // found a runnable task!
            chosen_task_location = Some(TaskLocation{index : i, queue : QUEUE_COUNT - 1});
            task_moved = true;
            // debug!("select_next_task(): AP {} chose Task {:?}", apic_id, *t);
            break; 
        }

        // Allocate tokens based on priority
        let (modified_tokens, new_queue) = {
            let chosen_task = chosen_task_location.and_then(|location| runqueue_locked.queue[location.queue].get(location.index));
            match chosen_task {
                Some(x) => (x.get_initial_token_count(), x.get_initial_priority_queue()),
                None => (DEFAULT_TOKENS, 2),
            }
        };

        // Move to the appropriate queue
        if let Some(task_location) = chosen_task_location {
            runqueue_locked.update_and_move_to_queue(task_location.index, task_location.queue, new_queue, modified_tokens);
        }

    }
    true
}