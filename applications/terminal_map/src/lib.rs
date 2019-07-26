//! This crate builds the relationship between applications and terminals.
//! It records which application should print to which terminal.
#![no_std]
#![feature(asm)]

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate libterm;
extern crate task;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::sync::Arc;
use spin::Mutex;
use libterm::Terminal;

lazy_static! {
    /// The mapping between applications and terminals.
    static ref TERMINAL_MAPPING: Arc<Mutex<BTreeMap<usize, Arc<Mutex<Terminal>>>>> =
        Arc::new(Mutex::new(BTreeMap::new()));
}

lazy_static! {
    /// The default terminal. We choose to unwrap here since failing to create a default terminal
    /// is indeed a fatal error.
    static ref DEFAULT_TERMINAL: Arc<Mutex<Terminal>> = Arc::new(Mutex::new(Terminal::new().unwrap()));
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

    if let Some(terminal) = TERMINAL_MAPPING.lock().get(&task_id) {
        return Ok(Arc::clone(&terminal));
    }

    TERMINAL_MAPPING.lock().insert(task_id, DEFAULT_TERMINAL.clone());
    Ok(Arc::clone(&DEFAULT_TERMINAL))
}

/// Applications call this function to set another task to use the same terminal as
/// itself. If the task to be set the terminal has already got a terminal to print,
/// the old one will be returned, otherwise None is returned.
pub fn set_terminal_same_as_myself(task_id: usize) -> Result<Option<Arc<Mutex<Terminal>>>, &'static str> {

    let new_terminal = get_terminal_or_default()?;
    Ok(TERMINAL_MAPPING.lock().insert(task_id, new_terminal))
}

/// Remove the binding between an application and its terminal. If the application
/// to be unbound has no bound terminal, this function performs nothing and returns None.
/// Otherwise, it unbound the tasks with its terminal and then returns its previously
/// assigned terminal.
pub fn remove_terminal(task_id: usize) -> Option<Arc<Mutex<Terminal>>> {
    TERMINAL_MAPPING.lock().remove(&task_id)
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
