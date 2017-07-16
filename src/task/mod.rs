
use spin::{Once, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use irq_safety::{RwLockIrqSafe, RwLockIrqSafeReadGuard, RwLockIrqSafeWriteGuard};
use collections::{BTreeMap, Vec};
use collections::string::String;
use alloc::arc::Arc;
use core::sync::atomic::{Ordering, AtomicUsize, AtomicBool, ATOMIC_BOOL_INIT};
use arch::{pause, ArchTaskState};
use alloc::boxed::Box;
use core::mem;
use core::fmt;
use core::ops::DerefMut;
use memory::{Stack, ModuleArea, MemoryManagementInfo};

#[macro_use] pub mod scheduler;


// declare types "TaskId" as a usize and AtomicTaskId as an Atomic usize
int_like!(TaskId, AtomicTaskId, usize, AtomicUsize);


// #[thread_local] // not sure thread_local is a valid attribute
static CURRENT_TASK: AtomicTaskId = AtomicTaskId::default();


/// Used to ensure that context switches are done atomically
static CONTEXT_SWITCH_LOCK: AtomicBool = ATOMIC_BOOL_INIT;


#[repr(u8)] // one byte
#[derive(PartialEq, Debug, Copy, Clone)]
pub enum RunState {
    /// in the midst of setting up the task
    INITING = 0,
    /// able to be scheduled in, but not currently running
    RUNNABLE,
    /// blocked on something, like I/O or a wait event
    BLOCKED,
    /// thread has completed and is ready for cleanup
    EXITED,
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




pub struct Task {
    /// the unique id of this Task, similar to Linux's pid.
    pub id: TaskId,
    /// which cpu core the Task is currently running on.
    /// negative if not currently running.
    pub running_on_cpu: i8,
    /// the runnability status of this task, basically whether it's allowed to be scheduled in.
    pub runstate: RunState,
    /// architecture-specific task state, e.g., registers.
    pub arch_state: ArchTaskState,
    /// the simple name of this Task
    pub name: String,
    /// the kernelspace stack.  Wrapped in Option<> so we can initialize it to None.
    pub kstack: Option<Box<[u8]>>,
    /// the userspace stack.  Wrapped in Option<> so we can initialize it to None.
    pub ustack: Option<Stack>,
    /// memory management details: page tables, mappings, allocators, etc.
    /// Wrapped in an Arc & Mutex because it's shared between other tasks in the same address space
    pub mmi: Option<Arc<Mutex<MemoryManagementInfo>>>, 
}


impl Task {

    /// creates a new Task structure and initializes it to be non-Runnable.
    fn new(task_id: TaskId) -> Task {
        Task {
            id: task_id,
            runstate: RunState::INITING,
            running_on_cpu: -1, // not running on any cpu
            arch_state: ArchTaskState::new(),
            name: format!("task{}", task_id.into()),
            kstack: None,
            ustack: None,
            mmi: None,
        }
    }

    /// set the name of this Task
    pub fn set_name(&mut self, n: String) {
        self.name = n;
    }

    /// set the RunState of this Task
    pub fn set_runstate(&mut self, rs: RunState) {
        self.runstate = rs;
    }

    /// returns true if this Task is currently running on any cpu.
    pub fn is_running(&self) -> bool {
        (self.running_on_cpu >= 0)
    }

    pub fn is_runnable(&self) -> bool {
        (self.runstate == RunState::RUNNABLE)
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

    /// switches from the current (`self`)  to the given `next` Task
    /// the lock on
    pub fn context_switch(&mut self, mut next: &mut Task) {
        // debug!("context_switch [0], getting lock.");
        // Set the global lock to avoid the unsafe operations below from causing issues
        while CONTEXT_SWITCH_LOCK.compare_and_swap(false, true, Ordering::SeqCst) {
            pause();
        }

        // debug!("context_switch [1], testing runstates.");
        assert!(next.runstate == RunState::RUNNABLE, "scheduler bug: chosen 'next' Task was not RUNNABLE!");


        // update runstates
        self.running_on_cpu = -1; // no longer running
        next.running_on_cpu = 0; // only one CPU right now


        // debug!("context_switch [2], setting CURRENT_TASK.");
        // update the current task to `next`
        CURRENT_TASK.store(next.id, Ordering::SeqCst);

        // FIXME: releasing the lock here is a temporary workaround, as there is only one CPU active right now
        CONTEXT_SWITCH_LOCK.store(false, Ordering::SeqCst);


        // We now do the page table switching here, so we can use our higher-level PageTable abstractions
        {
            use memory::{PageTable, ActivePageTable};

            let prev_mmi = self.mmi.as_mut().expect("context_switch: couldn't get prev task's MMI!");
            let next_mmi = next.mmi.as_mut().expect("context_switch: couldn't get next task's MMI!");

            if Arc::ptr_eq(prev_mmi, next_mmi) {
                // do nothing because we're not changing address spaces
                // debug!("context_switch [3]: prev_mmi is the same as next_mmi!");
            }
            else {

                let mut prev_mmi_locked = prev_mmi.lock();
                let mut next_mmi_locked = next_mmi.lock();

                // time to change to a different address space and switch the page tables!
                debug!("context_switch [3]: prev_mmi is different than next_mmi ... switching tables!");
                let ref mut prev_table = prev_mmi.lock().page_table;
                let ref mut next_table = next_mmi.lock().page_table;
                    
                // prev_table must be an ActivePageTable, and next_table must be an InactivePageTable
                match (prev_table, next_table) {
                    (&mut PageTable::Active(ref mut active_table), &mut PageTable::Inactive(ref inactive_table)) => {
                        let (prev_table_now_inactive, new_active_table) = active_table.switch(inactive_table);
                        prev_mmi_locked.set_page_table(PageTable::Inactive(prev_table_now_inactive));
                        next_mmi_locked.set_page_table(PageTable::Active(new_active_table)); // constructs an ActivePageTable from the newly-switched-to cr3 value
                    }
                    _ => {
                        panic!("context_switch(): prev_table must be an ActivePageTable, next_table must be an InactivePageTable!");
                    }
                }

            }
        }



        // perform the actual context switch
        // NOTE:  interrupts are automatically enabled at the end of switch_to
        unsafe {
            self.arch_state.switch_to(&next.arch_state);
        }


    }



    pub fn get_kstack(&self) -> Option<&Box<[u8]>> {
        self.kstack.as_ref()
    }
}


impl fmt::Display for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{{{}}}", self.name, self.id.into())
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

    /// returns a shared reference to the current `Task`
    fn get_current(&self) -> Option<&Arc<RwLock<Task>>> {
        self.list.get(&CURRENT_TASK.load(Ordering::SeqCst))
    }

    /// returns a shared reference to the `Task` specified by the given `task_id`
    pub fn get_task(&self, task_id: &TaskId) -> Option<&Arc<RwLock<Task>>> {
        self.list.get(task_id)
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
            _ => {
                error!("failed to create task {}", new_id.into());
                Err("Error: overwrote task id!")
            }
        }

    }


    /// initialize the first `Task` with special id = 0.
    /// basically just sets up a Task structure around the bootstrapped kernel thread,
    /// the one that enters `rust_main()`.
    /// Returns a reference to the `Task`, protected by a `RwLock`
    pub fn init_task_zero(&mut self, task_zero_mmi: MemoryManagementInfo) -> Result<&Arc<RwLock<Task>>, &str> {
        assert_has_not_been_called!("init_task_zero was already called once!");

        let id_zero = TaskId::from(0);
        let mut task_zero = Task::new(id_zero);
        task_zero.runstate = RunState::RUNNABLE;
        task_zero.running_on_cpu = 0; // only one CPU core is up right now
        task_zero.mmi = Some(Arc::new(Mutex::new(task_zero_mmi)));

        CURRENT_TASK.store(id_zero, Ordering::SeqCst); // set this as the current task, obviously

        // insert the new context into the list
        match self.list.insert(id_zero, Arc::new(RwLock::new(task_zero))) {
            None => {
                // None indicates that the insertion didn't overwrite anything, which is what we want
                println_unsafe!("Successfully created initial task0");
                let tz = self.list.get(&id_zero).expect("init_task_zero(): couldn't find task_zero in tasklist");
                scheduler::add_task_to_runqueue(tz.clone());
                Ok(tz)
            }
            _ => {
                panic!("WTF: task_zero already existed?!?");
                Err("WTF: task_zero already existed?!?")
            }
        }
    }

    

    /// Spawns a new kernel task with the same address space as the current task. 
    /// The new kernel thread is set up to enter the given function `func` and passes it the arguments `arg`.
    /// This merely makes the new task Runanble, it does not context switch to it immediately. That will happen on the next scheduler invocation.
    pub fn spawn_kthread<A: fmt::Debug, R: fmt::Debug>(&mut self,
            func: fn(arg: A) -> R, arg: A, thread_name: &str)
            -> Result<&Arc<RwLock<Task>>, &str> {

        // get the current tasks's memory info
        let curr_mmi_cloned: Option<Arc<Mutex<MemoryManagementInfo>>>;
        {
            let curr_task = self.get_current().expect("spawn_kthread(): get_current failed in getting curr_mmi").read();
            curr_mmi_cloned = curr_task.mmi.clone();
        }

        let locked_new_task = self.new_task().expect("couldn't create task in spawn_kthread()!");
        {
            let mut new_task = locked_new_task.write();
            new_task.set_name(String::from(thread_name));

            // the new kernel thread uses the same address space as the current task (Arc was cloned above)
            new_task.mmi = curr_mmi_cloned;

            // create and set up a new 16KB kstack
            let mut kstack = vec![0; 16384].into_boxed_slice(); // `kstack` is the bottom of the kernel stack

            // When this new task is scheduled in, the first spot on the kstack will be popped as the next instruction pointer
            // as such, putting a function pointer on the top of the kstack will cause it to be invoked.
            let offset = kstack.len() - mem::size_of::<usize>();
            unsafe {
                // put it on the top of the kstack
                let func_ptr = kstack.as_mut_ptr().offset(offset as isize);
                *(func_ptr as *mut usize) = kthread_wrapper::<A, R> as usize;

                // debug!("checking func_ptr: func_ptr={:#x} *func_ptr={:#x}, kthread_wrapper={:#x}", func_ptr as usize, *(func_ptr as *const usize) as usize, kthread_wrapper::<A, R> as usize);
            }


            // set up the kthread stuff
            let kthread_call = Box::new( KthreadCall::new(arg, func) );
            debug!("Creating kthread_call: {:?}", kthread_call);


            // currently we're using the very bottom of the kstack for kthread arguments
            let offset2: isize = 0;
            unsafe {
                let arg_ptr = kstack.as_mut_ptr().offset(offset2);
                let kthread_ptr: *mut KthreadCall<A, R> = Box::into_raw(kthread_call);
                *(arg_ptr as *mut _) = kthread_ptr; // as *mut KthreadCall<A, R>; // as usize; // consumes the kthread_call Box!

                debug!("checking kthread_call: arg_ptr={:#x} *arg_ptr={:#x} kthread_ptr={:#x} {:?}", arg_ptr as usize, *(arg_ptr as *const usize) as usize, kthread_ptr as usize, *kthread_ptr);
                // let recovered_kthread = Box::from_raw(kthread_ptr);
                // debug!("recovered_kthread: {:?}", recovered_kthread);
            }


            new_task.arch_state.set_stack((kstack.as_ptr() as usize) + offset); // the top of the kstack
            new_task.kstack = Some(kstack);

            new_task.runstate = RunState::RUNNABLE; // ready to be scheduled in

        }

        scheduler::add_task_to_runqueue(locked_new_task.clone());

        Ok(locked_new_task)
    }


    /// Spawns a new  userspace task based on the provided `ModuleArea`, which should have an entry point called `main`.
    /// optionally, provide a `name` for the new Task. If none is provided, the name from the given `ModuleArea` is used.
    pub fn spawn_userspace(&mut self, module: &ModuleArea, name: Option<&str>) -> Result<&Arc<RwLock<Task>>, &str> {

        use memory::{address_is_page_aligned, StackAllocator, Page, Frame, EntryFlags, PageTable, VirtualMemoryArea, ActivePageTable, InactivePageTable, TemporaryPage, FRAME_ALLOCATOR, FrameAllocator};
        use memory::*;
        
        ::interrupts::disable_interrupts();

        // get the current tasks's memory info
        let mut curr_mmi_ptr: Option<Arc<Mutex<MemoryManagementInfo>>>;
        {
            let curr_task = self.get_current().expect("spawn_kthread(): get_current failed in getting curr_mmi").read();
            curr_mmi_ptr = curr_task.mmi.clone();
        }

        // create a new InactivePageTable to represent the new process's address space. 
        // currently, we manually copy all of the existing mappings into the new MMI struct (kernel mappings).
        // there is probably a better way to do this by copying the page table frames themselves... or something else. 
        let (mut new_userspace_mmi, old_inactive_table) =
        {

            let mut curr_mmi_locked = curr_mmi_ptr.as_mut().expect("spawn_userspace: couldn't get current task's MMI!").lock();
            let MemoryManagementInfo { 
                page_table: ref mut curr_page_table, 
                vmas: ref curr_vmas, 
                stack_allocator: ref curr_stack_allocator,
            } = *curr_mmi_locked;

            
            match curr_page_table {
                &mut PageTable::Active(ref mut active_table) => {
                    let mut frame_allocator = FRAME_ALLOCATOR.try().unwrap().lock();
                    let mut temporary_page = TemporaryPage::new(frame_allocator.deref_mut());

                    // now that we have the current task's active table, we need a new inactive table for the userspace Task
                    let mut new_inactive_table: InactivePageTable = {
                        let frame = frame_allocator.allocate_frame().expect("no more frames");
                        InactivePageTable::new(frame, active_table, &mut temporary_page)
                    };


                    // we need to use the current active_table to obtain each vma's Page -> Frame mappings/translations
                    let mut curr_page_to_frame_mappings = {
                        let mut mappings: Vec<(Page, Frame, EntryFlags)> = Vec::with_capacity(curr_vmas.len());
                        for vma in curr_vmas {
                            // println_unsafe!("looking at vma: {:?}", vma);
                            for page in vma.pages() { 
                                // println_unsafe!("   looking at page: {:?}", page);
                                mappings.push( 
                                    (page, 
                                     active_table.translate_page(page).expect("page in curr_vma was not mapped!?!"),
                                     vma.flags(),
                                    )
                                );
                            }
                        }
                        mappings
                    };

                    // deep copy the vmas vec so we no longer use the current task's vmas
                    let mut new_vmas = curr_vmas.to_vec(); 

                    // create a new stack allocator for this userspace process
                    let user_stack_allocator = {
                        use memory::StackAllocator;
                        let stack_alloc_start = Page::containing_address(USER_STACK_BOTTOM); 
                        let stack_alloc_end = Page::containing_address(USER_STACK_TOP_ADDR);
                        let stack_alloc_range = Page::range_inclusive(stack_alloc_start, stack_alloc_end);
                        StackAllocator::new(stack_alloc_range, true)
                    };
                    

                    // set up the userspace module flags/vma, the actual mapping happens in the with() closure below 
                    assert!(address_is_page_aligned(module.start_address()), "modules must be page aligned!");
                    let module_flags: EntryFlags = PRESENT | USER_ACCESSIBLE;
                    new_vmas.push(VirtualMemoryArea::new(module.start_address(), module.size(), module_flags, module.name()));
                    

                    active_table.with(&mut new_inactive_table, &mut temporary_page, |mapper| {
                        // iterate over all of the VMAs from the current MMI and also map them to the new_inactive_table
                        for (page, frame, flags) in curr_page_to_frame_mappings {
                            // println_unsafe!("     mapping page {:?} to frame {:?} with flags {:?}", page, frame, flags);
                            mapper.map_to(page, frame, flags, frame_allocator.deref_mut());
                        }

                        // map the userspace module into the new address space
                        // we can use identity mapping here because we have a higher-half mapped kernel, YAY! :)
                        // println_unsafe!("!! mapping userspace module with name: {}", module.name());
                        mapper.map_contiguous_frames(module.start_address(), module.size(), 
                                                     module.start_address() as VirtualAddress, // identity mapping
                                                     module_flags, frame_allocator.deref_mut());

                        // allocate a new userspace stack
                        

                        // TOOD: give this process a new heap? (assign it a range of virtual addresses but don't alloc phys mem yet)

                    });
                    

                    // println_unsafe!("\nAfter active_table.with() in spawn_userspace");


                    // instead of waiting for the next context switch, we switch page tables now 
                    // such that the new userspace task can be setup (heap/stack) and started immediately
                    let (old_inactive_table, mut new_active_table) = active_table.switch(&new_inactive_table);

                    // NOTE: here we are operating in the new userspace Task's address space

                    let mut active_table = new_active_table; // make sure we cannot mistakenly access the old active table



                    let mut returned_mmi = MemoryManagementInfo {
                        page_table: PageTable::Active(active_table),
                        vmas: new_vmas,
                        stack_allocator: user_stack_allocator,
                    };

                    (returned_mmi, old_inactive_table)

                }
                _ => {
                    panic!("spawn_userspace(): current page_table must be an ActivePageTable!");
                }
            }
        };

        // NOTE: here we are operating in the new userspace Task's address space
        curr_mmi_ptr.as_mut().expect("spawn_userspace: after switch, couldn't get current task's MMI!")
            .lock().set_page_table(PageTable::Inactive(old_inactive_table));


        let locked_new_task = self.new_task().expect("couldn't create task in spawn_userspace()!");
        {
            let mut new_task = locked_new_task.write();
            new_task.set_name(String::from(
                match name {
                    Some(x) => x,
                    None => module.name(),
                }
            ));

            let ustack: Stack = new_userspace_mmi.alloc_stack(4).expect("spawn_userspace: couldn't allocate new user stack!");
            let ustack_top: usize  = ustack.top() - mem::size_of::<usize>();
            new_task.ustack = Some(ustack);


            new_task.mmi = Some(Arc::new(Mutex::new(new_userspace_mmi)));
            // map the userspace module into our new address space
            // reminder that we are already in that address space

            // TODO: revamp this to use the stack_allocator api, which is safer with a guard page!
            let mut kstack = vec![0; 16384].into_boxed_slice(); // `kstack` is the bottom of the kstackk

            // we're just setting up this kstack as a backup, i don't think it will be used yet
            let kstack_offset = kstack.len() - mem::size_of::<usize>();
            unsafe {
                let kstack_func_ptr = kstack.as_mut_ptr().offset(kstack_offset as isize);
                *(kstack_func_ptr as *mut usize) = kstack_placeholder as usize;
            }
            new_task.kstack = Some(kstack);


            // to jump to userspace, we need to set the new task's rsp (stack pointer) to the top of our newly-allocated ustack
            // let ustack_offset = ustack.len() - mem::size_of::<usize>();
            // unsafe {
            //     let ustack_top = (ustack.as_ptr() as usize) + ustack_offset;
            //     new_task.ustack = Some(ustack);
            //     new_task.arch_state.jump_to_userspace(ustack_top, 0); //FIXME THIS IS DEF WRONG 
            // }

            // to jump to userspace, we need to set the new task's rsp (stack pointer) to the top of our newly-allocated ustack
            // for now, since we don't support ELF sections yet, 
            // we assume that the module's very first address is its main entry point
            unsafe {
                new_task.arch_state.jump_to_userspace(ustack_top, module.start_address() as VirtualAddress);
            }

            // not quite ready for this one to be scheduled in by our context_switch function
            // TODO: add this back in
            // new_task.runstate = RunState::RUNNABLE; // ready to be scheduled in
        }

        // TODO: add this back in
        // scheduler::add_task_to_runqueue(locked_new_task.clone());

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
/// and should only be accessed using the `get_tasklist()` function
/* private*/ static TASK_LIST: Once<RwLockIrqSafe<TaskList>> = Once::new();

// the max number of tasks
const MAX_NR_TASKS: usize = usize::max_value() - 1;



/*

fn init_tasklist() -> RwLockIrqSafe<TaskList> {
    RwLockIrqSafe::new(TaskList::new())
}

/// get a locked, immutable reference to the global `TaskList`.
/// Returns a `RwLockIrqSafeReadGuard` containing the `TaskList`.
/// to modify the task list, call `get_tasklist_mut()` instead of this.
pub fn get_tasklist() -> RwLockIrqSafeReadGuard<'static, TaskList> {
    // the first time this is called, the tasklist will be inited
    // on future invocations, that inited task list is simply returned
    TASK_LIST.call_once(init_tasklist).read()
}


/// get a locked, mutable reference to the global `TaskList`.
/// Returns a `RwLockIrqSafeWriteGuard` containing the `TaskList`.
/// For read-only access of the task list, call `get_tasklist()` instead of this.
pub fn get_tasklist_mut() -> RwLockIrqSafeWriteGuard<'static, TaskList> {
    // the first time this is called, the tasklist will be inited
    // on future invocations, that inited task list is simply returned
    TASK_LIST.call_once(init_tasklist).write()
}

*/



/// get a reference to the global `TaskList`.
/// Returns a `RwLockIrqSafe` containing the `TaskList`.
/// to modify the task list, call `.write()` on the returned value.
/// To read the task list, call `.read()` on the returned value.
pub fn get_tasklist() -> &'static RwLockIrqSafe<TaskList> {
    // the first time this is called, the tasklist will be inited
    // on future invocations, that inited task list is simply returned
    TASK_LIST.call_once( || {
        RwLockIrqSafe::new(TaskList::new())
    })
}

/// this does not return
fn kthread_wrapper<A: fmt::Debug, R: fmt::Debug>() -> ! {

    let mut kthread_call_stack_ptr: *mut KthreadCall<A, R>;
    {
        let tasklist = get_tasklist().read();
        unsafe {
            let currtask = tasklist.get_current().expect("kthread_wrapper(): get_current failed in getting kstack").read();
            let kstack = currtask.get_kstack().expect("kthread_wrapper(): get_kstack failed.");
            // in spawn_kthread() above, we use the very bottom of the stack to hold the pointer to the kthread_call
            let off: isize = 0;
            // dereference it once to get the raw pointer (from the Box<KthreadCall>)
            kthread_call_stack_ptr = *(kstack.as_ptr().offset(off) as *mut *mut KthreadCall<A, R>) as *mut KthreadCall<A, R>;
        }
    }

    // the pointer to the kthread_call struct (func and arg) was placed on the stack
    let kthread_call: Box<KthreadCall<A, R>> = unsafe {
        Box::from_raw(kthread_call_stack_ptr)
    };
    let kthread_call_val: KthreadCall<A, R> = *kthread_call;

    // debug!("recovered kthread_call: {:?}", kthread_call_val);

    let arg: Box<A> = unsafe {
        Box::from_raw(kthread_call_val.arg)
    };
    let func: fn(arg: A) -> R = kthread_call_val.func;
    // debug!("kthread_wrapper [0.1]: arg {:?}", *arg as A);
    // debug!("kthread_wrapper [0.2]: func {:?}", func);


    info!("about to call kthread func, interrupts are {}", ::interrupts::interrupts_enabled());

    // actually invoke the function spawned in this kernel thread
    let exit_status = func(*arg);


    // cleanup current thread: put it into non-runnable mode, save exit status
    {
        let tasklist: RwLockIrqSafeReadGuard<_> = get_tasklist().read();
        tasklist.get_current().unwrap().write().set_runstate(RunState::EXITED);
    }

    // {
    //     let tasklist = get_tasklist().read();
    //     let curtask = tasklist.get_current().unwrap().write();
    //     debug!("kthread_wrapper[1.5]: curtask {:?} runstate = {:?}", curtask.id, curtask.runstate);
    // }

    debug!("kthread_wrapper [2]: exited with return value {:?}", exit_status);


    trace!("attempting to unschedule kthread...");
    schedule!();

    // we should never ever reach this point
    panic!("KTHREAD_WRAPPER WAS RESCHEDULED AFTER BEING DEAD!")
}




fn kstack_placeholder(_: u64) -> Option<u64> {
    println_unsafe!("!!! kstack_placeholder !!!");
    loop { }
    None
}