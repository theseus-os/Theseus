#![no_std]
#![feature(alloc)]
#![feature(asm)]
#![feature(core_intrinsics)]
#![feature(stmt_expr_attributes)]

extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate debugit;
extern crate irq_safety;
extern crate atomic_linked_list;
extern crate memory;
extern crate kernel_config;
extern crate task;
extern crate runqueue;
extern crate scheduler;
extern crate mod_mgmt;
extern crate gdt;
extern crate owning_ref;
extern crate apic;
extern crate context_switch;
extern crate path;


use core::mem;
use core::marker::PhantomData;
use core::ops::DerefMut;
use core::sync::atomic::{Ordering, AtomicBool, compiler_fence};
use alloc::vec::Vec;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::boxed::Box;
use irq_safety::{MutexIrqSafe, hold_interrupts, enable_interrupts, interrupts_enabled};
use memory::{get_kernel_mmi_ref, PageTable, MappedPages, Stack, MemoryManagementInfo, Page, VirtualAddress, FRAME_ALLOCATOR, VirtualMemoryArea, FrameAllocator, allocate_pages_by_bytes, TemporaryPage, EntryFlags, InactivePageTable, Frame};
use kernel_config::memory::{KERNEL_STACK_SIZE_IN_PAGES, USER_STACK_ALLOCATOR_BOTTOM, USER_STACK_ALLOCATOR_TOP_ADDR, address_is_page_aligned};
use task::{Task, TaskRef, SimdExt, get_my_current_task, RunState, TASKLIST, TASK_SWITCH_LOCKS};
use gdt::{AvailableSegmentSelector, get_segment_selector};
use path::Path;


/// Initializes tasking for the given AP core, including creating a runqueue for it
/// and creating its initial idle task for that core. 
pub fn init(kernel_mmi_ref: Arc<MutexIrqSafe<MemoryManagementInfo>>, apic_id: u8,
            stack_bottom: VirtualAddress, stack_top: VirtualAddress) 
            -> Result<TaskRef, &'static str> 
{
    TASK_SWITCH_LOCKS.insert(apic_id, AtomicBool::new(false));    

    runqueue::init(apic_id)?;
    
    let task_ref = task::create_idle_task(apic_id, stack_bottom, stack_top, kernel_mmi_ref)?;
    runqueue::add_task_to_specific_runqueue(apic_id, task_ref.clone())?;
    Ok(task_ref)
}


/// The argument type accepted by the `main` function entry point into each application.
type MainFuncArg = Vec<String>;

/// The type returned by the `main` function entry point of each application.
type MainFuncRet = isize;

/// The function signature of the `main` function that every application must have,
/// as it is the entry point into each application `Task`.
type MainFunc = fn(MainFuncArg) -> MainFuncRet;

/// A struct that uses the Builder pattern to create and customize new kernel `Task`s.
/// Note that the new `Task` will not actually be created until the [`spawn`](#method.spawn) method is invoked.
pub struct KernelTaskBuilder<F, A, R> {
    func: F,
    argument: A,
    _rettype: PhantomData<R>,
    name: Option<String>,
    pin_on_core: Option<u8>,

    #[cfg(simd_personality)]
    simd: SimdExt,
}

