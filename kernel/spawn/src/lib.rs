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

use core::fmt;
use core::mem;
use core::ops::DerefMut;
use core::sync::atomic::{Ordering, AtomicBool, compiler_fence};
use alloc::{Vec, String};
use alloc::arc::Arc;
use alloc::boxed::Box;


use irq_safety::{MutexIrqSafe, RwLockIrqSafe, enable_interrupts, interrupts_enabled};
use memory::{get_kernel_mmi_ref, PageTable, MappedPages, Stack, ModuleArea, MemoryManagementInfo, Page, VirtualAddress, FRAME_ALLOCATOR, VirtualMemoryArea, FrameAllocator, allocate_pages_by_bytes, TemporaryPage, EntryFlags, InactivePageTable, Frame};
use kernel_config::memory::{KERNEL_STACK_SIZE_IN_PAGES, USER_STACK_ALLOCATOR_BOTTOM, USER_STACK_ALLOCATOR_TOP_ADDR, address_is_page_aligned};
use task::{Task, get_my_current_task, RunState, TASKLIST, CONTEXT_SWITCH_LOCKS, CURRENT_TASKS};
use gdt::{AvailableSegmentSelector, get_segment_selector};


/// Initializes tasking for the given AP core, including creating a runqueue for it
/// and creating its initial idle task for that core. 
pub fn init(kernel_mmi_ref: Arc<MutexIrqSafe<MemoryManagementInfo>>, apic_id: u8,
            stack_bottom: VirtualAddress, stack_top: VirtualAddress) 
            -> Result<Arc<RwLockIrqSafe<Task>>, &'static str> 
{
    CONTEXT_SWITCH_LOCKS.insert(apic_id, AtomicBool::new(false));    

    scheduler::init_runqueue(apic_id);
    
    init_idle_task(kernel_mmi_ref, apic_id, stack_bottom, stack_top)
                .map( |t| t.clone())
}


