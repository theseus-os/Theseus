//! This crate manages application I/O related affairs, including byte input/output queue
//! (stdin/stdout), raw keyboard event input queue, and several flags controlling I/O behaviors.
//! 
//! When shell is about to spawn a new task, it must first keep it in a *blocked* state. Shell
//! must set up the IOProperty for the new task, and temporarily saves it to this crate by
//! calling `deposit_default_property`. Only after that can shell unblock the new task, (i.e.
//! sets it to runnable).
//! 
//! Newly spawned task should claim its IOProperty on start up by calling `claim_property`.
//! It will transfer the Arc pointer to the new task, and only leave a weak pointer in this crate.
//! 
//! All I/O interfaces provided to applications come in two form: state-spill-free and
//! non-state-spill-free. State-spill-free version requires the calling application to provide
//! its own IOProperty via reference, while non-state-spill-free version takes advantage of
//! the stored weak pointer in this crate. Which one is more handy should depend on particular
//! tasks.
//! 
//! Non-state-spill-free version won't work if this crate undergoes a crash or an update, since
//! all stored weak pointers will be lost. To recover from it, applications should call
//! `restore_property` to save another copy of weak pointer to this crate. After that,
//! non-state-spill-free version should work properly.
//! 
//! To avoid memory leaks, shell must take responsibility to remove outdated pointers in this
//! crate when spawned tasks exit, by calling `remove_property`.
#![no_std]
#![feature(asm)]

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate task;
extern crate dfqueue;
extern crate event_types;
extern crate spin;
extern crate keycodes_ascii;

use keycodes_ascii::KeyEvent;
use dfqueue::{DFQueueConsumer, DFQueueProducer};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;
use alloc::boxed::Box;
use alloc::sync::{Arc, Weak};
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering::SeqCst;

/// IOProperty stores all I/O related information, including input and output queues and
/// several flags controling corresponding shell and terminal behavior. During normal
/// application execution, strong pointers (i.e. Arc) to this structure are stored in
/// the application itself. This crate and shell should only keep weak pointers to them.
pub struct IOProperty {
    /// Indicating if the application wants to access the raw keyboard event.
    pub direct_key_access_flag: AtomicBool,
    /// Indicating if the application wants the shell to deliver character from stdin
    /// immedately rather than waiting for user striking enter.
    pub immediate_delivery_flag: AtomicBool,
    /// Indicating if the application wants the shell to echo characters to the terminal.
    /// On password input, applications should set this flag to true to prevent password
    /// from appearing on the screen.
    pub no_shell_echo_flag: AtomicBool,
    /// Applications write to this queue to output. (stdout)
    pub output_byte_producer: DFQueueProducer<u8>,
    /// Applications read from this queue to access raw keyboard events.
    pub key_event_consumer: DFQueueConsumer<KeyEvent>,
    /// Applications read from this queue for input. (stdin)
    pub input_byte_consumer: DFQueueConsumer<u8>,
}

// IMPORTANT: To avoid deadlock, if multiple structures are to be locked, we must lock them in the sequence
// same as the following definition.

lazy_static! {
    /// The shell stores the IOProperty in this map for the spawned application to claim. All stored
    /// properties here are enclosed by an Arc (strong) pointer. When the application claims its property,
    /// a weak pointer will be left at PROPERTY_DEPOSIT_WEAK.
    static ref PROPERTY_DEPOSIT_STRONG: Mutex<BTreeMap<usize, Arc<IOProperty>>> =
        Mutex::new(BTreeMap::new());
}

lazy_static! {
    /// This map stores all IOProperties that have already been claimed by applications. All weak pointers
    /// contained in this map should correspond to running applications. Shells should take care of
    /// removing them from this map (and also any unclaimed property in PROPERTY_DEPOSIT_STRONG) when
    /// tasks exit.
    static ref PROPERTY_DEPOSIT_WEAK: Mutex<BTreeMap<usize, Weak<IOProperty>>> =
        Mutex::new(BTreeMap::new());
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

/// Get the weak pointer to the task's IOProperty. This is the function which needs the spilled
/// states, i.e. those weak pointers. If this crate crashes or gets runtime-updated, the stored
/// weak pointers will be lost. To recover from it, we provide the `restore_property` function.
fn get_stored_property(task_id: &usize) -> Option<Arc<IOProperty>> {
    let map = PROPERTY_DEPOSIT_WEAK.lock();
    let result = map.get(&task_id);
    if let Some(weak) = result {
        if let Some(property) = weak.upgrade() {
            return Some(property);
        }
    }
    None
}

/// Shell calls this function to deposit an default IOProperty for the spawned application to claim.
/// By saying "default", it means the flags stored in the IOProperty are all set to false, which is
/// the default case where the application only read from stdin and print to stdout, where the input
/// characters are delivered only upon user striking the enter key.
pub fn deposit_default_property(child_task_id: usize,
                                app_output_producer: DFQueueProducer<u8>,
                                keyboard_event_consumer: DFQueueConsumer<KeyEvent>,
                                input_byte_consumer: DFQueueConsumer<u8>) -> Result<Weak<IOProperty>, &'static str> {
    let property = Arc::new(IOProperty {
        direct_key_access_flag: AtomicBool::new(false),
        immediate_delivery_flag: AtomicBool::new(false),
        no_shell_echo_flag: AtomicBool::new(false),
        output_byte_producer: app_output_producer,
        key_event_consumer: keyboard_event_consumer,
        input_byte_consumer: input_byte_consumer,
    });
    let weak_property = Arc::downgrade(&property);
    PROPERTY_DEPOSIT_STRONG.lock().insert(child_task_id, property);
    Ok(weak_property)
}

