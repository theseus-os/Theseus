use core::ops::DerefMut;
use alloc::arc::Arc;
use core::sync::atomic::{Ordering, AtomicUsize, AtomicBool, ATOMIC_BOOL_INIT};
use collections::VecDeque;
use irq_safety::{RwLockIrqSafe, RwLockIrqSafeReadGuard, RwLockIrqSafeWriteGuard};
use spin::RwLock;

use super::{RunState, get_tasklist, CURRENT_TASK, TaskId, AtomicTaskId, Task};

/// This function performs a context switch.
/// This is unsafe because we have to maintain references to the current and next tasks
/// beyond the duration of their task locks and the singular task_list lock.
///
/// Interrupts MUST be disabled before this function runs. 
pub unsafe fn schedule() -> bool {

    let current_taskid: TaskId = CURRENT_TASK.load(Ordering::SeqCst);
    // trace!("schedule [0]: current_taskid={}", current_taskid.into());

    let mut current_task = 0 as *mut Task; // a null Task ptr
    let mut next_task = 0 as *mut Task; // a null Task ptr
    

    // this is scoped to ensure that the tasklist's RwLockIrqSafe is released at the end.
    // we only request a read lock cuz we're not modifying the list here, 
    // rather just trying to find one that is runnable 
    {
        if let Some(selected_next_task) = select_next_task(&mut RUNQUEUE.write()) {
            next_task = selected_next_task.write().deref_mut();  // as *mut Task;
        }
        else {
            return false;
        }
    } // RUNQUEUE is released here


    if next_task as usize == 0 {
        // keep the same current task
        return false; // tasklist is automatically unlocked here, thanks RwLockIrqSafeReadGuard!
    }
    
    // same scoping reasons as above: to release the tasklist lock and the lock around current_task
    {
        let tasklist_immut = &get_tasklist().read(); // no need to modify the tasklist
        current_task = tasklist_immut.get_current().expect("spawn(): get_current failed in getting current_task")
                        .write().deref_mut() as *mut Task; 
    }


    // we want mutable references to mutable tasks
    let mut curr: &mut Task = &mut (*current_task); // as &mut Task; 
    let mut next: &mut Task = &mut (*next_task); // as &mut Task; 

    // trace!("BEFORE CONTEXT_SWITCH CALL (current={}), interrupts are {}", current_taskid.into(), ::interrupts::interrupts_enabled());

    curr.context_switch(next); 

    // let new_current: TaskId = CURRENT_TASK.load(Ordering::SeqCst);
    // trace!("AFTER CONTEXT_SWITCH CALL (current={}), interrupts are {}", new_current.into(), ::interrupts::interrupts_enabled());

    true
}


/// invokes the scheduler to pick a new task, but first disables interrupts. 
/// Interrupts will be automatically re-enabled after scheduling. 
/// The current thread may be picked again, it doesn't affect the current thread's runnability.
#[macro_export]
macro_rules! schedule {
    () => (    
        {
            unsafe {
                $crate::interrupts::disable_interrupts();
                $crate::task::scheduler::schedule();
                $crate::interrupts::enable_interrupts();
            }
        }
    )
}


type TaskRef = Arc<RwLock<Task>>;
type RunQueue = VecDeque<TaskRef>;

lazy_static! {
    static ref RUNQUEUE: RwLockIrqSafe<RunQueue> = RwLockIrqSafe::new(VecDeque::with_capacity(100));
}

pub fn add_task_to_runqueue(task: TaskRef) {
    RUNQUEUE.write().push_back(task);
}

pub fn remove_task_from_runqueue(task: TaskRef) {
    RUNQUEUE.write().retain(|x| Arc::ptr_eq(&x, &task));
}



/// this defines the scheduler policy.
/// returns None if there is no schedule-able task
fn select_next_task(runqueue_locked: &mut RwLockIrqSafeWriteGuard<RunQueue>) -> Option<TaskRef>  {
    
    let mut index_chosen: Option<usize> = None;


    for i in 0..runqueue_locked.len() {

        if let Some(t) = runqueue_locked.get(i) {
            if t.read().is_runnable() {
                // found the first runnable task
                index_chosen = Some(i);
                break; 
            }
        }
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