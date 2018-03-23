use core::ops::DerefMut;
use alloc::arc::Arc;
use alloc::VecDeque;
use irq_safety::{RwLockIrqSafe, interrupts_enabled, disable_interrupts};
use atomic_linked_list::atomic_map::AtomicMap;

use super::{Task, get_my_current_task};

/// This function performs a context switch.
///
/// Interrupts MUST be disabled before this function runs. 
pub fn schedule() -> bool {
    disable_interrupts();
    assert!(interrupts_enabled() == false, "Invoked schedule() with interrupts enabled!");

    // let current_taskid: TaskId = CURRENT_TASK.load(Ordering::SeqCst);
    // trace!("schedule [0]: current_taskid={}", current_taskid);

    let current_task: *mut Task;
    let next_task: *mut Task; 

    let apic_id = match ::interrupts::apic::get_my_apic_id() {
        Some(id) => id,
        _ => {
            error!("Couldn't get apic_id in schedule()");
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

    // trace!("BEFORE CONTEXT_SWITCH CALL (current={}), interrupts are {}", current_taskid, ::interrupts::interrupts_enabled());

    curr.context_switch(next, apic_id); 

    // let new_current: TaskId = CURRENT_TASK.load(Ordering::SeqCst);
    // trace!("AFTER CONTEXT_SWITCH CALL (current={}), interrupts are {}", new_current, ::interrupts::interrupts_enabled());

    true
}



type TaskRef = Arc<RwLockIrqSafe<Task>>;
type RunQueue = VecDeque<TaskRef>;

/// There is one runqueue per core, each core can only access its own private runqueue
/// and select a task from that runqueue to schedule in.
lazy_static! {
    static ref RUNQUEUES: AtomicMap<u8, RwLockIrqSafe<RunQueue>> = AtomicMap::new();
}


/// Creates a new runqueue for the given core
pub fn init_runqueue(which_core: u8) {
    trace!("Created runqueue for core {}", which_core);
    RUNQUEUES.insert(which_core, RwLockIrqSafe::new(RunQueue::new()));
}

/// Adds a `Task` reference to the given core's runqueue
pub fn add_task_to_specific_runqueue(which_core: u8, task: TaskRef) -> Result<(), &'static str> {
    if let Some(ref rq) = RUNQUEUES.get(&which_core) {
        debug!("Added task to runqueue {}, {:?}", which_core, task);
        rq.write().push_back(task);
        Ok(())
    }
    else {
        error!("add_task_to_specific_runqueue(): couldn't get core {}'s runqueue!", which_core);
        Err("couldn't get runqueue for requested core")
    }
}

/// Returns the "least busy" core, which is currently very simple, based on runqueue size.
fn get_least_busy_core() -> Option<u8> {
    let mut min_rq: Option<(u8, usize)> = None;

    for (id, rq) in RUNQUEUES.iter() {
        let rq_size = rq.read().len();

        if let Some(min) = min_rq {
            if rq_size < min.1 {
                min_rq = Some((*id, rq_size));
            }
        }
        else {
            min_rq = Some((*id, rq_size));
        }
    }

    min_rq.map(|m| m.0)
} 


/// Chooses the "least busy" core's runqueue (based on simple runqueue-size-based load balancing)
/// and adds a `Task` reference to that core's runqueue.
pub fn add_task_to_runqueue(task: TaskRef) -> Result<(), &'static str> {
    let mut core_id: Option<u8> = get_least_busy_core();
    // as a backup option, just choose the first runqueue
    if core_id.is_none() {
        core_id = RUNQUEUES.iter().next().map( |v| *v.0);
    }

    match core_id {
        Some(id) => {
            add_task_to_specific_runqueue(id, task)
        }
        _ => {
            error!("Couldn't find any runqueues to add Task {:?}", task);
            Err("couldn't find a suitable runqueue to add task!")
        }
    }
}


// TODO: test this function
pub fn remove_task_from_runqueue(which_core: u8, task: TaskRef) -> Result<(), &'static str> {
    if let Some(ref rq) = RUNQUEUES.get(&which_core) {
        rq.write().retain(|x| Arc::ptr_eq(&x, &task));
        Ok(())
    }
    else {
        error!("remove_task_from_runqueue(): couldn't get core {}'s runqueue!", which_core);
        Err("couldn't get runqueue for requested core")
    }
}



/// this defines the scheduler policy.
/// returns None if there is no schedule-able task
fn select_next_task(apic_id: u8) -> Option<TaskRef>  {

    let mut runqueue_locked = try_opt!(RUNQUEUES.get(&apic_id)).write();
    let mut index_chosen: Option<usize> = None;

    for (i, task) in runqueue_locked.iter().enumerate() {
        let t = task.read();

        // must be runnable
        if !t.is_runnable() {
            continue;
        }

        // must not be running
        if t.is_running() {
            continue;
        }

        // if this task is pinned, it must not be pinned to a different core
        if let Some(pinned) = t.pinned_core {
            if pinned != apic_id {
                // with per-core runqueues, this should never happen!
                panic!("select_next_task() (AP {}) found a task pinned to a different core: {:?}", apic_id, *t);
                // continue;
            }
        }
            
        // found a runnable task!
        index_chosen = Some(i);
        // debug!("select_next_task(): AP {} chose Task {:?}", apic_id, *t);
        break; 
    }

    if let Some(index) = index_chosen {
        let chosen_task: TaskRef = runqueue_locked.remove(index).unwrap();
        runqueue_locked.push_back(chosen_task.clone()); 
        Some(chosen_task)
    }
    else {
        None
    }

}