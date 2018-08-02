#![no_std]
#![feature(alloc)]
#![feature(asm)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate atomic_linked_list;
extern crate memory;
extern crate kernel_config;
extern crate task;
extern crate scheduler;
extern crate mod_mgmt;
extern crate gdt;
extern crate owning_ref;
#[macro_use] extern crate debugit;

use core::mem;
use core::marker::PhantomData;
use core::ops::DerefMut;
use core::sync::atomic::{Ordering, AtomicBool, compiler_fence};
use alloc::{Vec, String};
use alloc::arc::Arc;
use alloc::boxed::Box;


use irq_safety::{MutexIrqSafe, RwLockIrqSafe, enable_interrupts, interrupts_enabled};
use memory::{get_kernel_mmi_ref, PageTable, MappedPages, Stack, ModuleArea, MemoryManagementInfo, Page, VirtualAddress, FRAME_ALLOCATOR, VirtualMemoryArea, FrameAllocator, allocate_pages_by_bytes, TemporaryPage, EntryFlags, InactivePageTable, Frame};
use kernel_config::memory::{KERNEL_STACK_SIZE_IN_PAGES, USER_STACK_ALLOCATOR_BOTTOM, USER_STACK_ALLOCATOR_TOP_ADDR, address_is_page_aligned};
use task::{Task, TaskRef, get_my_current_task, RunState, TASKLIST, CONTEXT_SWITCH_LOCKS, CURRENT_TASKS};
use gdt::{AvailableSegmentSelector, get_segment_selector};


/// Initializes tasking for the given AP core, including creating a runqueue for it
/// and creating its initial idle task for that core. 
pub fn init(kernel_mmi_ref: Arc<MutexIrqSafe<MemoryManagementInfo>>, apic_id: u8,
            stack_bottom: VirtualAddress, stack_top: VirtualAddress) 
            -> Result<TaskRef, &'static str> 
{
    CONTEXT_SWITCH_LOCKS.insert(apic_id, AtomicBool::new(false));    

    scheduler::init_runqueue(apic_id);
    
    init_idle_task(kernel_mmi_ref, apic_id, stack_bottom, stack_top)
                .map( |t| t.clone())
}


/// initialize an idle task, of which there is one per processor core/AP/LocalApic.
/// The idle task is a task that runs by default (one per core) when no other task is running.
/// 
/// Returns a reference to the `Task`, protected by a `RwLockIrqSafe`
fn init_idle_task(kernel_mmi_ref: Arc<MutexIrqSafe<MemoryManagementInfo>>,
                      apic_id: u8, stack_bottom: VirtualAddress, stack_top: VirtualAddress) 
                      -> Result<TaskRef, &'static str> {

    let mut idle_task = Task::new();
    idle_task.name = format!("idle_task_ap{}", apic_id);
    idle_task.is_an_idle_task = true;
    idle_task.runstate = RunState::Runnable;
    idle_task.running_on_cpu = apic_id as isize; 
    idle_task.pinned_core = Some(apic_id); // can only run on this CPU core
    idle_task.mmi = Some(kernel_mmi_ref);
    idle_task.kstack = Some( 
        Stack::new( 
            stack_top, 
            stack_bottom, 
            MappedPages::from_existing(
                Page::range_inclusive_addr(stack_bottom, stack_top - stack_bottom),
                EntryFlags::WRITABLE | EntryFlags::PRESENT
            ),
        )
    );
    debug!("IDLE TASK STACK (apic {}) at bottom={:#x} - top={:#x} ", apic_id, stack_bottom, stack_top);
    let idle_task_id = idle_task.id;

    // set this as this core's current task, since it's obviously running
    CURRENT_TASKS.insert(apic_id, idle_task_id); 


    let task_ref = Arc::new(RwLockIrqSafe::new(idle_task));
    let old_task = TASKLIST.insert(idle_task_id, task_ref.clone());
    // insert should return None, because that means there was no other 
    if old_task.is_some() {
        error!("init_idle_task(): Fatal Error: TASKLIST already contained a task with the same id {} as idle_task_ap{}!", idle_task_id, apic_id);
        return Err("TASKLIST already contained a task with the new idle_task's ID");
    }

    try!(scheduler::add_task_to_specific_runqueue(apic_id, task_ref.clone()));

    Ok(task_ref)
}



#[cfg(not(target_feature = "sse2"))]
#[repr(C, packed)]
/// Must match the order of registers popped in the [`task`](../task/index.html) crate's `task_switch`
pub struct Context {
    r15: usize, 
    r14: usize,
    r13: usize,
    r12: usize,
    rbp: usize,
    rbx: usize,
    rip: usize,
}