impl<F, A, R> KernelTaskBuilder<F, A, R> 
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R,
{
    /// Creates a new `Task` from the given kernel function `func`
    /// that will be passed the argument `arg` when spawned. 
    pub fn new(func: F, argument: A) -> KernelTaskBuilder<F, A, R> {
        KernelTaskBuilder {
            argument: argument,
            func: func,
            _rettype: PhantomData,
            name: None,
            pin_on_core: None,

            #[cfg(simd_personality)]
            simd: SimdExt::None,
        }
    }

    /// Set the String name for the new Task.
    pub fn name(mut self, name: String) -> KernelTaskBuilder<F, A, R> {
        self.name = Some(name);
        self
    }

    /// Pin the new Task to a specific core.
    pub fn pin_on_core(mut self, core_apic_id: u8) -> KernelTaskBuilder<F, A, R> {
        self.pin_on_core = Some(core_apic_id);
        self
    }

    /// Mark this new Task as a SIMD-enabled Task 
    /// that can run SIMD instructions and use SIMD registers.
    #[cfg(simd_personality)]
    pub fn simd(mut self, extension: SimdExt) -> KernelTaskBuilder<F, A, R> {
        self.simd = extension;
        self
    }

    /// Finishes this `KernelTaskBuilder` and spawns a new kernel task in the same address space as the current task. 
    /// This merely makes the new task Runnable, it does not switch to it immediately. That will happen on the next scheduler invocation.
    #[inline(never)]
    pub fn spawn(self) -> Result<TaskRef, &'static str> 
        // where A: Send + 'static, 
        //       R: Send + 'static,
        //       F: FnOnce(A) -> R, 
    {
        let mut new_task = Task::new();
        new_task.name = self.name.unwrap_or_else(|| String::from( 
            // if a Task name wasn't provided, then just use the function's name
            unsafe { ::core::intrinsics::type_name::<F>() }
        ));
    
        #[cfg(simd_personality)] {  
            new_task.simd = self.simd;
        }

        // the new kernel thread uses the same kernel address space
        new_task.mmi = Some( try!(get_kernel_mmi_ref().ok_or("KERNEL_MMI was not initialized!!")) );

        // create and set up a new kstack
        let mut kstack: Stack = {
            let mut mmi = try!(new_task.mmi.as_mut().ok_or("new_task.mmi was None!")).lock();
            try!(mmi.alloc_stack(KERNEL_STACK_SIZE_IN_PAGES).ok_or("couldn't allocate kernel stack!"))
        };

        setup_context_trampoline(&mut kstack, &mut new_task, task_wrapper::<F, A, R>);

        // set up the kthread stuff
        let kthread_call = Box::new( KthreadCall::new(self.argument, self.func) );
        // debug!("Creating kthread_call: {:?}", debugit!(kthread_call));

        // currently we're using the very bottom of the kstack for kthread arguments
        let arg_ptr = kstack.bottom();
        let kthread_ptr: *mut KthreadCall<F, A, R> = Box::into_raw(kthread_call);  // consumes the kthread_call Box!
        unsafe {
            *(arg_ptr as *mut _) = kthread_ptr; // as *mut KthreadCall<A, R>; // as usize;
            // debug!("checking kthread_call: arg_ptr={:#x} *arg_ptr={:#x} kthread_ptr={:#x} {:?}", arg_ptr as usize, *(arg_ptr as *const usize) as usize, kthread_ptr as usize, debugit!(*kthread_ptr));
        }

        new_task.kstack = Some(kstack);
        new_task.runstate = RunState::Runnable; // ready to be scheduled in

        let new_task_id = new_task.id;
        let task_ref = TaskRef::new(new_task);
        let old_task = TASKLIST.lock().insert(new_task_id, task_ref.clone());
        // insert should return None, because that means there was no existing task with the same ID 
        if old_task.is_some() {
            error!("BUG: KernelTaskBuilder::spawn(): Fatal Error: TASKLIST already contained a task with the new task's ID!");
            return Err("BUG: TASKLIST a contained a task with the new task's ID");
        }
        
        if let Some(core) = self.pin_on_core {
            runqueue::add_task_to_specific_runqueue(core, task_ref.clone())?;
        }
        else {
            runqueue::add_task_to_any_runqueue(task_ref.clone())?;
        }

        Ok(task_ref)
    }

}

/// A struct that uses the Builder pattern to create and customize new application `Task`s.
/// Note that the new `Task` will not actually be created until the [`spawn`](#method.spawn) method is invoked.
pub struct ApplicationTaskBuilder {
    path: Path,
    argument: MainFuncArg,
    name: Option<String>,
    pin_on_core: Option<u8>,
    singleton: bool,

    #[cfg(simd_personality)]
    simd: SimdExt,
}

impl ApplicationTaskBuilder {
    /// Creates a new application `Task` from the given `path`, which points to 
    /// an application crate object file that must have an entry point called `main`.
    pub fn new(path: Path) -> ApplicationTaskBuilder {
        ApplicationTaskBuilder {
            path: path,
            argument: Vec::new(), // doesn't allocate yet
            name: None,
            pin_on_core: None,
            singleton: false,

            #[cfg(simd_personality)]
            simd: SimdExt::None,
        }
    }

