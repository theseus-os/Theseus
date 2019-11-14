//! This crate stores the IO queues and pointers to terminals for running applications.
//! It provides some APIs similar to std::io for applications to access those queues.
//! 
//! Usage example:
//! 1. shell spawns a new app, and creates queues of `stdin`, `stdout` and `stderr`
//! 2. shell stores the reader of `stdin` and writer of `stdout` and `stderr` to `app_io`,
//!    along with the reader of key event queue and the pointer to the running terminal instance.
//! 3. app calls app_io::stdin to get the reader of `stdin`, and can perform reading just like
//!    using the standard library
//! 4. app calls app_io::stdout to get the writer of `stdin`, and can perform output just like
//!    using the standard library
//! 5. after app exits, shell would set EOF flags to its `stdin`, `stdout` and `stderr` queues.
//! 6. if all apps in a job exit, app shell removes all the structure stored in `app_io` and
//!    destructs all stdio queues

#![no_std]

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate stdio;
extern crate spin;
#[macro_use] extern crate alloc;
extern crate keycodes_ascii;
extern crate libterm;
extern crate scheduler;
extern crate serial_port;
extern crate core_io;
extern crate window;
extern crate frame_buffer_alpha;
extern crate window_components;
extern crate window_manager_alpha;
extern crate window_manager;
extern crate text_area;
extern crate text_display;
extern crate frame_buffer;

use window::Window;
use frame_buffer::Coord;
use text_area::TextArea;
use text_display::TextGeneric;
use frame_buffer_alpha::FrameBufferAlpha;
use stdio::{StdioReader, StdioWriter, KeyEventReadGuard,
            KeyEventQueueReader};
use spin::{Mutex, MutexGuard};
use alloc::collections::BTreeMap;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::string::String;
use alloc::vec::Vec;
use libterm::{Terminal};
use libterm::cursor::{Cursor, CursorGeneric, CursorComponent};
use window_components::WindowComponents;

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
    use window_components::WindowComponents;

    lazy_static! {
        /// Map applications to their IoControlFlags structure. Here the key is the task_id
        /// of each task. When shells call `insert_child_streams`, the default value is
        /// automatically inserted for that task. When shells call `remove_child_streams`,
        /// the corresponding structure is removed from this map.
        static ref APP_IO_CTRL_FLAGS: Mutex<BTreeMap<usize, IoControlFlags>> = Mutex::new(BTreeMap::new());
    }

    lazy_static! {
        /// Map applications to their IoStreams structure. Here the key is the task_id of
        /// each task. Shells should call `insert_child_streams` when spawning a new app,
        /// which effectively stores a new key value pair to this map. After applications
        /// exit, shells should call `remove_child_streams` to clean up.
        static ref APP_IO_STREAMS: Mutex<BTreeMap<usize, IoStreams>> = Mutex::new(BTreeMap::new());
    }

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
    pub fn lock_all_maps() -> (MutexGuard<'static, BTreeMap<usize, IoControlFlags>>, MutexGuard<'static, BTreeMap<usize, IoStreams>>) {
        (APP_IO_CTRL_FLAGS.lock(), APP_IO_STREAMS.lock())
    }
}