#[cfg(not(target_feature = "sse2"))]
impl Context {
    pub fn new(rip: usize) -> Context {
        Context {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            rbp: 0,
            rbx: 0,
            rip: rip,
        }
    }
}


#[cfg(target_feature = "sse2")]
#[repr(C, packed)]
/// Must match the order of registers popped in the [`task`](../task/index.html) crate's `task_switch`
pub struct Context {
    xmm15: u128,
    xmm14: u128,   
    xmm13: u128,   
    xmm12: u128,   
    xmm11: u128,   
    xmm10: u128,   
    xmm9:  u128,   
    xmm8:  u128,   
    xmm7:  u128,   
    xmm6:  u128,   
    xmm5:  u128,   
    xmm4:  u128,   
    xmm3:  u128,   
    xmm2:  u128,   
    xmm1:  u128,   
    xmm0:  u128, 

    r15: usize, 
    r14: usize,
    r13: usize,
    r12: usize,
    rbp: usize,
    rbx: usize,
    rip: usize,
}

#[cfg(target_feature = "sse2")]
impl Context {
    pub fn new(rip: usize) -> Context {
        Context {
            xmm15: 0,
            xmm14: 0,   
            xmm13: 0,   
            xmm12: 0,   
            xmm11: 0,   
            xmm10: 0,   
            xmm9:  0,   
            xmm8:  0,   
            xmm7:  0,   
            xmm6:  0,   
            xmm5:  0,   
            xmm4:  0,   
            xmm3:  0,   
            xmm2:  0,   
            xmm1:  0,   
            xmm0:  0,   

            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            rbp: 0,
            rbx: 0,
            rip: rip,
        }
    }
}



#[derive(Debug)]
struct KthreadCall<A, R, F> {
    /// comes from Box::into_raw(Box<A>)
    pub arg: *mut A,
    pub func: F,
    _rettype: PhantomData<R>,
}

impl<A, R, F> KthreadCall<A, R, F> {
    fn new(a: A, f: F) -> KthreadCall<A, R, F> where F: FnOnce(A) -> R {
        KthreadCall {
            arg: Box::into_raw(Box::new(a)),
            func: f,
            _rettype: PhantomData,
        }
    }
}



/// Spawns a new kernel task with the same address space as the current task. 
/// The new kernel thread is set up to enter the given function `func` and passes it the arguments `arg`.
/// This merely makes the new task Runnable, it does not context switch to it immediately. That will happen on the next scheduler invocation.
/// 
/// # Arguments
/// 
/// * `func`: the function or closure that will be invoked in the new task.
/// * `arg`: the argument to the function `func`. It must be a type that implements the Send trait, i.e., not a borrowed reference.
/// * `thread_name`: the String name of the new task.
/// * `pin_on_core`: the core number that this task will be permanently scheduled onto, or if None, the "least busy" core will be chosen.
/// 
#[inline(never)]
pub fn spawn_kthread<A, R, F>(func: F, arg: A, thread_name: String, pin_on_core: Option<u8>)
    -> Result<TaskRef, &'static str> 
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R, 
{
    let mut new_task = Task::new();
    new_task.set_name(thread_name);

    // the new kernel thread uses the same kernel address space
    new_task.mmi = Some( try!(get_kernel_mmi_ref().ok_or("spawn_kthread(): KERNEL_MMI was not initialized!!")) );

    // create and set up a new kstack
    let kstack: Stack = {
        let mut mmi = try!(new_task.mmi.as_mut().ok_or("spawn_kthread: new_task.mmi was None!")).lock();
        try!(mmi.alloc_stack(KERNEL_STACK_SIZE_IN_PAGES).ok_or("spawn_kthread: couldn't allocate kernel stack!"))
    };

    // When this new task is scheduled in, a `Context` struct be popped off the stack,
    // and then at the end of that struct is the next instruction that will be popped off as part of the "ret" instruction. 
    // So we need to allocate space for the saved context registers to be popped off when this task is switch to.
    let new_context_ptr = (kstack.top_usable() - mem::size_of::<Context>()) as *mut Context;
    unsafe {
        *new_context_ptr = Context::new(kthread_wrapper::<A, R, F> as usize);
        new_task.saved_sp = new_context_ptr as usize; 
    }

    // set up the kthread stuff
    let kthread_call = Box::new( KthreadCall::new(arg, func) );
    debug!("Creating kthread_call: {:?}", debugit!(kthread_call));


    // currently we're using the very bottom of the kstack for kthread arguments
    let arg_ptr = kstack.bottom();
    let kthread_ptr: *mut KthreadCall<A, R, F> = Box::into_raw(kthread_call);  // consumes the kthread_call Box!
    unsafe {
        *(arg_ptr as *mut _) = kthread_ptr; // as *mut KthreadCall<A, R>; // as usize;
        debug!("checking kthread_call: arg_ptr={:#x} *arg_ptr={:#x} kthread_ptr={:#x} {:?}", arg_ptr as usize, *(arg_ptr as *const usize) as usize, kthread_ptr as usize, debugit!(*kthread_ptr));
    }


    new_task.kstack = Some(kstack);
    new_task.runstate = RunState::Runnable; // ready to be scheduled in

    let new_task_id = new_task.id;
    let task_ref = Arc::new(RwLockIrqSafe::new(new_task));
    let old_task = TASKLIST.insert(new_task_id, task_ref.clone());
    // insert should return None, because that means there was no other 
    if old_task.is_some() {
        error!("spawn_kthread(): Fatal Error: TASKLIST already contained a task with the new task's ID!");
        return Err("TASKLIST already contained a task with the new task's ID");
    }
    
    if let Some(core) = pin_on_core {
        try!(scheduler::add_task_to_specific_runqueue(core, task_ref.clone()));
    }
    else {
        try!(scheduler::add_task_to_runqueue(task_ref.clone()));
    }

    Ok(task_ref)
}



