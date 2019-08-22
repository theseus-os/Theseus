//! This crate stores the IO queues and pointers to terminals for running applications.
//! It provides some APIs similar to std::io for applications to access those queues.
//! 
//! Usage example:
//! 1. shell spawns a new app, and creates queues of `stdin`, `stdout` and `stderr`
//! 2. shell stores the read handle of `stdin` and write handle of `stdout` and `stderr` to `app_io`,
//!    along with the read handle of key event queue and the pointer to the running terminal instance.
//! 3. app calls app_io::stdin to get the read handle of `stdin`, and can perform reading just like
//!    using the standard library
//! 4. app calls app_io::stdout to get the write handle of stdin, and can perform output just like
//!    using the standard library
//! 5. after app exits, shell removes the structure stored in `app_io` and destructs all stdio queues

#![no_std]

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate stdio;
extern crate spin;
extern crate alloc;
extern crate keycodes_ascii;
extern crate libterm;
extern crate scheduler;

use stdio::{StdioReadHandle, StdioWriteHandle, KeyEventConsumerGuard,
            KeyEventQueueReadHandle};
use spin::{Mutex, MutexGuard};
use alloc::collections::BTreeMap;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::string::String;
use alloc::vec::Vec;
use libterm::Terminal;

/// Stores the stdio queues, key event queue and the pointer to the terminal
/// for applications. This structure is provided for application's use and only
/// contains necessary one-end handles to queues. On the shell side, we have
/// full control to queues.
pub struct IoHandles {
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

/// Applications set the flags in this structure to inform the parent shell to
/// perform IO accordingly.
pub struct IoControlFlags {
    /// When set to be `true`, the shell will immediately flush received character
    /// input to stdin rather than waiting for enter keystrike. Default value
    /// is `false`.
    stdin_instant_flush: bool
}

impl IoControlFlags {
    pub fn new() -> IoControlFlags {
        IoControlFlags {
            stdin_instant_flush: false
        }
    }
}

impl IoHandles {
    pub fn new(stdin: StdioReadHandle, stdout: StdioWriteHandle,
               stderr: StdioWriteHandle,
               key_event_read_handle: Arc<Mutex<Option<KeyEventQueueReadHandle>>>,
               terminal: Arc<Mutex<Terminal>>) -> IoHandles {
        IoHandles {
            stdin,
            stdout,
            stderr,
            key_event_read_handle,
            terminal
        }
    }
}

// IMPORTANT NOTE:
// To avoid probable deadlock, if we need to lock multiple maps at the same time, we must
// perform it in the sequence the *same* as the following definition.

lazy_static! {
    /// Map applications to their IoControlFlags structure. Here the key is the task_id
    /// of each task. When shells call `insert_child_handles`, the default value is
    /// automatically inserted for that task. When shells call `remove_child_handles`,
    /// the corresponding structure is removed from this map.
    static ref APP_IO_CTRL_FLAGS: Mutex<BTreeMap<usize, IoControlFlags>> = Mutex::new(BTreeMap::new());
}

lazy_static! {
    /// Map applications to their IoHandles structure. Here the key is the task_id of
    /// each task. Shells should call `insert_child_handles` when spawning a new app,
    /// which effectively stores a new key value pair to this map. After applications
    /// exit, shells should call `remove_child_handles` to clean up.
    static ref APP_IO_HANDLES: Mutex<BTreeMap<usize, IoHandles>> = Mutex::new(BTreeMap::new());
}

lazy_static! {
    /// The default terminal.
    static ref DEFAULT_TERMINAL: Option<Arc<Mutex<Terminal>>> =
        match Terminal::new() {
            Ok(terminal) => Some(Arc::new(Mutex::new(terminal))),
            Err(_) => None
        };
}

/// Applications call this function to get the terminal to which it should print.
/// If the calling application has already been assigned a terminal to print, that assigned
/// terminal is returned. Otherwise, the default terminal is assigned to the calling application
/// and then returned.
pub fn get_terminal_or_default() -> Result<Arc<Mutex<Terminal>>, &'static str> {
    
    let task_id = task::get_my_current_task_id()
                      .ok_or("Cannot get task ID for getting default terminal")?;