/// initialize an idle task, of which there is one per processor core/AP/LocalApic.
/// The idle task is a task that runs by default (one per core) when no other task is running.
/// Returns a reference to the `Task`, protected by a `RwLockIrqSafe`
fn init_idle_task(kernel_mmi_ref: Arc<MutexIrqSafe<MemoryManagementInfo>>,
                      apic_id: u8, stack_bottom: VirtualAddress, stack_top: VirtualAddress) 
                      -> Result<Arc<RwLockIrqSafe<Task>>, &'static str> {

    let mut idle_task = Task::new();
    idle_task.name = format!("idle_task_ap{}", apic_id);
    idle_task.runstate = RunState::RUNNABLE;
    idle_task.running_on_cpu = apic_id as isize; 
    idle_task.pinned_core = Some(apic_id); // can only run on this CPU core
    idle_task.mmi = Some(kernel_mmi_ref);
    idle_task.kstack = Some( 
        Stack::new( 
            stack_top, 
            stack_bottom, 
            MappedPages::from_existing(Page::range_inclusive_addr(stack_bottom, stack_top - stack_bottom)),
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



/// Must match the order of registers popped in the nano_core's boot/boot.asm:task_switch
#[derive(Default, Debug)]
#[repr(C, packed)]
pub struct Context {
    r15: usize, 
    r14: usize,
    r13: usize,
    r12: usize,
    rbp: usize,
    rbx: usize,
    rip: usize,
}

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


#[derive(Debug)]
struct KthreadCall<A, R> {
    /// comes from Box::into_raw(Box<A>)
    pub arg: *mut A,
    pub func: fn(arg: A) -> R,
}

impl<A, R> KthreadCall<A, R> {
    fn new(a: A, f: fn(arg: A) -> R) -> KthreadCall<A, R> {
        KthreadCall {
            arg: Box::into_raw(Box::new(a)),
            func: f,
        }
    }
}



/// Spawns a new kernel task with the same address space as the current task. 
/// The new kernel thread is set up to enter the given function `func` and passes it the arguments `arg`.
/// This merely makes the new task Runanble, it does not context switch to it immediately. That will happen on the next scheduler invocation.
#[inline(never)]
pub fn spawn_kthread<A: fmt::Debug, R: fmt::Debug>(func: fn(arg: A) -> R, arg: A, thread_name: String)
        -> Result<Arc<RwLockIrqSafe<Task>>, &'static str> {

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
        *new_context_ptr = Context::new(kthread_wrapper::<A, R> as usize);
        new_task.saved_sp = new_context_ptr as usize; 
        debug!("spawn_kthread(): new_context: {:#X} --> {:?}", new_context_ptr as usize, *new_context_ptr);
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


    new_task.kstack = Some(kstack);
    new_task.runstate = RunState::RUNNABLE; // ready to be scheduled in

    let new_task_id = new_task.id;
    let task_ref = Arc::new(RwLockIrqSafe::new(new_task));
    let old_task = TASKLIST.insert(new_task_id, task_ref.clone());
    // insert should return None, because that means there was no other 
    if old_task.is_some() {
        error!("spawn_kthread(): Fatal Error: TASKLIST already contained a task with the new task's ID!");
        return Err("TASKLIST already contained a task with the new task's ID");
    }
    
    try!(scheduler::add_task_to_runqueue(task_ref.clone()));

    Ok(task_ref)
}


/// Spawns a new userspace task based on the provided `ModuleArea`, which should have an entry point called `main`.
/// optionally, provide a `name` for the new Task. If none is provided, the name from the given `ModuleArea` is used.
pub fn spawn_userspace(module: &ModuleArea, name: Option<String>) -> Result<Arc<RwLockIrqSafe<Task>>, &'static str> {

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
            debug!("spawn_userspace(): new_context: {:#X} --> {:?}", new_context_ptr as usize, *new_context_ptr);
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

                let (frame, temp_frames1, temp_frames2) = {
                    let mut allocator = allocator_mutex.lock();
                    (
                        try!(allocator.allocate_frame().ok_or("couldn't allocate frame")),
                        [allocator.allocate_frame(), allocator.allocate_frame(), allocator.allocate_frame()],
                        [allocator.allocate_frame(), allocator.allocate_frame(), allocator.allocate_frame()]
                    )
                };

                // now that we have the kernel's active table, we need a new inactive table for the userspace Task
                let mut new_inactive_table: InactivePageTable = {
                    InactivePageTable::new(frame, active_table, TemporaryPage::new(temp_frames1))
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

                    try!(mod_mgmt::parse_elf_executable(temp_module_mapping.start_address() as VirtualAddress, module.size()))
                    
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
    new_task.runstate = RunState::RUNNABLE; // ready to be scheduled in
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
pub fn remove_task(_id: usize) -> Option<Arc<RwLockIrqSafe<Task>>> {
    unimplemented!();
// assert!(get_task(id).unwrap().runstate == Runstate::Exited, "A task must be exited before it can be removed from the TASKLIST!");
    // TASKLIST.remove(id)
}



/// this does not return
fn kthread_wrapper<A: fmt::Debug, R: fmt::Debug>() -> ! {

    let kthread_call_stack_ptr: *mut KthreadCall<A, R>;
    {
        let currtask = get_my_current_task().expect("kthread_wrapper(): get_my_current_task() failed in getting kstack").read();
        let kstack = currtask.kstack.as_ref().expect("kthread_wrapper(): failed to get current task's kstack.");
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
    let arg: A = *arg; 
    // debug!("kthread_wrapper [0.1]: arg {:?}", arg);
    // debug!("kthread_wrapper [0.2]: func {:?}", func);

    enable_interrupts();
    compiler_fence(Ordering::SeqCst); // I don't think this is necessary...    
    info!("kthread_wrapper(): about to call kthread func {:?} with arg {:?}, interrupts are {}", func, arg, interrupts_enabled());
    assert!(interrupts_enabled(), "FATAL ERROR: kthread_wrapper(): interrupts did not get enabled properly!");

    // actually invoke the function spawned in this kernel thread
    let exit_status = func(arg);

    debug!("kthread_wrapper [2]: exited with return value {:?}", exit_status);

    // cleanup current thread: put it into non-runnable mode, save exit status
    {
        get_my_current_task().expect("kthread_wrapper(): couldn't get_my_current_task() after kthread func returned.")
                             .write().set_runstate(RunState::EXITED);
    }

    scheduler::schedule();

    // we should never ever reach this point
    panic!("KTHREAD_WRAPPER WAS RESCHEDULED AFTER BEING DEAD!")
}


/// this is invoked by the kernel component of a new userspace task 
/// (using its kernel stack) and jumps to userspace using its userspace stack.
fn userspace_wrapper() -> ! {

    debug!("userspace_wrapper [0]");

    // the things we need to invoke jump_to_userspace
    let ustack_top: usize;
    let entry_func: usize; 

    { // scoped to release tasklist lock before calling jump_to_userspace
        let currtask = get_my_current_task().expect("userspace_wrapper(): get_my_current_task() failed").write();
        ustack_top = currtask.ustack.as_ref().expect("userspace_wrapper(): ustack was None!").top_usable();
        entry_func = currtask.new_userspace_entry_addr.expect("userspace_wrapper(): new_userspace_entry_addr was None!");
    }
    debug!("userspace_wrapper [1]: ustack_top: {:#x}, module_entry: {:#x}", ustack_top, entry_func);

    // SAFE: just jumping to userspace 
    unsafe {
        jump_to_userspace(ustack_top, entry_func);
    }


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
