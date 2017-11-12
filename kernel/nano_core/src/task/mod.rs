
use spin::{Once, Mutex, RwLock};
use irq_safety::{RwLockIrqSafe, RwLockIrqSafeReadGuard};
use collections::{BTreeMap, Vec};
use collections::string::String;
use alloc::arc::Arc;
use core::sync::atomic::{Ordering, AtomicUsize, AtomicBool, ATOMIC_USIZE_INIT, ATOMIC_BOOL_INIT};
use arch::{pause, ArchTaskState};
use alloc::boxed::Box;
use core::fmt;
use core::ops::DerefMut;
use memory::{Stack, ModuleArea, MemoryManagementInfo, VirtualAddress, PhysicalAddress};
use kernel_config::memory::{USER_STACK_ALLOCATOR_BOTTOM, USER_STACK_ALLOCATOR_TOP_ADDR, address_is_page_aligned};

#[macro_use] pub mod scheduler;


type TaskId = usize;
type AtomicTaskId = AtomicUsize;


/// The memory management info and address space of the kernel
static KERNEL_MMI: Once<Arc<Mutex<MemoryManagementInfo>>> = Once::new();

/// The `TaskId` of the currently executing `Task`
static CURRENT_TASK: AtomicTaskId = ATOMIC_USIZE_INIT;
pub fn get_current_task_id() -> TaskId {
    CURRENT_TASK.load(Ordering::Acquire)
}

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
    pub kstack: Option<Stack>,
    /// the userspace stack.  Wrapped in Option<> so we can initialize it to None.
    pub ustack: Option<Stack>,
    /// memory management details: page tables, mappings, allocators, etc.
    /// Wrapped in an Arc & Mutex because it's shared between other tasks in the same address space
    pub mmi: Option<Arc<Mutex<MemoryManagementInfo>>>, 
    /// for special behavior of new userspace task
    pub new_userspace_entry_addr: Option<VirtualAddress>, 
}


impl Task {

    /// creates a new Task structure and initializes it to be non-Runnable.
    fn new(task_id: TaskId) -> Task {
        Task {
            id: task_id,
            runstate: RunState::INITING,
            running_on_cpu: -1, // not running on any cpu
            arch_state: ArchTaskState::new(),
            name: format!("task{}", task_id),
            kstack: None,
            ustack: None,
            mmi: None,
            new_userspace_entry_addr: None,
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
            name: format!("task{}", task_id),
            kstack: None,
        }
    }
    */