    if let Some(property) = APP_IO_HANDLES.lock().get(&task_id) {
        return Ok(Arc::clone(&property.terminal));
    }

    loop {
        match *DEFAULT_TERMINAL {
            Some(ref terminal) => return Ok(Arc::clone(&terminal)),
            _ => { error!("Failed to get default terminal, retrying..."); }
        }
        scheduler::schedule(); // yield the CPU and try again later
    }
}

/// Lock all shared states (i.e. those defined in `lazy_static!`) and execute the closure.
/// This function is mainly provided for the shell to clean up the pointers stored in the map
/// without causing a deadlock. In other words, by using this function, shell can prevent the
/// application from holding the lock of these shared maps before killing it. Otherwise, the
/// lock will never get a chance to be released. Since we currently don't have stack unwinding.
pub fn lock_and_execute<'a>(f: Box<dyn Fn(MutexGuard<'a, BTreeMap<usize, IoControlFlags>>,
                                          MutexGuard<'a, BTreeMap<usize, IoHandles>>)>) {
    let locked_flags = APP_IO_CTRL_FLAGS.lock();
    let locked_handles = APP_IO_HANDLES.lock();
    f(locked_flags, locked_handles);
}

/// Shells call this function to store queue handles and the pointer to terminal for applications.
/// If there is any existing handles for the task (which should not happen in normal practice),
/// it returns the old one, otherwise returns None.
pub fn insert_child_handles(task_id: usize, handles: IoHandles) -> Option<IoHandles> {
    let mut locked_flags = APP_IO_CTRL_FLAGS.lock();
    locked_flags.insert(task_id, IoControlFlags::new());
    core::mem::drop(locked_flags);
    let mut locked_handles = APP_IO_HANDLES.lock();
    locked_handles.insert(task_id, handles)
}

/// Shells call this function to remove queues and pointer to terminal for applications. It returns
/// the removed handles in the return value if the key matches, otherwise returns None.
pub fn remove_child_handles(task_id: &usize) -> Option<IoHandles> {
    let mut locked_flags = APP_IO_CTRL_FLAGS.lock();
    locked_flags.remove(task_id);
    core::mem::drop(locked_flags);
    let mut locked_handles = APP_IO_HANDLES.lock();
    locked_handles.remove(task_id)
}

/// Applications call this function to acquire a read handle to its stdin queue.
/// 
/// Errors can occur in two cases. One is when it fails to get the task_id of the calling
/// task, and the second is that there's no stdin handles stored in the map. Shells should
/// make sure to store IoHandles for the newly spawned app first, and then unblocks the app
/// to let it run.
pub fn stdin() -> Result<StdioReadHandle, &'static str> {
    let task_id = task::get_my_current_task_id().ok_or("failed to get task_id to get stdin")?;
    let locked_handles = APP_IO_HANDLES.lock();
    match locked_handles.get(&task_id) {
        Some(queues) => Ok(queues.stdin.clone()),
        None => Err("no stdin for this task")
    }
}

/// Applications call this function to acquire a write handle to its stdout queue.
/// 
/// Errors can occur in two cases. One is when it fails to get the task_id of the calling
/// task, and the second is that there's no stdout handles stored in the map. Shells should
/// make sure to store IoHandles for the newly spawned app first, and then unblocks the app
/// to let it run.
pub fn stdout() -> Result<StdioWriteHandle, &'static str> {
    let task_id = task::get_my_current_task_id().ok_or("failed to get task_id to get stdout")?;
    let locked_handles = APP_IO_HANDLES.lock();
    match locked_handles.get(&task_id) {
        Some(queues) => Ok(queues.stdout.clone()),
        None => Err("no stdout for this task")
    }
}

/// Applications call this function to acquire a write handle to its stderr queue.
/// 
/// Errors can occur in two cases. One is when it fails to get the task_id of the calling
/// task, and the second is that there's no stderr handles stored in the map. Shells should
/// make sure to store IoHandles for the newly spawned app first, and then unblocks the app
/// to let it run.
pub fn stderr() -> Result<StdioWriteHandle, &'static str> {
    let task_id = task::get_my_current_task_id().ok_or("failed to get task_id to get stderr")?;
    let locked_handles = APP_IO_HANDLES.lock();
    match locked_handles.get(&task_id) {
        Some(queues) => Ok(queues.stderr.clone()),
        None => Err("no stderr for this task")
    }
}

