use core::ops::DerefMut;
use core::sync::atomic::{Ordering, AtomicUsize, AtomicBool, ATOMIC_BOOL_INIT};

use super::{RunState, get_tasklist, CURRENT_TASK, TaskId, AtomicTaskId, Task};

/// This function picks the next task and then context switch to it. 
/// This is unsafe because we have to maintain references to the current and next tasks
/// beyond the duration of their task locks and the singular task_list lock.
pub unsafe fn schedule() -> bool {
    
    let current_taskid: TaskId = CURRENT_TASK.load(Ordering::SeqCst);
    // debug!("schedule [0]: current_taskid={}", current_taskid.into());

    let mut current_task = 0 as *mut Task; // a null Task ptr
    let mut next_task = 0 as *mut Task; // a null Task ptr
    

    // this is scoped to ensure that the tasklist's RwLock is released at the end.
    // we only request a read lock cuz we're not modifying the list here, 
    // rather just trying to find one that is runnable 
    {
        let tasklist_immut = &get_tasklist().read(); // no need to modify the tasklist
        { 
            // iterate over all tasks EXCEPT the current one
            for (taskid, locked_task) in tasklist_immut.iter().filter(|x| *(x.0) != current_taskid) {
                let id_considered = (*taskid).into();

                let mut task = locked_task.write();
                // debug!("schedule [1]: considering task {} [{:?}]", id_considered, task.runstate);
                if task.runstate == RunState::RUNNABLE {
                    // we use an unsafe deref_mut() operation to ensure that this reference
                    // can remain beyond the lifetime of the tasklist RwLock being held.
                    next_task = task.deref_mut() as *mut Task;
                    // debug!("schedule [2]: chose task {}", *task);
                    break;
                }
            } // writable locked_task is released here
        }

        if next_task as usize == 0 {
            // keep the same current task
            return false; // tasklist is automatically unlocked here, thanks RwLockReadGuard!
        }

        // same scoping reasons as above: to release the tasklist lock and the lock around current_task
        {
            current_task = tasklist_immut.get_current().expect("spawn(): get_current failed in getting current_task")
                           .write().deref_mut() as *mut Task; 
        }
    } // read-only tasklist lock is released here

    // we want mutable references to mutable tasks
    let mut curr = &mut (*current_task); // as &mut Task; 
    let mut next = &mut (*next_task); // as &mut Task; 
    curr.context_switch(next); 

    true
}


/// invokes the scheduler to pick a new task with interrupts disabled. 
/// The current thread may be picked again, it doesn't affect the current thread's runnability.
#[macro_export]
macro_rules! schedule {
    () => (    
        unsafe {
            ::x86_64::instructions::interrupts::disable();
            $crate::task::scheduler::schedule();
            ::x86_64::instructions::interrupts::enable();
        }   
    )
}