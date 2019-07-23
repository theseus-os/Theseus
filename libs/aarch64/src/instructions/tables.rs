//! Instructions for loading descriptor tables (GDT, IDT, etc.).

use structures::gdt::SegmentSelector;

/// A struct describing a pointer to a descriptor table (GDT / IDT).
/// This is in a format suitable for giving to 'lgdt' or 'lidt'.
#[repr(C, packed)]
pub struct DescriptorTablePointer {
    /// Size of the DT.
    pub limit: u16,
    /// Pointer to the memory region containing the DT.
    pub base: u64,
}

/// Load GDT table.
pub unsafe fn lgdt(_gdt: &DescriptorTablePointer) {
    //TODO
}

/// Load LDT table.
pub unsafe fn lldt(_ldt: &DescriptorTablePointer) {
    // asm!("lldt ($0)" :: "r" (ldt) : "memory");
    //TODO
}

/// Load IDT table.
pub unsafe fn lidt(_idt: &DescriptorTablePointer) {
    // asm!("lidt ($0)" :: "r" (idt) : "memory");
    //TODO
}

/// Load the task state register using the `ltr` instruction.
pub unsafe fn load_tss(_sel: SegmentSelector) {
    // asm!("ltr $0" :: "r" (sel.0));
    //TODO
}