    /// switches from the current (`self`)  to the given `next` Task
    /// no locks need to be held to call this, but interrupts (later, preemption) should be disabled
    pub fn context_switch(&mut self, mut next: &mut Task) {
        // debug!("context_switch [0], getting lock.");
        // Set the global lock to avoid the unsafe operations below from causing issues
        while CONTEXT_SWITCH_LOCK.compare_and_swap(false, true, Ordering::SeqCst) {
            pause();
        }

        // debug!("context_switch [1], testing runstates.");
        assert!(next.runstate == RunState::RUNNABLE, "scheduler bug: chosen 'next' Task was not RUNNABLE!");

        let curr_id: usize = self.id;
        let next_id: usize = next.id;
        
        if false {
            // trace!("context_switch: switching from {}({}) to {}({})", self.name, self.id, next.name, next.id;
            use serial_port;
            serial_port::serial_out("\x1b[33m[W] context_switch: switching from ");
            serial_port::serial_outb((curr_id + 48) as u8);
            serial_port::serial_out(" to ");
            serial_port::serial_outb((next_id + 48) as u8);
            serial_port::serial_out(" \x1b[0m\n");
        }

        // update runstates
        self.running_on_cpu = -1; // no longer running
        next.running_on_cpu = 0; // only one CPU right now


        // change the privilege stack (RSP0) in the TSS
        // TODO: skip this when switching to kernel threads, i.e., when next is not a userspace task
        {
            use interrupts::tss_set_rsp0;
            let next_kstack = next.get_kstack().expect("context_switch(): error: next task's kstack was None!");
            tss_set_rsp0(next_kstack.bottom() + (next_kstack.size() / 2)); // the middle half of the stack
        }

        // We now do the page table switching here, so we can use our higher-level PageTable abstractions
        {
            use memory::{PageTable};

            let prev_mmi = self.mmi.as_mut().expect("context_switch: couldn't get prev task's MMI!");
            let next_mmi = next.mmi.as_mut().expect("context_switch: couldn't get next task's MMI!");


            // TODO: we also don't need to switch page tables if we're just moving from any userspace task
            //       to a kernel thread. We'll need some type of identifier in the Task struct
            //       in order to identify a task as userspace or kernel thread only


            if Arc::ptr_eq(prev_mmi, next_mmi) {
                // do nothing because we're not changing address spaces
                // debug!("context_switch [3]: prev_mmi is the same as next_mmi!");
            }
            else {

                // time to change to a different address space and switch the page tables!
                // debug!("context_switch [3]: switching tables! ({} to {})", self.name, next.name);

                let mut prev_mmi_locked = prev_mmi.lock();
                let mut next_mmi_locked = next_mmi.lock();


                let (prev_table_now_inactive, new_active_table) = {
                    // prev_table must be an ActivePageTable, and next_table must be an InactivePageTable
                    match (&mut prev_mmi_locked.page_table, &mut next_mmi_locked.page_table) {
                        (&mut PageTable::Active(ref mut active_table), &mut PageTable::Inactive(ref inactive_table)) => {
                            active_table.switch(inactive_table)
                        }
                        _ => {
                            panic!("context_switch(): prev_table must be an ActivePageTable, next_table must be an InactivePageTable!");
                        }
                    }
                };

                prev_mmi_locked.set_page_table(PageTable::Inactive(prev_table_now_inactive));
                next_mmi_locked.set_page_table(PageTable::Active(new_active_table)); 

            }
        }
       
       
        // debug!("context_switch [2], setting CURRENT_TASK.");
        // update the current task to `next`
        CURRENT_TASK.store(next.id, Ordering::SeqCst);

        // FIXME: releasing the lock here is a temporary workaround, as there is only one CPU active right now
        CONTEXT_SWITCH_LOCK.store(false, Ordering::SeqCst);


        // NOTE:  interrupts are automatically enabled at the end of switch_to
        unsafe {
            self.arch_state.switch_to(&next.arch_state);
        }

    }



    pub fn get_kstack(&self) -> Option<&Stack> {
        self.kstack.as_ref()
    }
}


