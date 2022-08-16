//! A simple library that handles stdio queues for applications running in terminal instances.
//! 
//! This provides some APIs similar to Rust's `std::io` for applications to access those queues.
//! 
//! Usage example:
//! 1. shell spawns a new app, and creates queues of `stdin`, `stdout`, and `stderr` for that app.
//! 2. shell stores the reader for `stdin` and writers for `stdout` and `stderr` to `app_io`,
//!    along with the reader of the key events queue and references to the running terminal instance.
//! 3. app calls [`stdin()`] to get the reader of `stdin`, and can perform reading just like
//!    using the standard library
//! 4. app calls [`stdout()`] to get the writer of `stdin`, and can perform output just like
//!    using the standard library
//! 5. after app exits, shell would set `EOF` flags to its `stdin`, `stdout`, and `stderr` queues.
//! 6. once all apps in a job exit, app shell removes all the structure stored in `app_io` and
//!    destructs all stdio queues

#![no_std]
#![feature(const_btree_new)]

#[macro_use] extern crate log;
extern crate stdio;
extern crate spin;
#[macro_use] extern crate alloc;
extern crate keycodes_ascii;
extern crate libterm;
extern crate logger;
extern crate core2;
extern crate window_manager;

use stdio::{StdioReader, StdioWriter, KeyEventReadGuard,
            KeyEventQueueReader};
use spin::{Mutex, MutexGuard};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use libterm::Terminal;

/// Stores the stdio queues, key event queue and the pointer to the terminal
/// for applications. This structure is provided for application's use and only
/// contains necessary one-end readers/writers to queues. On the shell side, we have
/// full control to queues.
pub struct IoStreams {
    /// The reader to stdin.
    stdin: StdioReader,
    /// The writer to stdout.
    stdout: StdioWriter,
    /// The writer to stderr.
    stderr: StdioWriter,
    /// The reader to key event queue. This is the same reader as that in
    /// shell. Apps can take this reader to directly access keyboard events.
    key_event_reader: Arc<Mutex<Option<KeyEventQueueReader>>>,
    /// Points to the terminal.
    terminal: Arc<Mutex<Terminal>>
}

/// Applications set the flags in this structure to inform the parent shell to
/// perform IO accordingly.
pub struct IoControlFlags {
    /// When set to be `true`, the shell will immediately flush received character
    /// input to stdin rather than waiting for enter keystrike.
    stdin_instant_flush: bool
}

impl IoControlFlags {
    /// Create a new `IoControlFlags` instance. `stdin_instant_flush` is set to
    /// be `false` on default.
    pub fn new() -> IoControlFlags {
        IoControlFlags {
            stdin_instant_flush: false
        }
    }
}

impl IoStreams {
    pub fn new(stdin: StdioReader, stdout: StdioWriter,
               stderr: StdioWriter,
               key_event_reader: Arc<Mutex<Option<KeyEventQueueReader>>>,
               terminal: Arc<Mutex<Terminal>>) -> IoStreams {
        IoStreams {
            stdin,
            stdout,
            stderr,
            key_event_reader,
            terminal
        }
    }
}

mod shared_maps {
    use spin::{Mutex, MutexGuard};
    use alloc::collections::BTreeMap;
    use IoControlFlags;
    use IoStreams;

    /// Map a task to its IoControlFlags structure.
    /// When shells call `insert_child_streams`, the default value is automatically inserted for that task.
    /// When shells call `remove_child_streams`, the corresponding structure is removed from this map.
    static APP_IO_CTRL_FLAGS: Mutex<BTreeMap<usize, IoControlFlags>> = Mutex::new(BTreeMap::new());

    /// Map a task id to its IoStreams structure.
    /// Shells should call `insert_child_streams` when spawning a new app,
    /// which effectively stores a new key value pair to this map. 
    /// After a shell's child app exits, the shell should call `remove_child_streams` to clean it up.
    static APP_IO_STREAMS: Mutex<BTreeMap<usize, IoStreams>> = Mutex::new(BTreeMap::new());

