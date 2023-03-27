#![no_std]

use x86_64::structures::tss::TaskStateSegment;
use atomic_linked_list::atomic_map::AtomicMap;
use spin::Mutex;
use memory::VirtualAddress;
use cpu::CpuId;

/// The index of the double fault stack in a TaskStateSegment (TSS)
pub const DOUBLE_FAULT_IST_INDEX: usize = 0;

/// The TSS list, one per CPU.
static TSS: AtomicMap<CpuId, Mutex<TaskStateSegment>> = AtomicMap::new();


/// Sets the current CPU's TSS privilege stack 0 (RSP0) entry, which points to the stack that 
/// the x86_64 hardware automatically switches to when transitioning from Ring 3 -> Ring 0.
/// Should be set to an address within the current userspace task's kernel stack.
/// WARNING: If set incorrectly, the OS will crash upon an interrupt from userspace into kernel space!!
pub fn tss_set_rsp0(new_privilege_stack_top: VirtualAddress) -> Result<(), &'static str> {
    let cpu_id = cpu::current_cpu();
    let mut tss_entry = TSS.get(&cpu_id).ok_or_else(|| {
        log::error!("tss_set_rsp0(): couldn't find TSS for CPU {}", cpu_id);
        "No TSS for the current CPU" 
    })?.lock();
    tss_entry.privilege_stack_table[0] = x86_64::VirtAddr::new(new_privilege_stack_top.value() as u64);
    // log::trace!("tss_set_rsp0: new TSS {:?}", tss_entry);
    Ok(())
}


/// Sets up TSS entry for the given CPU core. 
///
/// Returns a reference to a Mutex wrapping the new TSS entry.
pub fn create_tss(
    cpu_id: CpuId, 
    double_fault_stack_top_unusable: VirtualAddress, 
    privilege_stack_top_unusable: VirtualAddress
) -> &'static Mutex<TaskStateSegment> {
    let mut tss = TaskStateSegment::new();
    // TSS.RSP0 is used in kernel space after a transition from Ring 3 -> Ring 0
    tss.privilege_stack_table[0] = x86_64::VirtAddr::new(privilege_stack_top_unusable.value() as u64);
    tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX] = x86_64::VirtAddr::new(double_fault_stack_top_unusable.value() as u64);

    // insert into TSS list
    TSS.insert(cpu_id, Mutex::new(tss));
    let tss_ref = TSS.get(&cpu_id).unwrap(); // safe to unwrap since we just added it to the list
    // log::debug!("Created TSS for CPU {}, TSS: {:?}", cpu_id, tss_ref);
    tss_ref
}