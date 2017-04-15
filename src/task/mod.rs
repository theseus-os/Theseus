
use spin::{Once, RwLock, RwLockReadGuard, RwLockWriteGuard};
use collections::BTreeMap;
use collections::string::String;
use alloc::arc::Arc;
use core::sync::atomic::{Ordering, AtomicUsize, AtomicBool, ATOMIC_BOOL_INIT};
use arch::{pause, ArchTaskState};
use alloc::boxed::Box;
use core::mem;

// declare types "TaskId" as a usize and AtomicTaskId as an Atomic usize
int_like!(TaskId, AtomicTaskId, usize, AtomicUsize);


// #[thread_local] // not sure thread_local is a valid attribute
pub static CURRENT_TASK: AtomicTaskId = AtomicTaskId::default();


/// Used to ensure that context switches are done atomically
static CONTEXT_SWITCH_LOCK: AtomicBool = ATOMIC_BOOL_INIT;






#[derive(PartialEq)]
pub enum RunState {
    /// in the midst of setting up the task
    INITING,
    /// able to be scheduled in, but not currently running 
    RUNNABLE, 
    /// actually running 
    RUNNING,
    /// blocked on something, like I/O or a wait event
    BLOCKED, 
    /// includes the exit code (i8)
    EXITED(i8), 
}

struct Task {
    pub id: TaskId,
    pub runstate: RunState, 
    pub arch_state: ArchTaskState,
    /// the kernel stack
    pub kstack: Option<Box<[u8]>>,
    pub name: String,
}


impl Task {

    fn new(task_id: TaskId) -> Task { 
        Task {
            id: task_id, 
            runstate: RunState::INITING, 
            arch_state: ArchTaskState::new(),
            name: format!("task{}", task_id.into()),
            kstack: None,
        }
    }


    // TODO: implement this 
    /*
    fn clone_task(&self, new_id: TaskId) -> Task {
        Task {
            id: task_id, 
            runstate: RunState::INITING, 
            arch_state: self.arch_state.clone(),
            name: format!("task{}", task_id.into()),
            kstack: None,
        }
    }
    */

    /// switches from the current (`self`)  to the `next` `Task`
    /// the lock on 
    pub fn context_switch(&mut self, mut next: &mut Task) {
        // Set the global lock to avoid the unsafe operations below from causing issues
        while CONTEXT_SWITCH_LOCK.compare_and_swap(false, true, Ordering::SeqCst) {
            pause();
        }

        assert!(next.runstate != RunState::BLOCKED, "scheduler bug: chosen 'next' Task was BLOCKED!");
        assert!(next.runstate != RunState::RUNNING, "scheduler bug: chosen 'next' Task was already RUNNING!");

        // update runstates
        self.runstate = RunState::RUNNABLE; 
        next.runstate = RunState::RUNNING; 

        // store the current context ID
        CURRENT_TASK.store(next.id, Ordering::SeqCst);

        // FIXME: releasing the lock here is a temporary workaround, as there is only one CPU active right now
        CONTEXT_SWITCH_LOCK.store(false, Ordering::SeqCst);


        // perform the actual context switch
        unsafe {
            self.arch_state.switch_to(&next.arch_state);
        }
    }


}






/// a singleton that represents all tasks
pub struct TaskList {
    list: BTreeMap<TaskId, Arc<RwLock<Task>>>,
    taskid_counter: usize, 
}

impl TaskList {
    fn new() -> TaskList {
        assert_has_not_been_called!("attempted to initialize TaskList twice!");

        TaskList {
            list: BTreeMap::new(),
            taskid_counter: 1,
        }
    }

    fn get_current(&self) -> Option<&Arc<RwLock<Task>>> {
        self.list.get(&CURRENT_TASK.load(Ordering::SeqCst))
    }

    /// Get a iterator for the list of contexts.
    pub fn iter(&self) -> ::collections::btree_map::Iter<TaskId, Arc<RwLock<Task>>> {
        self.list.iter()
    }