    /// Set the String name for the new Task.
    pub fn name(mut self, name: String) -> ApplicationTaskBuilder {
        self.name = Some(name);
        self
    }

    /// Pin the new Task to a specific core.
    pub fn pin_on_core(mut self, core_apic_id: u8) -> ApplicationTaskBuilder {
        self.pin_on_core = Some(core_apic_id);
        self
    }

    /// Mark this new Task as a SIMD-enabled Task 
    /// that can run SIMD instructions and use SIMD registers.
    #[cfg(simd_personality)]
    pub fn simd(mut self, extension: SimdExt) -> ApplicationTaskBuilder {
        self.simd = extension;
        self
    }

    /// Set the argument strings for this Task.
    pub fn argument(mut self, argument: MainFuncArg) -> ApplicationTaskBuilder {
        self.argument = argument;
        self
    }

    /// Sets this application Task to be a **singleton** application.
    /// A singleton application is a special application whose public symbols are added
    /// to the default namespace's symbol map, which allows other applications to depend upon it. 
    /// This also prevents this application from being re-loaded again, making it a system-wide singleton that cannot be duplicated.
    /// 
    /// In general, for regular applications, you likely should *not* use this. 
    pub fn singleton(mut self) -> ApplicationTaskBuilder {
        self.singleton = true;
        self
    }

    /// Spawns a new application task that runs in kernel mode (currently the only way to run applications).
    /// This merely makes the new task Runnable, it does not task switch to it immediately. That will happen on the next scheduler invocation.
    /// 
    /// This is similar (but not identical) to the `exec()` system call in POSIX environments. 
    pub fn spawn(self) -> Result<TaskRef, &'static str> {
        let app_crate_ref = {
            let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("couldn't get_kernel_mmi_ref")?;
            let mut kernel_mmi = kernel_mmi_ref.lock();
            mod_mgmt::get_default_namespace().ok_or("couldn't get default namespace")?
                .load_application_crate(&self.path, kernel_mmi.deref_mut(), self.singleton, false)?
        };

        // get the LoadedSection for the "main" function in the app_crate
        let main_func_sec_ref = app_crate_ref.lock_as_ref().get_function_section("main")
            .ok_or("ApplicationTaskBuilder::spawn(): couldn't find \"main\" function!")?.clone();

        let mut space: usize = 0; // must live as long as main_func, see MappedPages::as_func()
        let main_func = {
            let main_func_sec = main_func_sec_ref.lock();
            let mapped_pages = main_func_sec.mapped_pages.lock();
            mapped_pages.as_func::<MainFunc>(main_func_sec.mapped_pages_offset, &mut space)?
        };

        // build and spawn the actual underlying kernel Task
        let ktb = KernelTaskBuilder::new(*main_func, self.argument)
            .name(self.name.unwrap_or_else(|| app_crate_ref.lock_as_ref().crate_name.clone()));
        
        let ktb = if let Some(core) = self.pin_on_core {
            ktb.pin_on_core(core)
        } else {
            ktb
        };

        #[cfg(simd_personality)]
        let ktb = ktb.simd(self.simd);

        let app_task = ktb.spawn()?;
        app_task.lock_mut().app_crate = Some(app_crate_ref);

        Ok(app_task)
    }

}


#[derive(Debug)]
struct KthreadCall<F, A, R> {
    /// comes from Box::into_raw(Box<A>)
    pub arg: *mut A,
    pub func: F,
    _rettype: PhantomData<R>,
}

impl<F, A, R> KthreadCall<F, A, R> {
    fn new(a: A, f: F) -> KthreadCall<F, A, R> where F: FnOnce(A) -> R {
        KthreadCall {
            arg: Box::into_raw(Box::new(a)),
            func: f,
            _rettype: PhantomData,
        }
    }
}



