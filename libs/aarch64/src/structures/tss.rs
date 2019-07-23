//! Provides a type for the task state segment structure.
//! For compatibility with x86 temporarily. To be implemented in the future
use VirtualAddress;

/// In 64-bit mode the TSS holds information that is not
/// directly related to the task-switch mechanism,
/// but is used for finding kernel level stack
/// if interrupts arrive while in kernel mode.
#[repr(C, packed)]
pub struct TaskStateSegment {
    reserved_1: u32,
    /// The full 64-bit canonical forms of the stack pointers (RSP) for privilege levels 0-2.
    pub privilege_stack_table: [VirtualAddress; 3],
    reserved_2: u64,
    /// The full 64-bit canonical forms of the interrupt stack table (IST) pointers.
    pub interrupt_stack_table: [VirtualAddress; 7],
    reserved_3: u64,
    reserved_4: u16,
    /// The 16-bit offset to the I/O permission bit map from the 64-bit TSS base.
    pub iomap_base: u16,
}

impl TaskStateSegment {
    /// Creates a new TSS with zeroed privilege and interrupt stack table and a zero
    /// `iomap_base`.
    pub const fn new() -> TaskStateSegment {
        TaskStateSegment {
            privilege_stack_table: [VirtualAddress(0); 3],
            interrupt_stack_table: [VirtualAddress(0); 7],
            iomap_base: 0,
            reserved_1: 0,
            reserved_2: 0,
            reserved_3: 0,
            reserved_4: 0,
        }
    }
}