    /// Create a new context.
    ///
    /// ## Returns
    /// A Result with a reference counter for the created Context.
    pub fn create_task(&mut self) -> Result<&Arc<RwLock<Task>>, &str> {

        // first, find a free task id! 
        if self.taskid_counter >= MAX_NR_TASKS {
            self.taskid_counter = 1;
        }
        let starting_taskid: usize = self.taskid_counter;

        // find the next unused id
        while self.list.contains_key(&TaskId::from(self.taskid_counter)) {
            self.taskid_counter += 1;
            if starting_taskid == self.taskid_counter {
                return Err("unable to find free id for new task");
            }
        }

        let new_id = TaskId::from(self.taskid_counter);
        self.taskid_counter += 1;

        // insert the new context into the list
        match self.list.insert(new_id, Arc::new(RwLock::new(Task::new(new_id)))) {
            None => { // None indicates that the insertion didn't overwrite anything, which is what we want
                debug!("Successfully created task {}", new_id.into());
                Ok(self.list.get(&new_id).unwrap())
            }
            _ => Err("Error: overwrote task id!")
        }

    }


    /// Spawn a new task that enters the given function `func`.
    pub fn spawn(&mut self, func: extern fn()) -> Result<&Arc<RwLock<Task>>, &str> {

        let mut curr_cr3: usize = 0;
        {
            curr_cr3 = self.get_current().unwrap().read().arch_state.get_page_table();
        }

        let locked_new_task = self.create_task().expect("couldn't create task in spawn()!");
        {
            // request a mutable reference
            let mut new_task = locked_new_task.write();
            
            // allocate a vector of 16 KB
            let mut stack = vec![0; 16384].into_boxed_slice(); // `stack` is the bottom of the stack


            // TODO: is there a better way to represent function pointers in Rust?

            // Place the `func` function pointer in the first spot on the stack
            let offset = stack.len() - mem::size_of::<usize>();
            unsafe {
                // put it on the top of the stack
                let func_ptr = stack.as_mut_ptr().offset(offset as isize);
                *(func_ptr as *mut usize) = func as usize;
            }

            // this line would be useful if we wish to create an entirely new address space:
            // new_task.arch_state.set_page_table(unsafe { ::arch::memory::paging::ActivePageTable::new().address() });

            // for now, just use the same address space because we're creating a new kernel thread
            new_task.arch_state.set_stack((stack.as_ptr() as usize) + offset); // the top of the stack
            new_task.kstack = Some(stack);

            new_task.runstate = RunState::RUNNABLE; // ready to be scheduled in
        }

        Ok(locked_new_task)
    }

    /// Remove a task from the list.
    ///
    /// ## Parameters
    /// - `id`: the TaskId to be removed.
    ///
    /// ## Returns
    /// An Option with a reference counter for the removed Task. 
    pub fn remove(&mut self, id: TaskId) -> Option<Arc<RwLock<Task>>> {
        self.list.remove(&id)
    }
}



/// the main task list, a singleton that is hidden 
/// and should only be accessed using the get_tasklist() function
/* private*/ static TASK_LIST: Once<RwLock<TaskList>> = Once::new(); 

// the max number of tasks
const MAX_NR_TASKS: usize = usize::max_value() - 1;


// /// a convenience function to get the current task from anywhere,
// /// like Linux's current() macro
// #[inline(always)]
// pub fn get_current_task() -> &Arc<RwLock<Task>> {
//     get_tasklist().read().get_current().expect("failed to get_current_task()")
// }



/// get a reference to the global `TaskList`.
/// Returns a `RwLock` containing the `TaskList`.
/// to modify the task list, call `.write()` on the returned value.
/// To read the task list, call `.read()` on the returned value. 
pub fn get_tasklist() -> &'static RwLock<TaskList> {
    // the first time this is called, the tasklist will be inited
    // on future invocations, that inited task list is simply returned
    TASK_LIST.call_once( || { 
        RwLock::new(TaskList::new())
    }) 
}