/// This function sets up the given new Task's stack pointer to properly redirect to given entry point
/// when the new Task is first scheduled in. 
/// 
/// When a new task is first scheduled in, a `Context` struct will be popped off the stack,
/// and at the end of that struct is the address of the next instruction that will be popped off as part of the "ret" instruction, 
/// i.e., the entry point into the new task. 
/// 
/// So, this function allocates space for the saved context registers to be popped off when this task is first switched to.
/// It also sets the given `new_task`'s saved_sp (its saved stack pointer, which holds the Context for task switching).
/// 
fn setup_context_trampoline(kstack: &mut Stack, new_task: &mut Task, entry_point_function: fn() -> !) {
    
    /// A private macro that actually creates the Context and sets it up in the `new_task`.
    /// We use a macro here so we can pass in the proper `ContextType` at runtime, 
    /// which is useful for both the simd_personality config and regular/SSE configs.
    macro_rules! set_context {
        ($ContextType:ty) => (
            let new_context_ptr = (kstack.top_usable() - mem::size_of::<$ContextType>()) as *mut $ContextType;
            // TODO: FIXME: use the MappedPages approach to avoid this unsafe block here
            unsafe {
                *new_context_ptr = <($ContextType)>::new(entry_point_function as usize);
                new_task.saved_sp = new_context_ptr as usize; 
            }
        );
    }


    // If `simd_personality` is enabled, all of the `context_switch*` implementation crates are simultaneously enabled,
    // in order to allow choosing one of them based on the configuration options of each Task (SIMD, regular, etc).
    // If `simd_personality` is NOT enabled, then we use the context_switch routine that matches the actual build target. 
    #[cfg(simd_personality)] {
        match new_task.simd {
            SimdExt::AVX => {
                // warn!("USING AVX CONTEXT for Task {:?}", new_task);
                set_context!(context_switch::ContextAVX);
            }
            SimdExt::SSE => {
                // warn!("USING SSE CONTEXT for Task {:?}", new_task);
                set_context!(context_switch::ContextSSE);
            }
            SimdExt::None => {
                // warn!("USING REGULAR CONTEXT for Task {:?}", new_task);
                set_context!(context_switch::ContextRegular);
            }
        }
    }

    #[cfg(not(simd_personality))] {
        // The context_switch crate exposes the proper TARGET-specific `Context` type here.
        set_context!(context_switch::Context);
    }
}


