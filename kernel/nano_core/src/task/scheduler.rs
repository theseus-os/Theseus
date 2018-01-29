use core::ops::DerefMut;
use alloc::arc::Arc;
use alloc::VecDeque;
use irq_safety::{RwLockIrqSafe, RwLockIrqSafeWriteGuard};
use spin::RwLock;

use super::{get_tasklist, Task};

/// This function performs a context switch.
/// This is unsafe because we have to maintain references to the current and next tasks
/// beyond the duration of their task locks and the singular task_list lock.
///
/// Interrupts MUST be disabled before this function runs. 
pub unsafe fn schedule(reenable_interrupts: bool) -> bool {
    assert!(::interrupts::interrupts_enabled() == false, "Invoked schedule() with interrupts enabled!");

    // let current_taskid: TaskId = CURRENT_TASK.load(Ordering::SeqCst);
    // trace!("schedule [0]: current_taskid={}", current_taskid);

    let current_task: *mut Task;
    let next_task: *mut Task; 

    let apic_id = match ::interrupts::apic::get_my_apic_id() {
        Ok(id) => id,
        _ => {
            error!("Couldn't get apic_id in schedule()");
            return false;
        }
    };

    if let Some(selected_next_task) = select_next_task(apic_id) {
        next_task = selected_next_task.write().deref_mut();  // as *mut Task;
    }
    else {
        return false;
    }

    if next_task as usize == 0 {
        // keep the same current task
        return false; // tasklist is automatically unlocked here, thanks RwLockIrqSafeReadGuard!
    }
    
    // same scoping reasons as above: to release the tasklist lock and the lock around current_task
    {
        let tasklist_immut = &get_tasklist().read(); // no need to modify the tasklist
        current_task = tasklist_immut.get_my_current_task().expect("spawn(): get_my_current_task() failed in getting current_task")
                       .write().deref_mut() as *mut Task; 
    }

    if current_task == next_task {
        // no need to switch if the chosen task is the same as the current task
        return false; // tasklist is automatically unlocked here
    }

    // we want mutable references to mutable tasks
    let curr: &mut Task = &mut (*current_task); // as &mut Task; 
    let next: &mut Task = &mut (*next_task); // as &mut Task; 

    // trace!("BEFORE CONTEXT_SWITCH CALL (current={}), interrupts are {}", current_taskid, ::interrupts::interrupts_enabled());

    curr.context_switch(next, apic_id, reenable_interrupts); 

    // let new_current: TaskId = CURRENT_TASK.load(Ordering::SeqCst);
    // trace!("AFTER CONTEXT_SWITCH CALL (current={}), interrupts are {}", new_current, ::interrupts::interrupts_enabled());

    true
}


/// invokes the scheduler to pick a new task, but first disables interrupts. 
/// Interrupts will NOT be re-enabled after scheduling, so this is safe to call from within an interrupt handler.
/// This also allows us to perform a context switch directly to another task, if we wish... which we never do as of now.
/// The current thread may be picked again, it doesn't affect the current thread's runnability.
#[macro_export]
macro_rules! schedule {
    () => (    
        {
            unsafe {
                $crate::interrupts::disable_interrupts();
                $crate::task::scheduler::schedule(false);
                // interrupts are enabled at the end of switch_to() anyway
                // $crate::interrupts::enable_interrupts();
            }
        }
    )
}


/// invokes the scheduler to pick a new task, but first disables interrupts. 
/// DO NOT CALL THIS FROM WITHIN AN INTERRUPT HANDLER! Interrupts will be automatically re-enabled after scheduling.
/// This iff condition allows us to perform a context switch directly to another task, if we wish... which we never do as of now.
/// The current thread may be picked again, it doesn't affect the current thread's runnability.
#[macro_export]
macro_rules! yield_task {
    () => (    
        {
            unsafe {
                $crate::interrupts::disable_interrupts();
                $crate::task::scheduler::schedule(true);
                // interrupts are enabled at the end of switch_to() anyway
                // $crate::interrupts::enable_interrupts();
            }
        }
    )
}




type TaskRef = Arc<RwLock<Task>>;
type RunQueue = VecDeque<TaskRef>;

/// Currently there is a single system-wide run queue, 
lazy_static! {
    static ref RUNQUEUE: RwLockIrqSafe<RunQueue> = RwLockIrqSafe::new(VecDeque::with_capacity(100));
}

pub fn add_task_to_runqueue(task: TaskRef) {
    RUNQUEUE.write().push_back(task);
}


// TODO: test this function
pub fn remove_task_from_runqueue(task: TaskRef) {
    RUNQUEUE.write().retain(|x| Arc::ptr_eq(&x, &task));
}



/// this defines the scheduler policy.
/// returns None if there is no schedule-able task
fn select_next_task(apic_id: u8) -> Option<TaskRef>  {

    let mut runqueue_locked = RUNQUEUE.write();
    let mut index_chosen: Option<usize> = None;

    for (i, task) in runqueue_locked.iter().enumerate() {
        let t = task.read();

        // must be runnable
        if !t.is_runnable() {
            continue;
        }

        // must not be running on any other cores
        if t.is_running() {
            continue;
        }

        // if this task is pinned, it must not be pinned to a different core
        if let Some(pinned) = t.pinned_core {
            if pinned != apic_id {
                continue;
            }
        }
            
        // found a runnable task!
        index_chosen = Some(i);
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



    // let mut next_task = 0 as *mut Task; // a null Task ptr

    // if next_task as usize == 0 {
    //    None 
    // }
    // else {
    //     Some(&mut *next_task)
    // }
}