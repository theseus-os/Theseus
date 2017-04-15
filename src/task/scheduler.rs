use core::ops::DerefMut;

/// This function picks the next task and then context switch to it. 
/// This is unsafe because we have to maintain references to the current and next tasks
/// beyond the duration of their task locks and the singular task_list lock.
pub unsafe fn schedule() -> bool {
    
    let current_taskid: AtomicTaskId = CURRENT_TASK.load(, Ordering::SeqCst);
    let mut current_task = 0 as *mut Task; // a null Task ptr
    let mut next_task = 0 as *mut Task; // a null Task ptr
    

    // this is scoped to ensure that the tasklist's RwLock is released at the end.
    // we only request a read lock cuz we're not modifying the list here, 
    // rather just trying to find one that is runnable 
    {
        for (taskid, locked_task) in super::get_tasklist().read().iter() {
            if taskid == current_taskid {
                continue;
            }
            let task = &locked_task.write();
            if task.runstate == super::RunState::RUNNABLE {
                // we use an unsafe deref_mut() operation to ensure that this reference
                // can remain beyond the lifetime of the tasklist RwLock being held.
                next_task = task.deref_mut() as *mut Task;
            }
        } // writable locked_task is released here
    } // read-only tasklist lock is released here

    if next_task as usize == 0 {
        warn!("schedule(): next task was None!"); 
        return false;
    }

    // same scoping reasons as above: to release the tasklist lock and the lock around current_task
    {
        current_task = super::get_tasklist().write().deref_mut() as *mut Task; 
    }

    // we want mutable references to mutable tasks
    let mut curr = &mut (&mut *current_task); // as &mut Task; 
    let mut next = &mut (&mut *next_task); // as &mut Task; 
    curr.context_switch(next); 

    true
}