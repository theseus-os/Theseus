//! This crate contains code for creating userspace tasks, which was previously in the `spawn`.

#![no_std]
#![feature(asm)]
#![feature(stmt_expr_attributes)]

#[macro_use] extern crate alloc;
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
extern crate fs_node;
extern crate catch_unwind;


use core::{
    mem,
    marker::PhantomData,
    sync::atomic::{Ordering, AtomicBool, compiler_fence},
};
use alloc::{
    vec::Vec,
    string::String,
    sync::Arc,
    boxed::Box,
};
use irq_safety::{MutexIrqSafe, hold_interrupts, enable_interrupts};
use memory::{get_kernel_mmi_ref, MemoryManagementInfo, VirtualAddress};
use task::{Task, TaskRef, get_my_current_task, RunState, TASKLIST};
use mod_mgmt::{CrateNamespace, SectionType, SECTION_HASH_DELIMITER};
use path::Path;
use fs_node::FileOrDir;

#[cfg(spawn_userspace)]
use core::ops::DerefMut;
#[cfg(spawn_userspace)]
use memory::{PageTable, MappedPages, Page, get_frame_allocator_ref, VirtualMemoryArea, FrameAllocator, allocate_pages_by_bytes, TemporaryPage, EntryFlags, Frame};
#[cfg(spawn_userspace)]
use kernel_config::memory::{USER_STACK_ALLOCATOR_BOTTOM, USER_STACK_ALLOCATOR_TOP_ADDR};
#[cfg(spawn_userspace)]
use gdt::{AvailableSegmentSelector, get_segment_selector};