type MainFuncSignature = fn(Vec<String>) -> isize;


/// Spawns a new application task that runs in kernel mode (currently the only way to run applications), 
/// based on the provided `ModuleArea` which is an object file that must have an entry point called `main`.
/// The new application `Task` is set up to enter the `main` function with the arguments `args`.
/// This merely makes the new task Runnable, it does not context switch to it immediately. That will happen on the next scheduler invocation.
/// 
/// This is similar (but not identical) to the `exec()` system call in POSIX environments. 
/// 
/// # Arguments
/// 
/// * `module`: the [`ModuleArea`](../memory/ModuleArea.t.html) that will be loaded and its main function invoked in the new `Task`.
/// * `args`: the arguments that will be passed to the `main` function of the application. 
/// * `task_name`: the String name of the new task. If None, the `module`'s crate name will be used. 
/// * `pin_on_core`: the core number that this task will be permanently scheduled onto, or if None, the "least busy" core will be chosen.
pub fn spawn_application(module: &ModuleArea, args: Vec<String>, task_name: Option<String>, pin_on_core: Option<u8>)
    -> Result<TaskRef, &'static str> 
{
    spawn_application_internal(module, args, task_name, pin_on_core, false)
}


/// Similar to [`spawn_application`](#method.spawn_application), but adds the newly-spanwed application's public symbols 
/// to the default namespace's symbol map, which allows other applications to depend upon it. 
/// This also prevents this application from being re-loaded again, making it a system-wide singleton that cannot be duplicated.
/// 
/// In general, for regular applications, you should likely use [`spawn_application`](#method.spawn_application).
pub fn spawn_application_singleton(module: &ModuleArea, args: Vec<String>, task_name: Option<String>, pin_on_core: Option<u8>)
    -> Result<TaskRef, &'static str> 
{
    spawn_application_internal(module, args, task_name, pin_on_core, true)
}



