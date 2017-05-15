
use spin::{Once, RwLock, RwLockReadGuard, RwLockWriteGuard};
use collections::BTreeMap;
use collections::string::String;
use alloc::arc::Arc;
use core::sync::atomic::{Ordering, AtomicUsize, AtomicBool, ATOMIC_BOOL_INIT};
use arch::{pause, ArchTaskState, get_page_table_register};
use alloc::boxed::Box;
use core::mem;
use core::any::Any;
use core::fmt;


#[macro_use] pub mod scheduler;


// declare types "TaskId" as a usize and AtomicTaskId as an Atomic usize
int_like!(TaskId, AtomicTaskId, usize, AtomicUsize);


// #[thread_local] // not sure thread_local is a valid attribute
static CURRENT_TASK: AtomicTaskId = AtomicTaskId::default();


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

#[derive(Debug)]
struct KthreadCall<A, R> {
    /// comes from Box::into_raw(Box<A>)
    pub arg: *mut A,
    pub func: fn(arg: A) -> R,
}

impl<A, R> KthreadCall<A, R> {
    pub fn new(a: A, f: fn(arg: A) -> R) -> KthreadCall<A, R> {
        KthreadCall {
            arg: Box::into_raw(Box::new(a)),
            func: f,
        }
    }
}


struct Task {
    pub id: TaskId,
    pub runstate: RunState, 
    pub arch_state: ArchTaskState,
    /// [unused] the kernel stack
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
        debug!("context_switch [0], getting lock.");
        // Set the global lock to avoid the unsafe operations below from causing issues
        while CONTEXT_SWITCH_LOCK.compare_and_swap(false, true, Ordering::SeqCst) {
            pause();
        }

        debug!("context_switch [1], testing runstates.");
        assert!(next.runstate != RunState::BLOCKED, "scheduler bug: chosen 'next' Task was BLOCKED!");
        assert!(next.runstate != RunState::RUNNING, "scheduler bug: chosen 'next' Task was already RUNNING!");

        // update runstates
        self.runstate = RunState::RUNNABLE; 
        next.runstate = RunState::RUNNING; 

        debug!("context_switch [2], setting CURRENT_TASK.");
        // update the current task to `next`
        CURRENT_TASK.store(next.id, Ordering::SeqCst);

        // FIXME: releasing the lock here is a temporary workaround, as there is only one CPU active right now
        CONTEXT_SWITCH_LOCK.store(false, Ordering::SeqCst);

        debug!("context_switch [3], calling switch_to().");

