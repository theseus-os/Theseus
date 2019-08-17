//! This crate stores the IO queues and pointers to terminals for running applications.
//! It provides some APIs similar to std::io for applications to access those queues.
#![no_std]

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate stdio;
extern crate spin;
extern crate alloc;
extern crate keycodes_ascii;
extern crate libterm;

use stdio::{StdioReadHandle, StdioWriteHandle, KeyEventConsumerGuard,
            KeyEventQueueReadHandle};
use spin::Mutex;
use alloc::collections::BTreeMap;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::string::String;
use alloc::vec::Vec;
use libterm::Terminal;

/// Stores the stdio queues, key event queue and the pointer to the terminal
/// for applications.
pub struct IoProperty {
    /// The read handle to stdin.
    stdin: StdioReadHandle,
    /// The write handle to stdout.
    stdout: StdioWriteHandle,
    /// The write handle to stderr.
    stderr: StdioWriteHandle,
    /// The read handle to key event queue. This is the same handle as that in
    /// shell. Apps can take this handle to directly access keyboard events.
    key_event_read_handle: Arc<Mutex<Option<KeyEventQueueReadHandle>>>,
    /// Points to the terminal.
    terminal: Arc<Mutex<Terminal>>
}

impl IoProperty {
    pub fn new(stdin: StdioReadHandle, stdout: StdioWriteHandle,
               stderr: StdioWriteHandle,
               key_event_read_handle: Arc<Mutex<Option<KeyEventQueueReadHandle>>>,
               terminal: Arc<Mutex<Terminal>>) -> IoProperty {
        IoProperty {
            stdin,
            stdout,
            stderr,
            key_event_read_handle,
            terminal
        }
    }
}

lazy_static! {
    /// Map applications to their IoProperty structure.
    static ref APP_TO_QUEUES: Mutex<BTreeMap<usize, IoProperty>> = Mutex::new(BTreeMap::new());
}

lazy_static! {
    /// The default terminal. We choose to unwrap here since failing to create a default terminal
    /// is indeed a fatal error.
    static ref DEFAULT_TERMINAL: Arc<Mutex<Terminal>> = Arc::new(Mutex::new(Terminal::new().unwrap()));
}

/// Get the task id of the calling task, or return the error message if it fails.
fn get_task_id_or_error(err_msg: &'static str) -> Result<usize, &'static str> {
    match task::get_my_current_task_id() {
        Some(task_id) => { Ok(task_id) },
        None => {
            error!("{}", err_msg);
            Err(err_msg)
        }
    }
}

/// Applications call this function to get the terminal to which it should print.
/// If the calling application has already been assigned a terminal to print, that assigned
/// terminal is returned. Otherwise, the default terminal is assigned to the calling application
/// and then returned.
pub fn get_terminal_or_default() -> Result<Arc<Mutex<Terminal>>, &'static str> {
    
    let task_id = match task::get_my_current_task_id() {
        Some(task_id) => {task_id},
        None => {
            error!("Cannot get task ID for getting default terminal");
            return Err("Cannot get task ID for getting default terminal");
        }
    };

    if let Some(property) = APP_TO_QUEUES.lock().get(&task_id) {
        return Ok(Arc::clone(&property.terminal));
    }

    Ok(Arc::clone(&DEFAULT_TERMINAL))
}

/// Lock all shared states (i.e. those defined in `lazy_static!`) and execute the closure.
/// This function is mainly provided for the shell to clean up the pointers stored in the map
/// without causing a deadlock. In other words, by using this function, shell can prevent the
/// application from holding the lock of these shared maps before killing it. Otherwise, the
/// lock will never get a chance to be released. Since we currently don't have stack unwinding.
pub fn lock_and_execute(f: Box<Fn()>) {
    let _locked_queues = APP_TO_QUEUES.lock();
    f();
}

/// Shells call this function to store queues and the pointer to terminal for applications.
pub fn add_child(task_id: usize, stdin: StdioReadHandle, stdout: StdioWriteHandle,
                 stderr: StdioWriteHandle,
                 key_event_read_handle: Arc<Mutex<Option<KeyEventQueueReadHandle>>>)
                 -> Result<(), &'static str> {
    let terminal = get_terminal_or_default()?;
    let mut locked_queues = APP_TO_QUEUES.lock();
    let queues = IoProperty {
        stdin,
        stdout,
        stderr,
        key_event_read_handle,
        terminal
    };
    locked_queues.insert(task_id, queues);
    Ok(())
}

/// Shells call this function to remove queues and pointer to terminal for applications.
pub fn remove_child(task_id: usize) -> Result<(), &'static str> {
    let mut locked_queues = APP_TO_QUEUES.lock();
    locked_queues.remove(&task_id);
    Ok(())
}

/// Applications call this function to acquire a read handle to its stdin queue.
pub fn stdin() -> Result<StdioReadHandle, &'static str> {
    let task_id = get_task_id_or_error("failed to get task_id to get stdin")?;
    let locked_queues = APP_TO_QUEUES.lock();
    match locked_queues.get(&task_id) {
        Some(queues) => Ok(queues.stdin.clone()),
        None => Err("no stdin for this task")
    }
}

/// Applications call this function to acquire a write handle to its stdout queue.
pub fn stdout() -> Result<StdioWriteHandle, &'static str> {
    let task_id = get_task_id_or_error("failed to get task_id to get stdout")?;
    let locked_queues = APP_TO_QUEUES.lock();
    match locked_queues.get(&task_id) {
        Some(queues) => Ok(queues.stdout.clone()),
        None => Err("no stdout for this task")
    }
}

/// Applications call this function to acquire a write handle to its stderr queue.
pub fn stderr() -> Result<StdioWriteHandle, &'static str> {
    let task_id = get_task_id_or_error("failed to get task_id to get stderr")?;
    let locked_queues = APP_TO_QUEUES.lock();
    match locked_queues.get(&task_id) {
        Some(queues) => Ok(queues.stderr.clone()),
        None => Err("no stderr for this task")
    }
}

/// Applications call this function to take read handle to the key event queue to
/// directly access keyboard events. 
pub fn take_event_queue() -> Result<KeyEventConsumerGuard, &'static str> {
    let task_id = get_task_id_or_error("failed to get task_id to take event queue")?;
    let locked_queues = APP_TO_QUEUES.lock();
    match locked_queues.get(&task_id) {
        Some(queues) => {
            match queues.key_event_read_handle.lock().take() {
                Some(read_handle) => Ok(KeyEventConsumerGuard::new(read_handle,
                                        Box::new(|read_handle| { return_event_queue(read_handle); }))),
                None => Err("currently the read handle to key event queue is not available")
            }
        },
        None => Err("no IoProperty for this task")
    }
}

/// This function is automatically invoked upon dropping of `KeyEventConsumerGuard`.
/// It returns the read handle back to the shell.
fn return_event_queue(read_handle: KeyEventQueueReadHandle) {
    match get_task_id_or_error("failed to get task_id to return event queue") {
        Ok(task_id) => {
            let locked_queues = APP_TO_QUEUES.lock();
            match locked_queues.get(&task_id) {
                Some(queues) => {
                    queues.key_event_read_handle.lock().replace(read_handle);
                },
                None => { error!("no stderr for this task"); }
            };
        },
        Err(_) => {
            error!("failed to get task_id to store event queue");
        }
    };
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
