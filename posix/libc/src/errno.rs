//! Contains the error number variable that is updated by libc functions

use core::sync::atomic::{AtomicI32, Ordering};

// TODO: Should be a thread local variable
pub static ERRNO: AtomicI32 = AtomicI32::new(0);

