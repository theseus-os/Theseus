// TODO: Move into task crate. Hasn't yet been done because it creates a
// circular dependency as unwind depends on task.

#![feature(abi_x86_interrupt, try_blocks)]
#![no_std]

use core::{default::Default, iter::Iterator, sync::atomic::Ordering};
use gimli::{BaseAddresses, EhFrame, NativeEndian, UninitializedUnwindContext, UnwindSection};
use memory_structs::VirtualAddress;
use mod_mgmt::SectionType;
use task::{KillReason, TaskRef};
use x86_64::structures::idt::InterruptStackFrame;

pub fn cancel_task(task: TaskRef) {
    task.cancel_requested.store(true, Ordering::Relaxed);
}

pub fn set_trap_flag(stack_frame: &mut InterruptStackFrame) {
    unsafe { stack_frame.as_mut() }.update(|stack_frame| stack_frame.cpu_flags |= 0x100);
}

pub extern "x86-interrupt" fn interrupt_handler(mut stack_frame: InterruptStackFrame) {
    let instruction_pointer = stack_frame.instruction_pointer.as_u64();
    let stack_pointer = stack_frame.stack_pointer.as_u64();

    log::info!("instruction pointer: {instruction_pointer:0x?}");

    let can_unwind = Option::<_>::is_some(
        &try {
            let task = task::get_my_current_task().expect("couldn't get current task");

            // TODO: Search external unwind info.

            let krate = task.namespace.get_crate_containing_address(
                VirtualAddress::new_canonical(instruction_pointer as usize),
                false,
            )?;

            let krate = krate.lock_as_ref();
            let text_address = krate.text_pages.as_ref()?.1.start.value();
            let eh_frame_section = krate
                .sections
                .values()
                .find(|s| s.typ == SectionType::EhFrame)?;
            let eh_frame_address = eh_frame_section.virt_addr.value();

            let base_addresses = BaseAddresses::default()
                .set_text(text_address as u64)
                .set_eh_frame(eh_frame_address as u64);
            log::info!("{base_addresses:0x?}");

            let pages = eh_frame_section.mapped_pages.lock();
            let bytes = pages
                .as_slice(eh_frame_section.mapped_pages_offset, eh_frame_section.size)
                .ok()?;
            let eh_frame = EhFrame::new(bytes, NativeEndian);

            let frame_description_entry = eh_frame
                .fde_for_address(
                    &base_addresses,
                    instruction_pointer,
                    EhFrame::cie_from_offset,
                )
                .ok()?;

            let mut unwind_context = UninitializedUnwindContext::new();
            let _ = frame_description_entry
                .unwind_info_for_address(
                    &eh_frame,
                    &base_addresses,
                    &mut unwind_context,
                    instruction_pointer,
                )
                .ok()?;

            log::info!("covering");

            Some(())
        },
    );

    log::info!("stack frame: {:0x?}", stack_frame.stack_pointer);

    if can_unwind {
        unwind::start_remote_unwinding(
            KillReason::Requested,
            0,
            stack_pointer,
            instruction_pointer,
        )
        .expect("failed to unwind");
    } else {
        // FIXME: What happens if the APIC interrupt triggers here?
        set_trap_flag(&mut stack_frame);
    }
}
