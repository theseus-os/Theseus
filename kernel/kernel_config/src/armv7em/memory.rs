pub const PAGE_SHIFT: usize = 6;
pub const PAGE_SIZE: usize = 1 << PAGE_SHIFT;

pub const MAX_VIRTUAL_ADDRESS: usize = 0xFFFF_FFFF;
pub const MAX_PAGE_NUMBER: usize = MAX_VIRTUAL_ADDRESS / PAGE_SIZE;

pub const KERNEL_STACK_SIZE_IN_PAGES: usize = 16;