/// Applications call this function to claim its IOProperty back. It will leave a copy of weak
/// pointer pointing to its claimed property in PROPERTY_DEPOSIT_WEAK.
pub fn claim_property() -> Result<Arc<IOProperty>, &'static str> {

    let task_id = get_task_id_or_error("Cannot get task ID for claiming IOProperty")?;

    if let Some(property) = PROPERTY_DEPOSIT_STRONG.lock().remove(&task_id) {
        PROPERTY_DEPOSIT_WEAK.lock().insert(task_id, Arc::downgrade(&property));
        return Ok(property);
    }

    Err("Failed to claim IOProperty for the current task. It does not exist.")
}

/// Applications call this function to help this crate recover from a crash. Some macros rely
/// on spilled states, e.g. those weak pointers stored in PROPERTY_DEPOSIT_WEAK. These macros
/// are `println!` and `print!`. After a crash of this crate (or maybe an update), weak pointers
/// are lost, so these macros don't know where the argument string should be printed to, and
/// will return an Err. Applications can then call this function to store a copy of weak pointer
/// to this crate, and restore proper function of these macros.
pub fn restore_property(io_property: &Arc<IOProperty>) -> Result<(), &'static str> {

    let task_id = get_task_id_or_error("Cannot get task ID for IOProperty recovery")?;

    PROPERTY_DEPOSIT_WEAK.lock().insert(task_id, Arc::downgrade(io_property));
    Ok(())
}

/// Shell calls this function to remove outdated property pointers (those belong to exited
/// applications).
pub fn remove_property(task_id: usize) {
    PROPERTY_DEPOSIT_STRONG.lock().remove(&task_id);
    PROPERTY_DEPOSIT_WEAK.lock().remove(&task_id);
}

/// Applications call this function to enqueue a vector of bytes to stdout. This function
/// depends on spilled weak pointers stored in this crate, specifically in PROPERTY_DEPOSIT_WEAK.
/// We provide a way to recover those lost weak pointers after crashes, see function
/// `restore_property` for details.
pub fn put_output_bytes_spilled(bytes: Vec<u8>) -> Result<(), &'static str> {

    let task_id = get_task_id_or_error("Cannot get task ID for output bytes enqueuing")?;
    match get_stored_property(&task_id) {
        Some(io_property) => {
            put_output_bytes(&io_property, bytes);
            Ok(())
        },
        None => Err("No stored IOProperty for current task")
    }
}

/// Applications call this function to enqueue a vector of bytes to stdout. This is the
/// state spill free version, but requires the application to provide an reference to its
/// own IOProperty.
pub fn put_output_bytes(io_property: &Arc<IOProperty>, bytes: Vec<u8>) {
    for byte in bytes {
        io_property.output_byte_producer.enqueue(byte);
    }
}

/// Applications call this function to read a vector of bytes from stdin. If currently there
/// is nothing is the queue, then Ok(None) will be returned. This function
/// depends on spilled weak pointers stored in this crate, specifically in PROPERTY_DEPOSIT_WEAK.
/// We provide a way to recover those lost weak pointers after crashes, see function
/// `restore_property` for details.
pub fn get_input_bytes_spilled() -> Result<Option<Vec<u8>>, &'static str> {

    let task_id = get_task_id_or_error("Cannot get task ID for input bytes reading")?;
    match get_stored_property(&task_id) {
        Some(io_property) => get_input_bytes(&io_property),
        None => Err("No stored IOProperty for current task")
    }
}

/// Applications call this function to read a vector of bytes from stdin. If currently there
/// is nothing is the queue, then Ok(None) will be returned. This is the state spill free
/// version, but requires the application to provide an reference to its own IOProperty.
pub fn get_input_bytes(io_property: &Arc<IOProperty>) -> Result<Option<Vec<u8>>, &'static str> {
    let mut data: Vec<u8> = Vec::new();
    while let Some(b) = io_property.input_byte_consumer.peek() {
        b.mark_completed();
        data.push(*b);
    }
    if data.is_empty() {
        return Ok(None);
    }
    Ok(Some(data))
}

/// Applications call this function to get a keyboard event directed by the shell.
/// If nothing is currently in the queue, this function returns Ok(None). This function
/// depends on spilled weak pointers stored in this crate, specifically in PROPERTY_DEPOSIT_WEAK.
/// We provide a way to recover those lost weak pointers after crashes, see function
/// `restore_property` for details.
pub fn get_keyboard_event_spilled() -> Result<Option<KeyEvent>, &'static str> {

    let task_id = get_task_id_or_error("Cannot get task ID for raw keyboard event accessing")?;
    match get_stored_property(&task_id) {
        Some(io_property) => get_keyboard_event(&io_property),
        None => Err("No stored IOProperty for current task")
    }
}