    /// Lock and returns the `MutexGuard` of `APP_IO_CTRL_FLAGS`. Use `lock_all_maps()` if
    /// you want to lock both of the maps to avoid deadlock.
    pub fn lock_flag_map() -> MutexGuard<'static, BTreeMap<usize, IoControlFlags>> {
        APP_IO_CTRL_FLAGS.lock()
    }

    /// Lock and returns the `MutexGuard` of `APP_IO_STREAMS`. Use `lock_all_maps()` if
    /// you want to lock both of the maps to avoid deadlock.
    pub fn lock_stream_map() -> MutexGuard<'static, BTreeMap<usize, IoStreams>> {
        APP_IO_STREAMS.lock()
    }

    /// Lock two maps `APP_IO_CTRL_FLAGS` and `APP_IO_STREAMS` at the same time and returns a
    /// tuple containing their `MutexGuard`s. This function exerts a sequence of locking when
    /// we need to lock more than one of them. This prevents deadlock.
    pub fn lock_all_maps() -> (MutexGuard<'static, BTreeMap<usize, IoControlFlags>>,
                               MutexGuard<'static, BTreeMap<usize, IoStreams>>) {
        (APP_IO_CTRL_FLAGS.lock(), APP_IO_STREAMS.lock())
    }
}


/// An application can call this function to get the terminal to which it should print.
pub fn get_my_terminal() -> Option<Arc<Mutex<Terminal>>> {
    task::get_my_current_task_id()
        .and_then(|id| shared_maps::lock_stream_map()
            .get(&id)
            .map(|property| property.terminal.clone())
        )
}

/// Lock all shared states (i.e. those defined as `static`s) and execute the closure.
/// This function is mainly provided for the shell to clean up the pointers stored in the map
/// without causing a deadlock. In other words, by using this function, shell can prevent the
/// application from holding the lock of these shared maps before killing it. Otherwise, the
/// lock will never get a chance to be released. Since we currently don't have stack unwinding.
pub fn lock_and_execute<'a, F>(f: &F)
    where F: Fn(MutexGuard<'a, BTreeMap<usize, IoControlFlags>>,
                MutexGuard<'a, BTreeMap<usize, IoStreams>>) {
    let (locked_flags, locked_streams) = shared_maps::lock_all_maps();
    f(locked_flags, locked_streams);
}

/// Shells call this function to store queue readers/writers and the pointer to terminal for
/// applications. If there is any existing readers/writers for the task (which should not
/// happen in normal practice), it returns the old one, otherwise returns None.
pub fn insert_child_streams(task_id: usize, streams: IoStreams) -> Option<IoStreams> {
    shared_maps::lock_flag_map().insert(task_id, IoControlFlags::new());
    shared_maps::lock_stream_map().insert(task_id, streams)
}

/// Shells call this function to remove queues and pointer to terminal for applications. It returns
/// the removed streams in the return value if the key matches, otherwise returns None.
pub fn remove_child_streams(task_id: &usize) -> Option<IoStreams> {
    shared_maps::lock_flag_map().remove(task_id);
    shared_maps::lock_stream_map().remove(task_id)
}

/// Applications call this function to acquire a reader to its stdin queue.
/// 
/// Errors can occur in two cases. One is when it fails to get the task_id of the calling
/// task, and the second is that there's no stdin reader stored in the map. Shells should
/// make sure to store IoStreams for the newly spawned app first, and then unblocks the app
/// to let it run.
pub fn stdin() -> Result<StdioReader, &'static str> {
    let task_id = task::get_my_current_task_id().ok_or("failed to get task_id to get stdin")?;
    let locked_streams = shared_maps::lock_stream_map();
    match locked_streams.get(&task_id) {
        Some(queues) => Ok(queues.stdin.clone()),
        None => Err("no stdin for this task")
    }
}

/// Applications call this function to acquire a writer to its stdout queue.
/// 
/// Errors can occur in two cases. One is when it fails to get the task_id of the calling
/// task, and the second is that there's no stdout writer stored in the map. Shells should
/// make sure to store IoStreams for the newly spawned app first, and then unblocks the app
/// to let it run.
pub fn stdout() -> Result<StdioWriter, &'static str> {
    let task_id = task::get_my_current_task_id().ok_or("failed to get task_id to get stdout")?;
    let locked_streams = shared_maps::lock_stream_map();
    match locked_streams.get(&task_id) {
        Some(queues) => Ok(queues.stdout.clone()),
        None => Err("no stdout for this task")
    }
}