/// Applications call this function to take read handle to the key event queue to directly
/// access keyboard events.
/// 
/// Errors can occur in three cases. One is when it fails to get the task_id of the calling
/// task, and the second is that there's no stderr handles stored in the map. Shells should
/// make sure to store IoHandles for the newly spawned app first, and then unblocks the app
/// to let it run. The third case happens when the read handle of the key event queue has
/// already been taken by some other task, which may be running simultaneously, or be killed
/// prematurely so that it cannot return the key event handle on exit.
pub fn take_key_event_queue() -> Result<KeyEventConsumerGuard, &'static str> {
    let task_id = task::get_my_current_task_id().ok_or("failed to get task_id to take key event queue")?;
    let locked_handles = APP_IO_HANDLES.lock();
    match locked_handles.get(&task_id) {
        Some(queues) => {
            match queues.key_event_read_handle.lock().take() {
                Some(read_handle) => Ok(KeyEventConsumerGuard::new(read_handle,
                                        Box::new(|read_handle| { return_event_queue(read_handle); }))),
                None => Err("currently the read handle to key event queue is not available")
            }
        },
        None => Err("no key event queue handle for this task")
    }
}

/// This function is automatically invoked upon dropping of `KeyEventConsumerGuard`.
/// It returns the read handle of key event queue back to the shell.
fn return_event_queue(read_handle: KeyEventQueueReadHandle) {
    match task::get_my_current_task_id().ok_or("failed to get task_id to return event queue") {
        Ok(task_id) => {
            let locked_handles = APP_IO_HANDLES.lock();
            match locked_handles.get(&task_id) {
                Some(queues) => {
                    queues.key_event_read_handle.lock().replace(read_handle);
                },
                None => { error!("no stderr for this task"); }
            };
        },
        Err(e) => {
            error!("{}", e);
            error!("failed to get task_id to store event queue");
        }
    };
}

/// Applications call this function to set the flag which requests the parent shell to
/// flush stdin immediately upon character input, rather than waiting for enter key strike.
/// 
/// Errors can occur in two cases, when it fails to get the `task_id` of the calling task,
/// or it finds no IoControlFlags structure for that task.
pub fn request_stdin_instant_flush() -> Result<(), &'static str> {
    let task_id = task::get_my_current_task_id().ok_or("failed to get task_id to request stdin instant flush")?;
    let mut locked_flags = APP_IO_CTRL_FLAGS.lock();
    match locked_flags.get_mut(&task_id) {
        Some(flags) => {
            flags.stdin_instant_flush = true;
            Ok(())
        },
        None => Err("no io control flags for this task")
    }
}

/// Applications call this function to reset the flag which requests the parent shell to
/// flush stdin immediately upon character input, but to wait for enter key strike.
/// 
/// Errors can occur in two cases, when it fails to get the `task_id` of the calling task,
/// or it finds no IoControlFlags structure for that task.
pub fn cancel_stdin_instant_flush() -> Result<(), &'static str> {
    let task_id = task::get_my_current_task_id().ok_or("failed to get task_id to cancel stdin instant flush")?;
    let mut locked_flags = APP_IO_CTRL_FLAGS.lock();
    match locked_flags.get_mut(&task_id) {
        Some(flags) => {
            flags.stdin_instant_flush = false;
            Ok(())
        },
        None => Err("no io control flags for this task")
    }
}

/// Shell call this function to check whether a task is requesting instant stdin flush.
/// 
/// Error can occur when there is no IoControlFlags structure for that task.
pub fn is_requesting_instant_flush(task_id: &usize) -> Result<bool, &'static str> {
    let locked_flags = APP_IO_CTRL_FLAGS.lock();
    match locked_flags.get(task_id) {
        Some(flags) => {
            Ok(flags.stdin_instant_flush)
        },
        None => Err("no io control flags for this task")
    }
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