lazy_static! {
    /// The default terminal.
    static ref DEFAULT_TERMINAL: Option<Arc<Mutex<Terminal>>> = {

        // Requests a new window object from the window manager
        #[cfg(not(generic_display_sys))]       
        let (window, textarea, cursor) = { 
            let (window_width, window_height) = match window_manager_alpha::get_screen_size(){
                Ok(size) => size,
                Err(err) => { debug!("Fail to create the framebuffer"); return None; }
            };

            const WINDOW_MARGIN: usize = 20;
            let framebuffer = match FrameBufferAlpha::new(window_width - 2*WINDOW_MARGIN, window_height - 2*WINDOW_MARGIN, None){
                Ok(fb) => fb,
                Err(err) => { debug!("Fail to create the framebuffer"); return None; }
            };
            let window = match window_components::WindowComponents::new(
                Coord::new(WINDOW_MARGIN as isize, WINDOW_MARGIN as isize), Box::new(framebuffer)) {
                Ok(window_object) => { window_object },
                Err(err) => {
                    debug!("new window returned err"); 
                    return None;
                }
            };
            let textarea = {
                let (width_inner, height_inner) = window.inner_size();
                debug!("new window done width: {}, height: {}", width_inner, height_inner);
                // next add textarea to wincomps
                const TEXTAREA_BORDER: usize = 4;
                match TextArea::new(
                    Coord::new((window.get_border_size() + TEXTAREA_BORDER) as isize, (window.get_title_size() + TEXTAREA_BORDER) as isize),
                    width_inner - 2*TEXTAREA_BORDER, height_inner - 2*TEXTAREA_BORDER,
                    &window.winobj, None, None, Some(window.get_background()), None
                ) {
                    Ok(m) => m,
                    Err(err) => { debug!("new textarea returned err");  return None; }
                }
            };

            let cursor = CursorComponent::new();

            (window, textarea, cursor)
        };
        
        #[cfg(generic_display_sys)]       
        let (window, textarea, cursor) = {
            let window = match window_manager::new_default_window() {
            Ok(window_object) => window_object,
            Err(err) => { return None }
            };

            let (width, height) = window.dimensions();
            let width = width - 2 * window_manager::WINDOW_MARGIN;
            let height = height - 2 * window_manager::WINDOW_MARGIN;
            let textarea = match TextGeneric::new(width, height, libterm::FONT_COLOR, libterm::BACKGROUND_COLOR) {
                Ok(text) => text,
                Err(_) => { return None }
            };

            let cursor = CursorGeneric::new();

            (window, textarea, cursor)
        };

        match Terminal::new(
            Box::new(window), 
            Box::new(textarea),
            Box::new(cursor),
        ) {
            Ok(terminal) => Some(Arc::new(Mutex::new(terminal))),
            Err(_) => {trace!("Err"); None}
        }
    };
}

/// Applications call this function to get the terminal to which it should print.
/// If the calling application has already been assigned a terminal to print, that assigned
/// terminal is returned. Otherwise, the default terminal is assigned to the calling application
/// and then returned.
pub fn get_terminal_or_default() -> Result<Arc<Mutex<Terminal>>, &'static str> {
    
    let task_id = task::get_my_current_task_id()
                      .ok_or("Cannot get task ID for getting default terminal")?;

    if let Some(property) = shared_maps::lock_stream_map().get(&task_id) {
        return Ok(Arc::clone(&property.terminal));
    }

    loop {
        match *DEFAULT_TERMINAL {
            Some(ref terminal) => return Ok(Arc::clone(&terminal)),
            _ => { 
                error!("Failed to get default terminal, retrying...");
            }
        }
        scheduler::schedule(); // yield the CPU and try again later
    }
}

/// Lock all shared states (i.e. those defined in `lazy_static!`) and execute the closure.
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

/// The main printing macro, which simply pushes an output event to the input_event_manager's event queue. 
/// This ensures that only one thread (the input_event_manager acting as a consumer) ever accesses the GUI.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ({
        $crate::print_to_stdout_args(format_args!($($arg)*));
    });
}

use core::fmt;
use core_io::Write;
/// Converts the given `core::fmt::Arguments` to a `String` and enqueues the string into the correct
/// terminal print-producer
pub fn print_to_stdout_args(fmt_args: fmt::Arguments) {
    let task_id = match task::get_my_current_task_id() {
        Some(task_id) => task_id,
        None => {
            // We cannot use log macros here, because when they're mirrored to the vga, they will cause
            // infinite loops on an error. Instead, we write directly to the serial port. 
            let _ = serial_port::write_fmt_log(
                "\x1b[31m", "[E] ",
                format_args!("error in print!/println! macro: failed to get current task id"), "\x1b[0m\n"
            );
            return;
        }
    };

    // Obtains the correct stdout stream and push the output bytes.
    let locked_streams = shared_maps::lock_stream_map();
    match locked_streams.get(&task_id) {
        Some(queues) => {
            if let Err(_) = queues.stdout.lock().write_all(format!("{}", fmt_args).as_bytes()) {
                let _ = serial_port::write_fmt_log(
                    "\x1b[31m", "[E] ",
                    format_args!("failed to write to stdout"), "\x1b[0m\n"
                );
            }
        },
        None => {
            let _ = serial_port::write_fmt_log(
                "\x1b[31m", "[E] ",
                format_args!("error in print!/println! macro: no stdout queue for current task"), "\x1b[0m\n"
            );
            return;
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