/// Applications call this function to get a keyboard event directed by the shell.
/// If nothing is currently in the queue, this function returns Ok(None). This is the
/// state spill free version, but requires the application to provide an reference to
/// its own IOProperty.
pub fn get_keyboard_event(io_property: &Arc<IOProperty>) -> Result<Option<KeyEvent>, &'static str> {
    use core::ops::Deref;
    if let Some(peeked_data) = io_property.key_event_consumer.peek() {
        peeked_data.mark_completed();
        return Ok(Some(*peeked_data.deref()));
    }
    Ok(None)
}


/// Calls `print!()` with an extra newline ('\n') appended to the end. This macro depends on some
/// spilled states stored in PROPERTY_DEPOSIT_WEAK. See put_output_bytes_spilled for details.
#[macro_export]
macro_rules! println {
    ($fmt:expr) => (print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"), $($arg)*));
}

/// The main printing macro, which pushes an output string to the stdout. This macro depends on some
/// spilled states stored in PROPERTY_DEPOSIT_WEAK. See put_output_bytes_spilled for details.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ({
        $crate::print_to_stdout_args_spilled(format_args!($($arg)*))
    });
}

/// Calls `print!()` with an extra newline ('\n') appended to the end. This is the state spill free (ssf)
/// version, but requires the application to provide an reference to its own IOProperty.
#[macro_export]
macro_rules! ssfprintln {
    ($prop:expr, $fmt:expr) => (ssfprint!($prop, concat!($fmt, "\n")));
    ($prop:expr, $fmt:expr, $($arg:tt)*) => (ssfprint!($prop, concat!($fmt, "\n"), $($arg)*));
}

/// The main printing macro, which pushes an output string to the stdout. This is the state spill free (ssf)
/// version, but requires the application to provide an reference to its own IOProperty.
#[macro_export]
macro_rules! ssfprint {
    ($prop:expr, $($arg:tt)*) => ({
        $crate::print_to_stdout_args($prop, format_args!($($arg)*))
    });
}

/// Lock all shared states (i.e. those defined in `lazy_static!`) and execute the closure.
/// This function is mainly provided for the shell to clean up the pointers stored in the map
/// without causing a deadlock. In other words, by using this function, shell can prevent the
/// application from holding the lock of these shared maps before killing it. Otherwise, the
/// lock will never get a chance to be released. Since we currently don't have stack unwinding.
pub fn locked_and_execute(f: Box<Fn()>) {
    let _strong_property_guard = PROPERTY_DEPOSIT_STRONG.lock();
    let _weak_property_guard = PROPERTY_DEPOSIT_WEAK.lock();
    f();
}
/// Applications call this function to request the shell to forward keyboard events to them.
pub fn request_kbd_event_forward(io_property: &Arc<IOProperty>) {
    io_property.direct_key_access_flag.store(true, SeqCst);
}

/// Applications call this function to stop the shell forwarding keyboards events to them.
/// Instead, let the shell handle input events.
pub fn stop_kbd_event_forward(io_property: &Arc<IOProperty>) {
    io_property.direct_key_access_flag.store(false, SeqCst);
}

/// Applications call this function to request the shell to deliver characters immediately on user
/// keyboard stike, rather than waiting for an `enter`.
pub fn request_immediate_delivery(io_property: &Arc<IOProperty>) {
    io_property.immediate_delivery_flag.store(true, SeqCst);
}

/// Applications call this function to stop the shell immediately delivering characters upon user keystrike.
/// Instead, the shell waits for an `enter` and delivers all buffered characters together.
pub fn stop_immediate_delivery(io_property: &Arc<IOProperty>) {
    io_property.immediate_delivery_flag.store(false, SeqCst);
}

/// Applications call this function to disable the shell from echoing characters to the terminal screen.
pub fn request_no_echo(io_property: &Arc<IOProperty>) {
    io_property.no_shell_echo_flag.store(true, SeqCst);
}

/// Applications call this function to re-enable the shell to echo characters to the terminal screen.
pub fn stop_no_echo(io_property: &Arc<IOProperty>) {
    io_property.no_shell_echo_flag.store(false, SeqCst);
}

/// Converts the given `core::fmt::Arguments` to a `String` and enqueues the string into the correct terminal print-producer
use core::fmt;
pub fn print_to_stdout_args_spilled(fmt_args: fmt::Arguments) -> Result<(), &'static str> {
    put_output_bytes_spilled(format!("{}", fmt_args).into_bytes())
}

/// Converts the given `core::fmt::Arguments` to a `String` and enqueues the string into the correct terminal print-producer
/// This is the state spill free version, but requires the application to provide an reference to its
/// own IOProperty.
pub fn print_to_stdout_args(io_property: &Arc<IOProperty>, fmt_args: fmt::Arguments) {
    put_output_bytes(io_property, format!("{}", fmt_args).into_bytes())
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