#[cfg(spawn_userspace)]
/// Spawns a new userspace task based on the provided `path`, which must point to an ELF executable file with a defined entry point.
/// Optionally, provide a `name` for the new Task. If none is provided, the name is based on the given `Path`.
pub fn spawn_userspace(path: Path, name: Option<String>) -> Result<TaskRef, &'static str> {
    return Err("this function has not yet been adapted to use the fs-based crate namespace system");

    debug!("spawn_userspace [0]: Interrupts enabled: {}", interrupts_enabled());
    
    let mut new_task = Task::new();
    new_task.name = String::from(name.unwrap_or(String::from(path.as_ref())));

    let mut ustack: Option<Stack> = None;

    // create a new MemoryManagementInfo instance to represent the new process's address space. 
    let new_userspace_mmi = {
        let kernel_mmi_ref = get_kernel_mmi_ref().expect("spawn_userspace(): KERNEL_MMI was not yet initialized!");
        let mut kernel_mmi_locked = kernel_mmi_ref.lock();
        
        // create a new kernel stack for this userspace task
        let mut kstack: Stack = kernel_mmi_locked.alloc_stack(KERNEL_STACK_SIZE_IN_PAGES).expect("spawn_userspace: couldn't alloc_stack for new kernel stack!");

        setup_context_trampoline(&mut kstack, &mut new_task, userspace_wrapper);
    
        new_task.kstack = Some(kstack);
        // unlike when spawning a kernel task, we don't need to place any arguments at the bottom of the stack,
        // because we can just utilize the task's userspace entry point member


        // destructure the kernel's MMI so we can access its page table and vmas
        let MemoryManagementInfo { 
            page_table: ref mut kernel_page_table, 
            ..  // don't need to access the kernel's VMA list or stack allocator, we already allocated a kstack above
        } = *kernel_mmi_locked;
        
        match kernel_page_table {
            &mut PageTable::Active(ref mut active_table) => {
                
                // get frame allocator reference
                let allocator_mutex = try!(FRAME_ALLOCATOR.try().ok_or("couldn't get FRAME ALLOCATOR"));

                // frame is a single frame, and temp_frames1/2 are tuples of 3 Frames each.
                let (frame, temp_frames1, temp_frames2) = {
                    let mut allocator = allocator_mutex.lock();
                    // a quick closure to allocate one frame
                    let mut alloc_frame = || allocator.allocate_frame().ok_or("couldn't allocate frame"); 
                    (
                        try!(alloc_frame()),
                        (try!(alloc_frame()), try!(alloc_frame()), try!(alloc_frame())),
                        (try!(alloc_frame()), try!(alloc_frame()), try!(alloc_frame()))
                    )
                };

                // now that we have the kernel's active table, we need a new inactive table for the userspace Task
                let mut new_inactive_table: InactivePageTable = {
                    try!(InactivePageTable::new(frame, active_table, TemporaryPage::new(temp_frames1)))
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
                // first we need to temporarily map the module memory region into our address space, 
                // so we can then parse the module as an ELF file in the kernel. (Doesn't need to be USER_ACCESSIBLE). 
                let (elf_progs, entry_point) = {
                    let new_pages = try!(allocate_pages_by_bytes(module.size()).ok_or("couldn't allocate pages for module"));
                    let temp_module_mapping = {
                        let mut allocator = allocator_mutex.lock();
                        try!( active_table.map_allocated_pages_to(
                                  new_pages, Frame::range_inclusive_addr(module.start_address(), module.size()), 
                                  EntryFlags::PRESENT, allocator.deref_mut())
                        )
                    };

                    try!(mod_mgmt::elf_executable::parse_elf_executable(temp_module_mapping, module.size()))
                    
                    // temp_module_mapping is automatically unmapped when it falls out of scope here (frame allocator must not be locked)
                };
                
                let mut new_mapped_pages: Vec<MappedPages> = Vec::new();
                let mut new_user_vmas: Vec<VirtualMemoryArea> = Vec::with_capacity(elf_progs.len() + 2); // doesn't matter, but 2 is for stack and heap

                debug!("spawn_userspace [4]: ELF entry point: {:#x}", entry_point);
                new_task.new_userspace_entry_addr = Some(entry_point);

                // consumes temporary page, which auto unmaps it
                try!( active_table.with(&mut new_inactive_table, TemporaryPage::new(temp_frames2), |mapper| {
                    /*
                        * We need to set the kernel-related entries of our new inactive_table's P4 to the same values used in the kernel's P4.
                        * However, this is done in InactivePageTable::new(), just to make sure a new page table can never be created without including the shared kernel mappings.
                        * Thus, we do not need to handle that here.
                        */


                    // map the userspace module into the new address space.
                    // we can use identity mapping here because we have a higher-half mapped kernel, YAY! :)
                    // debug!("!! mapping userspace module with name: {}", module.name());
                    for prog in elf_progs.iter() {
                        // each program section in the ELF file could be more than one page, but they are contiguous in physical memory
                        debug!("  -- Elf prog: Mapping vaddr {:#x} to paddr {:#x}, size: {:#x}", prog.vma.start_address(), module.start_address() + prog.offset, prog.vma.size());
                        let new_flags = prog.vma.flags() | EntryFlags::USER_ACCESSIBLE;
                        let mapped_pages = {
                            let mut allocator = allocator_mutex.lock();
                            try!(mapper.map_frames(
                                    Frame::range_inclusive_addr(module.start_address() + prog.offset, prog.vma.size()), 
                                    Page::containing_address(prog.vma.start_address()),
                                    new_flags, allocator.deref_mut())
                            )
                        };
                        
                        new_mapped_pages.push(mapped_pages);
                        new_user_vmas.push(VirtualMemoryArea::new(prog.vma.start_address(), prog.vma.size(), new_flags, prog.vma.desc()));
                    }

                    // allocate a new userspace stack
                    let (user_stack, user_stack_vma) = {
                        let mut allocator = allocator_mutex.lock();                        
                        try!( user_stack_allocator.alloc_stack(mapper, allocator.deref_mut(), 16)
                                                  .ok_or("spawn_userspace: couldn't allocate new user stack!")
                        )
                    };
                    ustack = Some(user_stack); 
                    new_user_vmas.push(user_stack_vma);

                    // TODO: give this process a new heap? (assign it a range of virtual addresses but don't alloc phys mem yet)

                    Ok(()) // mapping closure completed successfully

                })); // TemporaryPage is dropped here
                

                // return a new mmi struct (for the new userspace task) to the enclosing scope
                MemoryManagementInfo {
                    page_table: PageTable::Inactive(new_inactive_table),
                    vmas: new_user_vmas,
                    extra_mapped_pages: new_mapped_pages,
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
    new_task.mmi = Some(Arc::new(MutexIrqSafe::new(new_userspace_mmi)));
    new_task.runstate = RunState::Runnable; // ready to be scheduled in
    let new_task_id = new_task.id;

    let task_ref = TaskRef::new(new_task);
    let old_task = TASKLIST.insert(new_task_id, task_ref.clone());
    // insert should return None, because that means there was no other 
    if old_task.is_some() {
        error!("BUG: spawn_userspace(): TASKLIST already contained a task with the new task's ID!");
        return Err("TASKLIST already contained a task with the new task's ID");
    }
    
    runqueue::add_task_to_any_runqueue(task_ref.clone())?;

    Ok(task_ref)
}



/// The entry point for all new `Task`s that run in kernelspace. This does not return!
fn task_wrapper<F, A, R>() -> !
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R, 
{
    let curr_task_ref = get_my_current_task().expect("BUG: task_wrapper(): couldn't get_my_current_task().");
    let curr_task_name = curr_task_ref.lock().name.clone();

    let kthread_call_stack_ptr: *mut KthreadCall<F, A, R> = {
        let t = curr_task_ref.lock();
        let kstack = t.kstack.as_ref().expect("BUG: task_wrapper(): failed to get current task's kstack.");
        // when spawning a kernel task() above, we use the very bottom of the stack to hold the pointer to the kthread_call
        // let off: isize = 0;
        unsafe {
            // dereference it once to get the raw pointer (from the Box<KthreadCall>)
            *(kstack.bottom() as *mut *mut KthreadCall<F, A, R>) as *mut KthreadCall<F, A, R>
        }
    };

    // the pointer to the kthread_call struct (func and arg) was placed on the stack
    let kthread_call: Box<KthreadCall<F, A, R>> = unsafe {
        Box::from_raw(kthread_call_stack_ptr)
    };
    let kthread_call_val: KthreadCall<F, A, R> = *kthread_call;

    let arg: Box<A> = unsafe {
        Box::from_raw(kthread_call_val.arg)
    };
    let func = kthread_call_val.func;
    let arg: A = *arg; 

    
    enable_interrupts(); // we must enable interrupts for the new task, otherwise we won't be able to preempt it.
    compiler_fence(Ordering::SeqCst); // I don't think this is necessary...    
    // debug!("task_wrapper [1]: \"{}\" about to call kthread func {:?} with arg {:?}, interrupts are {}", curr_task_name, debugit!(func), debugit!(arg), interrupts_enabled());

    // Now we're ready to actually invoke the entry point function that this Task was spawned for
    let exit_value = func(arg);

    // debug!("task_wrapper [2]: \"{}\" exited with return value {:?}", curr_task_name, debugit!(exit_value));
    // Here: now that the task is finished running, we must clean in up by doing three things:
    // (1) Put the task into a non-runnable mode (exited), and set its exit value
    // (2) Remove it from its runqueue
    // (3) Yield the CPU
    // The first two need to be done "atomically" (without interruption), so we must disable preemption.
    // Otherwise, this task could be marked as `Exited`, and then a context switch could occur to another task,
    // which would prevent this task from ever running again, so it would never get to remove itself from the runqueue.
    { 
        // (0) disable preemption (currently just disabling interrupts altogether)
        let _held_interrupts = hold_interrupts();
        
        // (1) Put the task into a non-runnable mode (exited), and set its exit value
        if curr_task_ref.exit(Box::new(exit_value)).is_err() {
            warn!("task_wrapper: \"{}\" task could not set exit value, because it had already exited. Is this correct?", curr_task_name);
        }

        // (2) Remove it from its runqueue
        #[cfg(not(runqueue_state_spill_evaluation))]  // the normal case
        {
            if let Err(e) = apic::get_my_apic_id()
                .and_then(|id| runqueue::get_runqueue(id))
                .ok_or("couldn't get this core's ID or runqueue to remove exited task from it")
                .and_then(|rq| rq.write().remove_task(&curr_task_ref)) 
            {
                error!("BUG: task_wrapper(): couldn't remove exited task from runqueue: {}", e);
            }
        }

        // re-enabled preemption here (happens automatically when _held_interrupts is dropped)
    }

    // (3) Yield the CPU
    scheduler::schedule();

    // nothing below here should ever run again, we should never ever reach this point

    error!("BUG: task_wrapper() WAS RESCHEDULED AFTER BEING DEAD!");
    loop { }
}


/// this is invoked by the kernel component of a new userspace task 
/// (using its kernel stack) and jumps to userspace using its userspace stack.
fn userspace_wrapper() -> ! {

    debug!("userspace_wrapper [0]");

    // the things we need to invoke jump_to_userspace
    let ustack_top: usize;
    let entry_func: usize; 

    { // scoped to release current task's lock before calling jump_to_userspace
        let currtask = get_my_current_task().expect("userspace_wrapper(): get_my_current_task() failed").lock();
        ustack_top = currtask.ustack.as_ref().expect("userspace_wrapper(): ustack was None!").top_usable();
        entry_func = currtask.new_userspace_entry_addr.expect("userspace_wrapper(): new_userspace_entry_addr was None!");
    }
    debug!("userspace_wrapper [1]: ustack_top: {:#x}, module_entry: {:#x}", ustack_top, entry_func);

    // SAFE: just jumping to userspace 
    unsafe {
        jump_to_userspace(ustack_top, entry_func);
    }
    // nothing below here should ever run again, we should never ever reach this point


    panic!("userspace_wrapper [end]: jump_to_userspace returned!!!");
}




/// Transitions the currently-running Task from kernel space to userspace.
/// Thus, it should be called from a userspace-ready task wrapper, i.e., `userspace_wrapper()`. 
/// Unsafe because both the stack_ptr and the function_ptr must be valid!
unsafe fn jump_to_userspace(stack_ptr: usize, function_ptr: usize) {
    
    // Steps to jumping to userspace:
    // 1) push stack segment selector (ss), i.e., the user_data segment selector
    // 2) push the userspace stack pointer
    // 3) push rflags, the control flags we wish to use
    // 4) push the code segment selector (cs), i.e., the user_code segment selector
    // 5) push the instruction pointer (rip) for the start of userspace, e.g., the function pointer
    // 6) set other segment registers (ds, es) to the user_data segment, same as (ss)
    // 7) issue iret to return to userspace

    // debug!("Jumping to userspace with stack_ptr: {:#x} and function_ptr: {:#x}",
    //                   stack_ptr, function_ptr);
    // debug!("stack: {:#x} {:#x} func: {:#x}", *(stack_ptr as *const usize), *((stack_ptr - 8) as *const usize), 
    //                 *(function_ptr as *const usize));



    let ss: u16 = get_segment_selector(AvailableSegmentSelector::UserData64).0;
    let cs: u16 = get_segment_selector(AvailableSegmentSelector::UserCode64).0;
    
    // interrupts must be enabled in the rflags for the new userspace task
    let rflags: usize = 1 << 9; // just set the interrupt bit, not the IOPL 
    
    // debug!("jump_to_userspace: rflags = {:#x}, userspace interrupts: {}", rflags, rflags & 0x200 == 0x200);



    asm!("mov ds, $0" : : "r"(ss) : "memory" : "intel", "volatile");
    asm!("mov es, $0" : : "r"(ss) : "memory" : "intel", "volatile");
    // NOTE: we do not set fs and gs here because they're used by the kernel for other purposes

    asm!("push $0" : : "r"(ss as usize) : "memory" : "intel", "volatile");
    asm!("push $0" : : "r"(stack_ptr) : "memory" : "intel", "volatile");
    asm!("push $0" : : "r"(rflags) : "memory" : "intel", "volatile");
    asm!("push $0" : : "r"(cs as usize) : "memory" : "intel", "volatile");
    asm!("push $0" : : "r"(function_ptr) : "memory" : "intel", "volatile");
    
    // Optionally, we can push arguments onto the stack here too.

    // final step, use iret instruction to jump to Ring 3
    asm!("iretq" : : : "memory" : "intel", "volatile");
}
