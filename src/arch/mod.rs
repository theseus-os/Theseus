mod arch_x86_64;

// #[cfg(target_arch = "x86_64")]  // TODO: can we have a cfg block { } ?
pub use self::arch_x86_64::{Registers, ArchTaskState, /* get_page_table_register, */
                            pause};

