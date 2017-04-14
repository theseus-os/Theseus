/// pick the next task and then context switch to it. 
pub unsafe fn schedule() -> bool {

    // TODO FIX THIS 
    let current_task: &Task = task_list.get where task.id == CURRENT_TASK

    let next_task: Option<&Task> = None;

    //// old code start
    let mut to_ptr = 0 as *mut Context;
    {
        // get the list of context
        let contexts = contexts();

        // get the current context
        {
            let context_lock = contexts.current().expect("context::switch: not inside of context");
            let mut context = context_lock.write();
            from_prt = context.deref_mut() as *mut Context;
        }

        // TODO we must create a mechanism to prevent switch processors from other CPU's

        // find the next context to be executed
        for (pid, context_lock) in contexts.iter() {
            if *pid > (*from_prt).id {
                let mut context = context_lock.write();
                to_ptr = context.deref_mut() as *mut Context;
            }
        }
    }
    //// old code end


    match next_task {
        Some(next) => {
            current_task.contex_switch(next);
            true
        }
        _ => { 
            warn!("schedule(): next task was None!"); 
            false
        }
    }

  
}