impl fmt::Display for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{{{}}}", self.name, self.id)
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
            // start at 1, task_zero is reserved for intial kernel thread
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
                debug!("Successfully created task {}", new_id);
                Ok(self.list.get(&new_id).expect("new_task(): couldn't find new task in tasklist"))
            }
            _ => {
                error!("failed to create task {}", new_id);
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

        let mmi_ref = KERNEL_MMI.call_once( || {
            Arc::new(Mutex::new(task_zero_mmi))
        });

        let id_zero: TaskId = 0;
        let mut task_zero = Task::new(id_zero);
        task_zero.runstate = RunState::RUNNABLE;
        task_zero.running_on_cpu = 0; // only one CPU core is up right now
        task_zero.mmi = Some(mmi_ref.clone());

        task_zero.kstack = {
            extern "C" {
                // these are exposed by the assembly linker, found in arch/arch_x86_64/boot.asm
                static initial_stack_top: usize;
                static initial_stack_bottom: usize;
            }

            use memory::Stack;
            // SAFE because we're just accessing linker defines
            unsafe {
                let task_zero_stack_top = &initial_stack_top as *const _ as usize;
                let task_zero_stack_bottom = &initial_stack_bottom as *const _ as usize;
                debug!("CREATING TASK ZERO STACK at top={:#x} bottom={:#x}", task_zero_stack_top, task_zero_stack_bottom);
                Some(Stack::new(task_zero_stack_top, task_zero_stack_bottom))
            }
        };

        CURRENT_TASK.store(id_zero, Ordering::SeqCst); // set this as the current task, obviously

        // insert the new context into the list
        match self.list.insert(id_zero, Arc::new(RwLock::new(task_zero))) {
            None => {
                // None indicates that the insertion didn't overwrite anything, which is what we want
                debug!("Successfully created initial task0");
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

        let new_task_locked = self.new_task().expect("couldn't create task in spawn_kthread()!");
        {
            let mut new_task = new_task_locked.write();
            new_task.set_name(String::from(thread_name));

            // the new kernel thread uses the same kernel address space
            new_task.mmi = Some(KERNEL_MMI.try().expect("spawn_kthread(): KERNEL_MMI was not initialized!!").clone());

            // create and set up a new kstack
            let kstack: Stack = {
                let mut mmi = new_task.mmi.as_mut().expect("spawn_kthread: new_task.mmi was None!").lock();
                mmi.alloc_stack(4).expect("spawn_kthread: couldn't allocate kernel stack!")
            };

            // When this new task is scheduled in, the first spot on the kstack will be popped as the next instruction pointer
            // as such, putting a function pointer on the top of the kstack will cause it to be invoked.
            let func_ptr: usize = kstack.top_usable(); // the top-most usable address on the kstack
            unsafe { 
                *(func_ptr as *mut usize) = kthread_wrapper::<A, R> as usize;
                debug!("checking func_ptr: func_ptr={:#x} *func_ptr={:#x}, kthread_wrapper={:#x}", func_ptr as usize, *(func_ptr as *const usize) as usize, kthread_wrapper::<A, R> as usize);
            }

            // set up the kthread stuff
            let kthread_call = Box::new( KthreadCall::new(arg, func) );
            debug!("Creating kthread_call: {:?}", kthread_call);


            // currently we're using the very bottom of the kstack for kthread arguments
            let arg_ptr = kstack.bottom();
            let kthread_ptr: *mut KthreadCall<A, R> = Box::into_raw(kthread_call);  // consumes the kthread_call Box!
            unsafe {
                *(arg_ptr as *mut _) = kthread_ptr; // as *mut KthreadCall<A, R>; // as usize;
                debug!("checking kthread_call: arg_ptr={:#x} *arg_ptr={:#x} kthread_ptr={:#x} {:?}", arg_ptr as usize, *(arg_ptr as *const usize) as usize, kthread_ptr as usize, *kthread_ptr);
            }


            new_task.arch_state.set_stack(func_ptr); // the top of the kstack
            new_task.kstack = Some(kstack);

            new_task.runstate = RunState::RUNNABLE; // ready to be scheduled in

        }

        scheduler::add_task_to_runqueue(new_task_locked.clone());

        Ok(new_task_locked)
    }


    /// Spawns a new  userspace task based on the provided `ModuleArea`, which should have an entry point called `main`.
    /// optionally, provide a `name` for the new Task. If none is provided, the name from the given `ModuleArea` is used.
    pub fn spawn_userspace(&mut self, module: &ModuleArea, name: Option<&str>) -> Result<&Arc<RwLock<Task>>, &str> {

        use memory::*;
        debug!("spawn_userspace [0]: Interrupts enabled: {}", ::interrupts::interrupts_enabled());

        let new_task_locked = self.new_task().expect("couldn't create task in spawn_userspace()!");
        {
            let mut new_task = new_task_locked.write();
            new_task.set_name(String::from(
                match name {
                    Some(x) => x,
                    None => module.name(),
                }
            ));

            let mut ustack: Option<Stack> = None;

            // create a new InactivePageTable to represent the new process's address space. 
            // currently, we manually copy all of the existing mappings into the new MMI struct (kernel mappings).
            // there is probably a better way to do this by copying the page table frames themselves... or something else. 
            let new_userspace_mmi = {

                let mut kernel_mmi_locked = KERNEL_MMI.try().expect("spawn_userspace(): KERNEL_MMI was not yet initialized!").lock();
                
                // create a new kernel stack for this userspace task
                let kstack: Stack = kernel_mmi_locked.alloc_stack(4).expect("spawn_userspace: couldn't alloc_stack for new kernel stack!");
                // when this new task is scheduled in, we want it to jump to the userspace_wrapper, which will then make the jump to actual userspace
                let func_ptr: usize = kstack.top_usable(); // the top-most usable address on the kstack
                unsafe { 
                    *(func_ptr as *mut usize) = userspace_wrapper as usize;
                    debug!("checking func_ptr: func_ptr={:#x} *func_ptr={:#x}, userspace_wrapper={:#x}", func_ptr as usize, *(func_ptr as *const usize) as usize, userspace_wrapper as usize);
                }
                new_task.kstack = Some(kstack);
                new_task.arch_state.set_stack(func_ptr); // the top of the kstack
                // unlike kthread_spawn, we don't need any arguments at the bottom of the stack,
                // because we can just utilize the task's userspace entry point member


                // destructure the kernel's MMI so we can access its page table and vmas
                let MemoryManagementInfo { 
                    page_table: ref mut kernel_page_table, 
                    ..  // don't need to access the kernel's VMA list or stack allocator, we already allocated a kstack above
                } = *kernel_mmi_locked;

                
                match kernel_page_table {
                    &mut PageTable::Active(ref mut active_table) => {
                        let mut frame_allocator = FRAME_ALLOCATOR.try().unwrap().lock();
                        let mut temporary_page = TemporaryPage::new(frame_allocator.deref_mut());

                        // now that we have the kernel's active table, we need a new inactive table for the userspace Task
                        let mut new_inactive_table: InactivePageTable = {
                            let frame = frame_allocator.allocate_frame().expect("no more frames");
                            InactivePageTable::new(frame, active_table, &mut temporary_page)
                        };

                        // create a new stack allocator for this userspace process
                        let mut user_stack_allocator = {
                            use memory::StackAllocator;
                            let stack_alloc_start = Page::containing_address(USER_STACK_ALLOCATOR_BOTTOM); 
                            let stack_alloc_end = Page::containing_address(USER_STACK_ALLOCATOR_TOP_ADDR);
                            let stack_alloc_range = Page::range_inclusive(stack_alloc_start, stack_alloc_end);
                            StackAllocator::new(stack_alloc_range, true) // true means it's for userspace
                        };

                        // set up the userspace module flags/vma, the actual mapping happens in the .with() closure below 
                        assert!(address_is_page_aligned(module.start_address()), "modules must be page aligned!");
                        // first we need to map the module memory region into our address space, 
                        // so we can then parse the module as an ELF file in the kernel. (Doesn't need to be USER_ACCESSIBLE). 
                        // For now just use identity mapping, we can use identity mapping here because we have a higher-half mapped kernel, YAY! :)
                        let module_flags: EntryFlags = PRESENT;
                        active_table.map_contiguous_frames(module.start_address(), module.size(), 
                                                    module.start_address() as VirtualAddress, // identity mapping
                                                    module_flags, frame_allocator.deref_mut());     
                        use mod_mgmt;
                        let (elf_progs, entry_point) = mod_mgmt::parse_elf_executable(module.start_address() as *const u8, module.size()).unwrap();
                        // now we can unmap the module because we're done reading from it in the ELF parser
                        active_table.unmap_contiguous_pages(module.start_address(), module.size(), frame_allocator.deref_mut());
                        
                        let mut new_user_vmas: Vec<VirtualMemoryArea> = Vec::with_capacity(elf_progs.len() + 2); // doesn't matter, but 2 is for stack and heap

                        debug!("spawn_userspace [4]: ELF entry point: {:#x}", entry_point);
                        new_task.new_userspace_entry_addr = Some(entry_point);

                        active_table.with(&mut new_inactive_table, &mut temporary_page, |mapper| {
                            /*
                             * We need to set the kernel-related entries of our new inactive_table's P4 to the same values used in the kernel's P4.
                             * However, this is done in InactivePageTable::new(), just to make sure a new page table can never be created without including the shared kernel mappings.
                             * Thus, we do not need to handle that here.
                             */


                            // map the userspace module into the new address space.
                            // we can use identity mapping here because we have a higher-half mapped kernel, YAY! :)
                            // debug!("!! mapping userspace module with name: {}", module.name());
                            // TODO: FIXME: map the actual elf regions to their proper address, no longer a linear dumb mapping
                            for prog in elf_progs.iter() {
                                // each program section in the ELF file could be more than one page, but they are contiguous in physical memory
                                debug!("  -- Elf prog: Mapping vaddr {:#x} to paddr {:#x}, size: {:#x}", prog.vma.start_address(), module.start_address() + prog.offset, prog.vma.size());
                                let new_flags = prog.vma.flags() | USER_ACCESSIBLE;
                                mapper.map_contiguous_frames(module.start_address() + prog.offset, prog.vma.size(), 
                                                             prog.vma.start_address(),
                                                             new_flags, frame_allocator.deref_mut());
                                new_user_vmas.push(VirtualMemoryArea::new(prog.vma.start_address(), prog.vma.size(), new_flags, prog.vma.desc()));
                            }

                            // allocate a new userspace stack
                            let (user_stack, user_stack_vma) = user_stack_allocator.alloc_stack(mapper, frame_allocator.deref_mut(), 4)
                                                                                   .expect("spawn_userspace: couldn't allocate new user stack!");
                            ustack = Some(user_stack); 
                            new_user_vmas.push(user_stack_vma);

                            // TODO: give this process a new heap? (assign it a range of virtual addresses but don't alloc phys mem yet)

                        });
                        

                        // return a new mmi struct (for the new userspace task) to the enclosing scope
                        MemoryManagementInfo {
                            page_table: PageTable::Inactive(new_inactive_table),
                            vmas: new_user_vmas,
                            stack_allocator: user_stack_allocator,
                        }
                    }

                    _ => {
                        panic!("spawn_userspace(): current page_table must be an ActivePageTable!");
                    }
                }
            };

            assert!(ustack.is_some(), "spawn_userspace(): ustack was None after trying to alloc_stack!");
            new_task.ustack = ustack;
            new_task.mmi = Some(Arc::new(Mutex::new(new_userspace_mmi)));
            new_task.runstate = RunState::RUNNABLE; // ready to be scheduled in
        }


        scheduler::add_task_to_runqueue(new_task_locked.clone());


        debug!("spawn_userspace [end]: Interrupts enabled: {}", ::interrupts::interrupts_enabled());

        Ok(new_task_locked)
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

    let kthread_call_stack_ptr: *mut KthreadCall<A, R>;
    {
        let tasklist = get_tasklist().read();
        let currtask = tasklist.get_current().expect("kthread_wrapper(): get_current failed in getting kstack").read();
        let kstack = currtask.get_kstack().expect("kthread_wrapper(): get_kstack failed.");
        // in spawn_kthread() above, we use the very bottom of the stack to hold the pointer to the kthread_call
        // let off: isize = 0;
        unsafe {
            // dereference it once to get the raw pointer (from the Box<KthreadCall>)
            kthread_call_stack_ptr = *(kstack.bottom() as *mut *mut KthreadCall<A, R>) as *mut KthreadCall<A, R>;
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


/// this is invoked by the kernel component of a new userspace task 
/// (using its kernel stack) and jumps to userspace using its userspace stack.
/// It runs 
fn userspace_wrapper() -> ! {

    debug!("userspace_wrapper [0]");

    // the three things we need to invoke jump_to_userspace
    let current_task: *mut Task; 
    let ustack_top: usize;
    let entry_func: usize; 

    { // scoped to release tasklist lock before calling jump_to_userspace
        let tasklist = get_tasklist().read();
        let mut currtask = tasklist.get_current().expect("userspace_wrapper(): get_current failed").write();
        ustack_top = currtask.ustack.as_ref().expect("userspace_wrapper(): ustack was None!").top_usable();
        entry_func = currtask.new_userspace_entry_addr.expect("userspace_wrapper(): new_userspace_entry_addr was None!");
        current_task = currtask.deref_mut() as *mut Task;
    }

    debug!("userspace_wrapper [1]: ustack_top: {:#x}, module_entry: {:#x}", ustack_top, entry_func);


    assert!(current_task as usize != 0, "userspace_wrapper(): current_task was null!");
    // SAFE: current_task is checked for null
    unsafe {
        let curr: &mut Task = &mut (*current_task); // dereference current_task and get a ref to it
        curr.arch_state.jump_to_userspace(ustack_top, entry_func);
    }


    panic!("userspace_wrapper [end]: jump_to_userspace returned!!!");
}