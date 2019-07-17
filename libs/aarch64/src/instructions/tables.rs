//! Instructions for loading descriptor tables (GDT, IDT, etc.).

use structures::gdt::SegmentSelector;

/// A struct describing a pointer to a descriptor table (GDT / IDT).
/// This is in a format suitable for giving to 'lgdt' or 'lidt'.
#[derive(Debug)]
#[repr(C, packed)]
pub struct DescriptorTablePointer {
    /// Size of the DT.
    pub limit: u16,
    /// Pointer to the memory region containing the DT.
    pub base: u64,
}

/// Load GDT table.
pub unsafe fn lgdt(gdt: &DescriptorTablePointer) {
    //TODO
}

/// Load LDT table.
pub unsafe fn lldt(ldt: &DescriptorTablePointer) {
    // asm!("lldt ($0)" :: "r" (ldt) : "memory");
    //TODO
}

/// Load IDT table.
pub unsafe fn lidt(idt: &DescriptorTablePointer) {
    // asm!("lidt ($0)" :: "r" (idt) : "memory");
    //TODO
}

/// Load the task state register using the `ltr` instruction.
pub unsafe fn load_tss(sel: SegmentSelector) {
    // asm!("ltr $0" :: "r" (sel.0));
    //TODO
}