/// The internal routine for spawning a new application task that runs in kernel mode (currently the only way to run applications), 
/// based on the provided `ModuleArea` which is an object file that must have an entry point called `main`.
/// The new application `Task` is set up to enter the `main` function with the arguments `args`.
/// This merely makes the new task Runnable, it does not context switch to it immediately. That will happen on the next scheduler invocation.
/// 
/// This is similar (but not identical) to the `exec()` system call in POSIX environments. 
/// 
/// # Arguments
/// 
/// * `module`: the [`ModuleArea`](../memory/ModuleArea.t.html) that will be loaded and its main function invoked in the new `Task`.
/// * `args`: the arguments that will be passed to the `main` function of the application. 
/// * `task_name`: the String name of the new task. If None, the `module`'s crate name will be used. 
/// * `pin_on_core`: the core number that this task will be permanently scheduled onto, or if None, the "least busy" core will be chosen.
/// * `is_singleton`: if true, adds this application's public symbols to the default namespace's symbol map, which allows other applications to depend upon it,
///    and prevents this application from being re-loaded again, making it a system-wide singleton that cannot be duplicated.
fn spawn_application_internal(module: &ModuleArea, args: Vec<String>, task_name: Option<String>, pin_on_core: Option<u8>, is_singleton: bool)
    -> Result<TaskRef, &'static str> 
{
    let app_crate_ref = {
        let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("couldn't get_kernel_mmi_ref")?;
        let mut kernel_mmi = kernel_mmi_ref.lock();
        mod_mgmt::get_default_namespace().load_application_crate(module, kernel_mmi.deref_mut(), is_singleton, false)?
    };

    // get the LoadedSection for the "main" function in the app_crate
    let main_func_sec_ref = app_crate_ref.lock_as_ref().get_function_section("main")
        .ok_or("spawn_application(): couldn't find \"main\" function!")?;

    let mut space: usize = 0; // must live as long as main_func, see MappedPages::as_func()
    let main_func = {
        let main_func_sec = main_func_sec_ref.lock();
        let mapped_pages = main_func_sec.mapped_pages.lock();
        mapped_pages.as_func::<MainFuncSignature>(main_func_sec.mapped_pages_offset, &mut space)?
    };

    let task_name = task_name.unwrap_or_else(|| app_crate_ref.lock_as_ref().crate_name.clone());
    let app_task = spawn_kthread(*main_func, args, task_name, pin_on_core)?;
    app_task.write().app_crate = Some(app_crate_ref);

    Ok(app_task)
}



/// Spawns a new userspace task based on the provided `ModuleArea`, which must be an ELF executable file with a defined entry point.
/// Optionally, provide a `name` for the new Task. If none is provided, the name from the given `ModuleArea` is used.
pub fn spawn_userspace(module: &ModuleArea, name: Option<String>) -> Result<TaskRef, &'static str> {

    debug!("spawn_userspace [0]: Interrupts enabled: {}", interrupts_enabled());
    
    let mut new_task = Task::new();
    new_task.set_name(String::from(name.unwrap_or(module.name().clone())));

    let mut ustack: Option<Stack> = None;

    // create a new MemoryManagementInfo instance to represent the new process's address space. 
    let new_userspace_mmi = {
        let kernel_mmi_ref = get_kernel_mmi_ref().expect("spawn_userspace(): KERNEL_MMI was not yet initialized!");
        let mut kernel_mmi_locked = kernel_mmi_ref.lock();
        
        // create a new kernel stack for this userspace task
        let kstack: Stack = kernel_mmi_locked.alloc_stack(KERNEL_STACK_SIZE_IN_PAGES).expect("spawn_userspace: couldn't alloc_stack for new kernel stack!");
        // allocate space for the saved context registers to be popped off when this task is switch to.
        let new_context_ptr = (kstack.top_usable() - mem::size_of::<Context>()) as *mut Context;
        unsafe {
            // when this new task is scheduled in, we want it to jump to the userspace_wrapper, which will then make the jump to actual userspace
            *new_context_ptr = Context::new(userspace_wrapper as usize);
            new_task.saved_sp = new_context_ptr as usize; 
        }
    
        new_task.kstack = Some(kstack);
        // unlike spawn_kthread, we don't need to place any arguments at the bottom of the stack,
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

    let task_ref = Arc::new(RwLockIrqSafe::new(new_task));
    let old_task = TASKLIST.insert(new_task_id, task_ref.clone());
    // insert should return None, because that means there was no other 
    if old_task.is_some() {
        error!("spawn_userspace(): Fatal Error: TASKLIST already contained a task with the new task's ID!");
        return Err("TASKLIST already contained a task with the new task's ID");
    }
    
    try!(scheduler::add_task_to_runqueue(task_ref.clone()));

    Ok(task_ref)
}



/// Remove a task from the list.
///
/// ## Parameters
/// - `id`: the TaskId to be removed.
///
/// ## Returns
/// An Option with a reference counter for the removed Task.
pub fn remove_task(_id: usize) -> Option<TaskRef> {
    unimplemented!();
// assert!(get_task(id).unwrap().runstate == Runstate::Exited, "A task must be exited before it can be removed from the TASKLIST!");
    // TASKLIST.remove(id)
}