#[cfg(spawn_userspace)]
/// Spawns a new userspace task based on the provided `path`, which must point to an ELF executable file with a defined entry point.
/// Optionally, provide a `name` for the new Task. If none is provided, the name is based on the given `Path`.
pub fn spawn_userspace(path: Path, name: Option<String>) -> Result<TaskRef, &'static str> {
    return Err("this function has not yet been adapted to use the fs-based crate namespace system");

    debug!("spawn_userspace [0]: Interrupts enabled: {}", irq_safety::interrupts_enabled());
    
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


        
        // get frame allocator reference
        let allocator_mutex = get_frame_allocator_ref().ok_or("couldn't get FRAME ALLOCATOR")?;

        // new_frame is a single frame, and temp_frames1/2 are tuples of 3 Frames each.
        let (new_frame, temp_frames1, temp_frames2) = {
            let mut allocator = allocator_mutex.lock();
            // a quick closure to allocate one frame
            let mut alloc_frame = || allocator.allocate_frame().ok_or("couldn't allocate frame"); 
            (
                alloc_frame()?,
                (alloc_frame()?, alloc_frame()?, alloc_frame()?),
                (alloc_frame()?, alloc_frame()?, alloc_frame()?)
            )
        };

        // now that we have the kernel's active table, we need a totally new page table for the userspace Task
        let mut new_page_table = PageTable::new_table(&mut kernel_mmi_locked.page_table, new_frame, TemporaryPage::new(temp_frames1))?;

        // create a new stack allocator for this userspace process
        let mut user_stack_allocator = {
            use memory::StackAllocator;
            let stack_alloc_start = Page::containing_address(USER_STACK_ALLOCATOR_BOTTOM); 
            let stack_alloc_end = Page::containing_address(USER_STACK_ALLOCATOR_TOP_ADDR);
            let stack_alloc_range = PageRange::new(stack_alloc_start, stack_alloc_end);
            StackAllocator::new(stack_alloc_range, true) // true means it's for userspace
        };

        // set up the userspace module flags/vma, the actual mapping happens in the .with() closure below 
        if module.start_address().frame_offset() != 0 {
            return Err("modules must be page aligned!");
        }
        // first we need to temporarily map the module memory region into our address space, 
        // so we can then parse the module as an ELF file in the kernel. (Doesn't need to be USER_ACCESSIBLE). 
        let (elf_progs, entry_point) = {
            let new_pages = allocate_pages_by_bytes(module.size()).ok_or("couldn't allocate pages for module")?;
            let temp_module_mapping = {
                let mut allocator = allocator_mutex.lock();
                kernel_mmi_locked.page_table.map_allocated_pages_to(
                    new_pages, FrameRange::from_phys_addr(module.start_address(), module.size()), 
                    EntryFlags::PRESENT, allocator.deref_mut()
                )?
            };

            elf_executable::parse_elf_executable(temp_module_mapping, module.size())?
            
            // temp_module_mapping is automatically unmapped when it falls out of scope here (frame allocator must not be locked)
        };
        
        let mut new_mapped_pages: Vec<MappedPages> = Vec::new();
        let mut new_user_vmas: Vec<VirtualMemoryArea> = Vec::with_capacity(elf_progs.len() + 2); // doesn't matter, but 2 is for stack and heap

        debug!("spawn_userspace [4]: ELF entry point: {:#x}", entry_point);
        new_task.new_userspace_entry_addr = Some(entry_point);

        kernel_mmi_locked.page_table.with(&mut new_page_table, TemporaryPage::new(temp_frames2), |mapper| {
            // Note that in PageTable::new_table(), the shared kernel mappings have already been copied over to the `new_page_table`,
            // to ensure that a new page table can never be created without including the shared kernel mappings.
            // Thus, we do not need to handle that here.

            // map the userspace module into the new address space.
            // we can use identity mapping here because we have a higher-half mapped kernel, YAY! :)
            // debug!("!! mapping userspace module with name: {}", module.name());
            for prog in elf_progs.iter() {
                // each program section in the ELF file could be more than one page, but they are contiguous in physical memory
                debug!("  -- Elf prog: Mapping vaddr {:#x} to paddr {:#x}, size: {:#x}", prog.vma.start_address(), module.start_address() + prog.offset, prog.vma.size());
                let new_flags = prog.vma.flags() | EntryFlags::USER_ACCESSIBLE;
                let mapped_pages = {
                    let mut allocator = allocator_mutex.lock();
                    mapper.map_frames(
                        FrameRange::from_phys_addr(module.start_address() + prog.offset, prog.vma.size()), 
                        Page::containing_address(prog.vma.start_address()),
                        new_flags, allocator.deref_mut()
                    )?
                };
                
                new_mapped_pages.push(mapped_pages);
                new_user_vmas.push(VirtualMemoryArea::new(prog.vma.start_address(), prog.vma.size(), new_flags, prog.vma.desc()));
            }

            // allocate a new userspace stack
            let (user_stack, user_stack_vma) = {
                let mut allocator = allocator_mutex.lock();                        
                user_stack_allocator.alloc_stack(mapper, allocator.deref_mut(), 16)
                    .ok_or("spawn_userspace: couldn't allocate new user stack!")?
            };
            ustack = Some(user_stack); 
            new_user_vmas.push(user_stack_vma);

            // TODO: give this process a new heap? (assign it a range of virtual addresses but don't alloc phys mem yet)

            Ok(()) // mapping closure completed successfully

        })?; // TemporaryPage is dropped here
        

        // return a new mmi struct (for the new userspace task) to the enclosing scope
        MemoryManagementInfo {
            page_table: new_page_table,
            vmas: new_user_vmas,
            extra_mapped_pages: new_mapped_pages,
            stack_allocator: user_stack_allocator,
        }
    };

    assert!(ustack.is_some(), "spawn_userspace(): ustack was None after trying to alloc_stack!");
    new_task.ustack = ustack;
    new_task.mmi = Some(Arc::new(MutexIrqSafe::new(new_userspace_mmi)));
    new_task.runstate = RunState::Runnable; // ready to be scheduled in
    let new_task_id = new_task.id;

    let task_ref = TaskRef::create(new_task);
    let old_task = TASKLIST.insert(new_task_id, task_ref.clone());
    // insert should return None, because that means there was no other 
    if old_task.is_some() {
        error!("BUG: spawn_userspace(): TASKLIST already contained a task with the new task's ID!");
        return Err("TASKLIST already contained a task with the new task's ID");
    }
    
    runqueue::add_task_to_any_runqueue(task_ref.clone())?;

    Ok(task_ref)
}


#[cfg(spawn_userspace)]
/// this is invoked by the kernel component of a new userspace task 
/// (using its kernel stack) and jumps to userspace using its userspace stack.
fn userspace_wrapper() -> ! {

    debug!("userspace_wrapper [0]");

    // the things we need to invoke jump_to_userspace
    let ustack_top: usize;
    let entry_func: usize; 

    { // scoped to release current task's lock before calling jump_to_userspace
        let currtask = get_my_current_task().expect("userspace_wrapper(): get_my_current_task() failed").lock();
        ustack_top = currtask.ustack.as_ref().expect("userspace_wrapper(): ustack was None!").top_usable().value();
        entry_func = currtask.new_userspace_entry_addr.expect("userspace_wrapper(): new_userspace_entry_addr was None!").value();
    }
    debug!("userspace_wrapper [1]: ustack_top: {:#x}, module_entry: {:#x}", ustack_top, entry_func);

    // SAFE: just jumping to userspace 
    unsafe {
        jump_to_userspace(ustack_top, entry_func);
    }
    // nothing below here should ever run again, we should never ever reach this point


    panic!("userspace_wrapper [end]: jump_to_userspace returned!!!");
}




#[cfg(spawn_userspace)]
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
