pub const PAGE_SHIFT: usize = 6;
pub const PAGE_SIZE: usize = 1 << PAGE_SHIFT;

pub const MAX_VIRTUAL_ADDRESS: usize = 0xFFFF_FFFF;
pub const MAX_PAGE_NUMBER: usize = MAX_VIRTUAL_ADDRESS / PAGE_SIZE;

pub const KERNEL_HEAP_START: usize = 0x2000_8000;
pub const KERNEL_HEAP_INITIAL_SIZE: usize = 4 * 1024;
