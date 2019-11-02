//! miscellaneous symbolic constants, types, and functions

use libc::{c_void, c_int, pid_t};

use task;

pub const NULL: *mut c_void = 0 as *mut c_void;

#[no_mangle]
pub extern "C" fn getpid() -> pid_t {
    task::get_my_current_task_id().unwrap() as c_int
}