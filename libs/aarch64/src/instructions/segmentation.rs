//! Provides functions to read and write segment registers.

use structures::gdt::SegmentSelector;

/// Reload code segment register.
/// Note this is special since we can not directly move
/// to %cs. Instead we push the new segment selector
/// and return value on the stack and use lretq
/// to reload cs and continue at 1:.
pub unsafe fn set_cs(sel: SegmentSelector) {
    //TODO
}

/// Reload stack segment register.
pub unsafe fn load_ss(sel: SegmentSelector) {
    //TODO
}

/// Reload data segment register.
pub unsafe fn load_ds(sel: SegmentSelector) {
    //TODO
}

/// Reload es segment register.
pub unsafe fn load_es(sel: SegmentSelector) {
    //TODO
}

/// Reload fs segment register.
pub unsafe fn load_fs(sel: SegmentSelector) {
    //TODO
}

/// Reload gs segment register.
pub unsafe fn load_gs(sel: SegmentSelector) {
    //TODO
}

/// Returns the current value of the code segment register.
pub fn cs() -> SegmentSelector {
    //TODO
    SegmentSelector(0)
}