/// Applications call this function to acquire a writer to its stderr queue.
/// 
/// Errors can occur in two cases. One is when it fails to get the task_id of the calling
/// task, and the second is that there's no stderr writer stored in the map. Shells should
/// make sure to store IoStreams for the newly spawned app first, and then unblocks the app
/// to let it run.
pub fn stderr() -> Result<StdioWriter, &'static str> {
    let task_id = task::get_my_current_task_id().ok_or("failed to get task_id to get stderr")?;
    let locked_streams = shared_maps::lock_stream_map();
    match locked_streams.get(&task_id) {
        Some(queues) => Ok(queues.stderr.clone()),
        None => Err("no stderr for this task")
    }
}

/// Applications call this function to take reader to the key event queue to directly
/// access keyboard events.
/// 
/// Errors can occur in three cases. One is when it fails to get the task_id of the calling
/// task, and the second is that there's no key event reader stored in the map. Shells should
/// make sure to store IoStreams for the newly spawned app first, and then unblocks the app
/// to let it run. The third case happens when the reader of the key event queue has
/// already been taken by some other task, which may be running simultaneously, or be killed
/// prematurely so that it cannot return the key event reader on exit.
pub fn take_key_event_queue() -> Result<KeyEventReadGuard, &'static str> {
    let task_id = task::get_my_current_task_id().ok_or("failed to get task_id to take key event queue")?;
    let locked_streams = shared_maps::lock_stream_map();
    match locked_streams.get(&task_id) {
        Some(queues) => {
            match queues.key_event_reader.lock().take() {
                Some(reader) => Ok(KeyEventReadGuard::new(
                    reader,
                    Box::new(|reader: &mut Option<KeyEventQueueReader>|  { return_event_queue(reader); }),
                )),
                None => Err("currently the reader to key event queue is not available")
            }
        },
        None => Err("no key event queue reader for this task")
    }
}

/// This function is automatically invoked upon dropping of `KeyEventReadGuard`.
/// It returns the reader of key event queue back to the shell.
fn return_event_queue(reader: &mut Option<KeyEventQueueReader>) {
    match task::get_my_current_task_id().ok_or("failed to get task_id to return event queue") {
        Ok(task_id) => {
            let locked_streams = shared_maps::lock_stream_map();
            match locked_streams.get(&task_id) {
                Some(queues) => {
                    core::mem::swap(&mut *queues.key_event_reader.lock(), reader);
                },
                None => { error!("no stderr for this task"); }
            };
        },
        Err(e) => {
            error!("app_io::return_event_queue(): Failed to get task_id to store new event queue. Error: {}", e);
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
    let mut locked_flags = shared_maps::lock_flag_map();
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
    let mut locked_flags = shared_maps::lock_flag_map();
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
    let locked_flags = shared_maps::lock_flag_map();
    match locked_flags.get(task_id) {
        Some(flags) => {
            Ok(flags.stdin_instant_flush)
        },
        None => Err("no io control flags for this task")
    }
}

/// Calls `print!()` with an extra newline ('\n') appended to the end. 
#[macro_export]
macro_rules! println {
    ($fmt:expr) => (print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"), $($arg)*));

}

/// The main printing macro, which simply writes to the current task's stdout stream.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ({
        $crate::print_to_stdout_args(format_args!($($arg)*));
    });
}

use core::fmt;
use core2::io::Write;
/// Converts the given `core::fmt::Arguments` to a `String` and enqueues the string into the correct
/// terminal print-producer
pub fn print_to_stdout_args(fmt_args: fmt::Arguments) {
    let task_id = match task::get_my_current_task_id() {
        Some(task_id) => task_id,
        None => {
            // We cannot use log macros here, because when they're mirrored to the vga, they will cause
            // infinite loops on an error. Instead, we write directly to the logger's output streams. 
            let _ = logger::write_str("\x1b[31m [E] error in print!/println! macro: failed to get current task id \x1b[0m\n");
            return;
        }
    };

    // Obtains the correct stdout stream and push the output bytes.
    let locked_streams = shared_maps::lock_stream_map();
    match locked_streams.get(&task_id) {
        Some(queues) => {
            if let Err(_) = queues.stdout.lock().write_all(format!("{}", fmt_args).as_bytes()) {
                let _ = logger::write_str("\x1b[31m [E] failed to write to stdout \x1b[0m\n");
            }
        },
        None => {
            let _ = logger::write_str("\x1b[31m [E] error in print!/println! macro: no stdout queue for current task \x1b[0m\n");
            return;
        }
    };
}