/// Waits until the given `task` has finished executing, 
/// i.e., blocks until its runstate is `RunState::Exited`.
/// Returns `Ok()` when the given `task` is actually exited,
/// and `Err()` if there is a problem or interruption while waiting for it to exit. 
/// # Note
/// * You cannot call `join()` on the current thread, because a thread cannot wait for itself to finish running. 
/// This will result in an `Err()` being immediately returned.
/// * You cannot call `join()` with interrupts disabled, because it will result in permanent deadlock
/// (well, this is only true if the requested `task` is running on the same cpu...  but good enough for now).
pub fn join(task: &TaskRef) -> Result<(), &'static str> {
    let curr_task = get_my_current_task().ok_or("join(): failed to check what current task is")?;
    if Arc::ptr_eq(task, curr_task) {
        return Err("BUG: cannot call join() on yourself (the current task).");
    }

    if !interrupts_enabled() {
        return Err("BUG: cannot call join() with interrupts disabled; it will cause deadlock.")
    }

    
    // First, wait for this Task to be marked as Exited (no longer runnable).
    loop {
        if let RunState::Exited(_) = task.read().runstate {
            break;
        }
    }

    // Then, wait for it to actually stop running on any CPU core.
    loop {
        let t = task.read();
        if !t.is_running() {
            return Ok(());
        }
    }
}



/// The entry point for all new kernel `Task`s. This does not return!
fn kthread_wrapper<A, R, F>() -> !
    where A: Send + 'static, 
          R: Send + 'static,
          F: FnOnce(A) -> R, 
{
    let curr_task_ref = get_my_current_task().expect("kthread_wrapper(): couldn't get_my_current_task().");
    let curr_task_name = curr_task_ref.read().name.clone();

    let kthread_call_stack_ptr: *mut KthreadCall<A, R, F> = {
        let t = curr_task_ref.read();
        let kstack = t.kstack.as_ref().expect("kthread_wrapper(): failed to get current task's kstack.");
        // in spawn_kthread() above, we use the very bottom of the stack to hold the pointer to the kthread_call
        // let off: isize = 0;
        unsafe {
            // dereference it once to get the raw pointer (from the Box<KthreadCall>)
            *(kstack.bottom() as *mut *mut KthreadCall<A, R, F>) as *mut KthreadCall<A, R, F>
        }
    };

    // the pointer to the kthread_call struct (func and arg) was placed on the stack
    let kthread_call: Box<KthreadCall<A, R, F>> = unsafe {
        Box::from_raw(kthread_call_stack_ptr)
    };
    let kthread_call_val: KthreadCall<A, R, F> = *kthread_call;

    let arg: Box<A> = unsafe {
        Box::from_raw(kthread_call_val.arg)
    };
    let func = kthread_call_val.func;
    let arg: A = *arg; 

    
    enable_interrupts();
    compiler_fence(Ordering::SeqCst); // I don't think this is necessary...    
    debug!("kthread_wrapper [1]: \"{}\" about to call kthread func {:?} with arg {:?}, interrupts are {}", curr_task_name, debugit!(func), debugit!(arg), interrupts_enabled());

    // actually invoke the function spawned in this kernel thread
    let exit_value = func(arg);

    // cleanup current thread: put it into non-runnable mode, save exit status

    debug!("kthread_wrapper [2]: \"{}\" exited with return value {:?}", curr_task_name, debugit!(exit_value));

    if curr_task_ref.write().exit(Box::new(exit_value)).is_err() {
        warn!("kthread_wrapper \"{}\" task could not set exit value, because it had already exited. Is this correct?", curr_task_name);
    }

    scheduler::schedule();
    // nothing below here should ever run again, we should never ever reach this point

    panic!("KTHREAD_WRAPPER WAS RESCHEDULED AFTER BEING DEAD!")
}


/// this is invoked by the kernel component of a new userspace task 
/// (using its kernel stack) and jumps to userspace using its userspace stack.
fn userspace_wrapper() -> ! {

    debug!("userspace_wrapper [0]");

    // the things we need to invoke jump_to_userspace
    let ustack_top: usize;
    let entry_func: usize; 

    { // scoped to release current task's RwLock before calling jump_to_userspace
        let currtask = get_my_current_task().expect("userspace_wrapper(): get_my_current_task() failed").write();
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
    // 6) set all other segment registers (ds, es, fs, gs) to the user_data segment, same as (ss)
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
    //asm!("mov fs, $0" : : "r"(ss) : "memory" : "intel", "volatile");


    asm!("push $0" : : "r"(ss as usize) : "memory" : "intel", "volatile");
    asm!("push $0" : : "r"(stack_ptr) : "memory" : "intel", "volatile");
    asm!("push $0" : : "r"(rflags) : "memory" : "intel", "volatile");
    asm!("push $0" : : "r"(cs as usize) : "memory" : "intel", "volatile");
    asm!("push $0" : : "r"(function_ptr) : "memory" : "intel", "volatile");
    
    // Optionally, we can push arguments onto the stack here too.

    // final step, use iret instruction to jump to Ring 3
    asm!("iretq" : : : "memory" : "intel", "volatile");
}
