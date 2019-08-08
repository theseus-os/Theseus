/// The virtual address where the initial kernel (the nano_core) is mapped to.
/// Actual value: 0xFFFFFFFF80000000.
/// i.e., the linear offset between physical memory and kernel memory.
/// So, for example, the VGA buffer will be mapped from 0xb8000 to 0xFFFFFFFF800b8000.
/// This is -2GiB from the end of the 64-bit address space.
pub const KERNEL_OFFSET: usize = 0xFFFF_FFFF_8000_0000;
/// For higher half virtual address the bits from KERNEL_OFFSET_BITS_START to 64 are 1
pub const KERNEL_OFFSET_BITS_START: u8 = 47;
/// The prefix of higher half virtual address;
pub const KERNEL_OFFSET_PREFIX: usize = 0b1_1111_1111_1111_1111;
