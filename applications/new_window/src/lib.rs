//! This application is to create a new window with given size.
//!
//! usage:      x y width height (unit is pixel)
//!
//! The new window has a half-transparent background. This simple application is to test `WindowManager` with multiple window overlapping each other,
//! as well as test `Window` which provides easy-to-use interface for application to enable GUI.
#![no_std]
#[macro_use]
extern crate log;
extern crate alloc;
extern crate color;

extern crate window;
extern crate shapes;
extern crate frame_buffer;

use alloc::string::String;
use alloc::vec::Vec;
use shapes::Coord;

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

    let mut bg_color = color::WHITE;
    bg_color.set_transparency(0x40);

    let mut window = match window::Window::new(
        coordinate,
        width,
        height,
        bg_color,
    ) {
        Ok(m) => m,
        Err(err) => {
            error!("new window components returned err: {}", err);
            return -2;
        }
    };

    loop {
        if let Err(err) = window.handle_event() {
            debug!("{}", err); 
            return 0;
        }
    }
}
