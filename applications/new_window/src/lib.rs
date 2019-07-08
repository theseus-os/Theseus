#![no_std]
#[macro_use] extern crate log;
#[macro_use] extern crate alloc;

extern crate keycodes_ascii;
extern crate spin;
extern crate dfqueue;
extern crate spawn;
extern crate runqueue;
extern crate event_types; 
extern crate window_manager_alpha;

extern crate print;
extern crate environment;

extern crate task;
extern crate getopts;
extern crate path;
extern crate fs_node;
extern crate root;
extern crate scheduler;

use getopts::Options;
use fs_node::{FsNode, FileOrDir};

use event_types::{Event};
use keycodes_ascii::{Keycode, KeyAction, KeyEvent};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::sync::Arc;
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use spawn::{ApplicationTaskBuilder, KernelTaskBuilder};
use path::Path;
use task::{TaskRef, ExitValue, KillReason};
use environment::Environment;
use spin::Mutex;
use core::ops::{Deref, DerefMut};

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {

    if (_args.len() != 5) {
        debug!("parameter not 5");
        return 0;
    }

    let x = _args[1].parse::<usize>().unwrap();
    let y = _args[2].parse::<usize>().unwrap();
    let width = _args[3].parse::<usize>().unwrap();
    let height = _args[4].parse::<usize>().unwrap();
    debug!("parameters {:?}", (x, y, width, height));

    let (window_width, window_height) = match window_manager_alpha::get_screen_size() {
        Ok(obj) => obj,
        Err(err) => {debug!("new window returned err"); return -1 }
    };

    let window_object = match window_manager_alpha::new_window(
        x, y, width, height
    ) {
        Ok(window_object) => window_object,
        Err(err) => {debug!("new window returned err"); return -2 }
    };

    loop {
        scheduler::schedule();  // do nothing
    }

    0
}
