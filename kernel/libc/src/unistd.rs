use crate::types::*;

use task;

#[no_mangle]
pub extern "C" fn getpid() -> pid_t {
    //TODO: do we need some check to make sure the pid is 32 bits?
    task::get_my_current_task_id().unwrap() as c_int
}