//! Contains the error number variable that is updated by libc functions

use core::sync::atomic::{AtomicI32, Ordering};

// TODO: Should be a thread local variable
// For that we need to look at the linking and loading section of Theseus,
// since a new .tbss section is added to the ELF file
pub static ERRNO: AtomicI32 = AtomicI32::new(0);

// // #[thread_local]
// // #[allow(non_upper_case_globals)]
// // #[no_mangle]
// // pub static mut errno: c_int = 0;

