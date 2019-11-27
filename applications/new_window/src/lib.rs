//! This application is to create a new window with given size.
//!
//! usage:      x y width height (unit is pixel)
//!
//! The new window has a half-transparent background. This simple application is to test `WindowManager` with multiple window overlapping each other,
//! as well as test `WindowComponents` which provides easy-to-use interface for application to enable GUI.

#![no_std]
#[macro_use]
extern crate log;
extern crate alloc;

extern crate dfqueue;
extern crate runqueue;
extern crate spawn;
extern crate window;
extern crate window_manager;
extern crate frame_buffer_alpha;
extern crate print;
extern crate window_components;
extern crate frame_buffer;
extern crate scheduler;

use alloc::string::String;
use alloc::vec::Vec;
use frame_buffer::Coord;

const WINDOW_BACKGROUND: u32 = 0x40FFFFFF;

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    if _args.len() != 4 {
        debug!("parameter not 5");
        return 0;
    }

    // take arguments as the parameter to create a window
    let coordinate = Coord::new(
        _args[0].parse::<isize>().unwrap(),
        _args[1].parse::<isize>().unwrap(),
    );
    let width = _args[2].parse::<usize>().unwrap();
    let height = _args[3].parse::<usize>().unwrap();
    debug!("parameters {:?}", (coordinate, width, height));

    let mut wincomps = match window_components::WindowComponents::new(
        coordinate,
        width,
        height,
        WINDOW_BACKGROUND,
        &frame_buffer_alpha::new,
    ) {
        Ok(m) => m,
        Err(err) => {
            error!("new window components returned err: {}", err);
            return -2;
        }
    };

    loop {
        if let Err(err) = wincomps.handle_event() {
            debug!("{}", err); 
            return 0;
        }
    }
}