        // perform the actual context switch
        unsafe {
            self.arch_state.switch_to(&next.arch_state);
        }
    }



    pub fn get_kstack(&self) -> Option<&Box<[u8]>> {
        self.kstack.as_ref()
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

    /// instantiate a new `Task`, wraps it in a RwLock and an Arc, and then adds it to the `TaskList`.
    /// this function doesn't actually set up the task's members, e.g., stack, registers, memory areas. 
    pub fn new_task(&mut self) -> Result<&Arc<RwLock<Task>>, &str> {

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
                Ok(self.list.get(&new_id).expect("new_task(): couldn't find new task in tasklist"))
            }
            _ => Err("Error: overwrote task id!")
        }

    }


    /// initialize the first `Task` with special id = 0. 
    /// basically just sets up a Task structure around the bootstrapped kernel thread,
    /// the one that enters`rust_main()`.
    /// Returns a reference to the `Task`, protected by a `RwLock`
    pub fn init_first_task(&mut self) -> Result<&Arc<RwLock<Task>>, &str> {
        assert_has_not_been_called!("init_first_task was already called once!");

        let id_zero = TaskId::from(0);

        let mut task_zero = Task::new(id_zero);
        CURRENT_TASK.store(id_zero, Ordering::SeqCst); // set this as the current task, obviously
        task_zero.runstate == RunState::RUNNING; // it's the one currently running!
        
        // task_zero's page table and stack registers will be set on the first context switch by `switch_to()`,
        // but we still have to set its page table to the current value 
        task_zero.arch_state.set_page_table(get_page_table_register());
        
        
        // insert the new context into the list
        match self.list.insert(id_zero, Arc::new(RwLock::new(task_zero))) {
            None => { // None indicates that the insertion didn't overwrite anything, which is what we want
                debug!("Successfully created initial task0");
                Ok(self.list.get(&id_zero).expect("init_first_task(): couldn't find task_zero in tasklist"))
            }
            _ => Err("WTF: task_zero already existed?!?")
        }
    }

    /// Spawn a new task that enters the given function `func` and passes it the arguments `arg`
    pub fn spawn<A: fmt::Debug + , R: fmt::Debug>(&mut self, 
            func: fn(arg: A) -> R, arg: A) 
            -> Result<&Arc<RwLock<Task>>, &str> {

        // right now we only have one page table (memory area) shared between the kernel,
        // so just get the current page table value and set the new task's value to the same thing
        let mut curr_pgtbl: usize = 0;
        {
            curr_pgtbl = self.get_current().expect("spawn(): get_current failed in getting curr_pgtbl")
                        .read().arch_state.get_page_table();
        }

        let locked_new_task = self.new_task().expect("couldn't create task in spawn()!");
        {
            // request a mutable reference
            let mut new_task = locked_new_task.write();
            
            // this line would be useful if we wish to create an entirely new address space:
            // new_task.arch_state.set_page_table(unsafe { ::arch::memory::paging::ActivePageTable::new().address() });
            // for now, just use the same address space because we're creating a new kernel thread
            new_task.arch_state.set_page_table(curr_pgtbl); 


            // create and set up a new 16KB stack 
            let mut stack = vec![0; 16384].into_boxed_slice(); // `stack` is the bottom of the stack

            // When scheduled in, the first spot on the stack will 
            let offset = stack.len() - mem::size_of::<usize>();
            unsafe {
                // put it on the top of the stack
                let func_ptr = stack.as_mut_ptr().offset(offset as isize);
                *(func_ptr as *mut usize) = kthread_wrapper::<A, R> as usize;

                // debug!("checking func_ptr: func_ptr={:#x} *func_ptr={:#x}, kthread_wrapper={:#x}", func_ptr as usize, *(func_ptr as *const usize) as usize, kthread_wrapper::<A, R> as usize);
            }


            // set up the kthread stuff
            let kthread_call = Box::new( KthreadCall::new(arg, func) );
            debug!("Creating kthread_call: {:?}", kthread_call); 


            // currently we're using the very bottom of the stack for kthread arguments
            let offset2: isize = 0;
            unsafe {
                let arg_ptr = stack.as_mut_ptr().offset(offset2);
                let mut kthread_ptr: *mut KthreadCall<A, R> = Box::into_raw(kthread_call);
                *(arg_ptr as *mut _) = kthread_ptr; // as *mut KthreadCall<A, R>; // as usize; // consumes the kthread_call Box!

                debug!("checking kthread_call: arg_ptr={:#x} *arg_ptr={:#x} kthread_ptr={:#x} {:?}", arg_ptr as usize, *(arg_ptr as *const usize) as usize, kthread_ptr as usize, *kthread_ptr);
                // let recovered_kthread = Box::from_raw(kthread_ptr);
                // debug!("recovered_kthread: {:?}", recovered_kthread);
            }


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

use core::ops::Deref;
/// this does not return
pub fn kthread_wrapper<A: fmt::Debug, R: fmt::Debug>() -> ! {
    debug!("kthread_wrapper [0], looping indefinitely");

    let mut kthread_call_stack_ptr: *mut KthreadCall<A, R>;
    {
        let tasklist = get_tasklist().read();
        unsafe {
            let currtask = tasklist.get_current().expect("kthread_wrapper(): get_current failed in getting kstack").read();
            let kstack = currtask.get_kstack().expect("kthread_wrapper(): get_kstack failed.");
            // in spawn() above, we use the very bottom of the stack to hold the pointer to the kthread_call
            let off: isize = 0;
            // dereference it once to get the raw pointer (from the Box<KthreadCall>)
            kthread_call_stack_ptr = *(kstack.as_ptr().offset(off) as *mut *mut KthreadCall<A, R>) as *mut KthreadCall<A, R>;
        }
    }

    println!("kthread_call_stack_ptr = {:#x}", kthread_call_stack_ptr as usize);

    // the pointer to the kthread_call struct (func and arg) was placed on the stack
    let kthread_call: Box<KthreadCall<A, R>> = unsafe {
        Box::from_raw(kthread_call_stack_ptr) 
    };
    let kthread_call_val: KthreadCall<A, R> = *kthread_call;

    debug!("recovered kthread_call: {:?}", kthread_call_val);

    let arg: Box<A> = unsafe {
        Box::from_raw(kthread_call_val.arg)
    };
    let func: fn(arg: A) -> R = kthread_call_val.func;

    // debug!("kthread_wrapper [0.1]: arg {:?}", *arg as A);
    // debug!("kthread_wrapper [0.2]: func {:?}", func);
    let exit_status = func(*arg); // FIXME: this works for objects but not primitives like u64
    debug!("kthread_wrapper [2]: exited with return value {:?}", exit_status);


    // cleanup current thread: put it into non-runnable mode, save exit status
    {
        let tasklist = get_tasklist().read();
        tasklist.get_current().unwrap().write().runstate = RunState::EXITED(3);
    }


    debug!("attempting to unschedule kthread...");
    schedule!();

    loop { }
}