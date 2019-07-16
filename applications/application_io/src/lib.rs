#![no_std]
#![feature(asm)]

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate alloc;
extern crate task;
extern crate dfqueue;
extern crate event_types;
extern crate spin;
extern crate keycodes_ascii;

use keycodes_ascii::KeyEvent;
use dfqueue::DFQueueConsumer;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;
use alloc::boxed::Box;

// IMPORTANT: To avoid deadlock, if multiple structure are to be locked, we must lock them in the sequence
// same as the following definition.

lazy_static! {
    /// Maps teh child applications's task ID to a flag indicating whether the application want direct access of
    /// keyboard events or not. If the flag is set to be true, shell will direct all keyboard events to the application
    /// through the DFQueue.
    static ref APP_DIRECT_KEY_ACCESS: Mutex<BTreeMap<usize, bool>> =
        Mutex::new(BTreeMap::new());
}

lazy_static! {
    /// Maps the child application's task ID to its consumer of the keyboard events directed by the parent shell.
    /// The queue simulates connecting an input event stream to the application.
    static ref SHELL_KEY_EVENT_CONSUMER: Mutex<BTreeMap<usize, DFQueueConsumer<KeyEvent>>> =
        Mutex::new(BTreeMap::new());
}

/// Lock all shared states (i.e. those defined in `lazy_static!`) and execute the closure.
pub fn locked_and_execute(f: Box<Fn()>) {
    let _flag_guard = APP_DIRECT_KEY_ACCESS.lock();
    let _consumer_guard = SHELL_KEY_EVENT_CONSUMER.lock();
    f();
}

/// Set up queues between shell and application.
pub fn create_app_shell_relation(child_task_id: usize,
                                 keyboard_event_consumer: DFQueueConsumer<KeyEvent>) -> Result<(), &'static str> {
    APP_DIRECT_KEY_ACCESS.lock().insert(child_task_id, false);
    SHELL_KEY_EVENT_CONSUMER.lock().insert(child_task_id, keyboard_event_consumer);
    Ok(())
}

/// Remove queues between shell and application.
pub fn remove_app_shell_relation(child_task_id: usize) -> Result<(), &'static str> {
    APP_DIRECT_KEY_ACCESS.lock().remove(&child_task_id);
    SHELL_KEY_EVENT_CONSUMER.lock().remove(&child_task_id);
    Ok(())
}

/// Applications call this function to get a keyboard event directed by the shell.
/// If nothing is currently in the queue, this function returns Ok(None).
pub fn get_keyboard_event() -> Result<Option<KeyEvent>, &'static str> {
    let task_id = match task::get_my_current_task_id() {
        Some(task_id) => {task_id},
        None => {
            error!("Cannot get task ID for keyboard event consuming");
            return Err("Cannot get task ID for keyboard event consuming");
        }
    };

    use core::ops::Deref;
    let map = SHELL_KEY_EVENT_CONSUMER.lock();
    let result = map.get(&task_id);
    if let Some(consumer) = result {
        if let Some(peeked_data) = consumer.peek() {
            peeked_data.mark_completed();
            return Ok(Some(*peeked_data.deref()));
        }
    }
    return Ok(None);
}

/// Check whether an application is requesting the shell to forward the keyboard evnets.
pub fn is_requesting_forward(task_id: usize) -> bool {
    let map = APP_DIRECT_KEY_ACCESS.lock();
    let result = map.get(&task_id);
    if let Some(flag) = result {
        return *flag;
    }
    return false;
}

/// Applications call this function to request the shell to forward keyboard events to them.
pub fn request_kbd_event_forward() -> Result<(), &'static str> {
    let task_id = match task::get_my_current_task_id() {
        Some(task_id) => {task_id},
        None => {
            error!("Cannot get task ID for starting keyboard event forwaring");
            return Err("Cannot get task ID for starting keyboard event forwarding");
        }
    };

    APP_DIRECT_KEY_ACCESS.lock().insert(task_id, true);
    Ok(())
}

/// Applications call this function to stop the shell forwarding keyboards events to them.
/// Instead, let the shell handle input events.
pub fn stop_kbd_event_forward() -> Result<(), &'static str> {
    let task_id = match task::get_my_current_task_id() {
        Some(task_id) => {task_id},
        None => {
            error!("Cannot get task ID for starting keyboard event consuming");
            return Err("Cannot get task ID for starting keyboard event consuming");
        }
    };

    APP_DIRECT_KEY_ACCESS.lock().remove(&task_id);
    Ok(())
}

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    loop {
        // block this task, because it never needs to actually run again
        if let Some(my_task) = task::get_my_current_task() {
            my_task.block();
        }
    }
}
