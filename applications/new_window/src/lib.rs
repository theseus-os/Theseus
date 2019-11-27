//! This application is to create a new window with given size, and user could edit text on it
//!
//! usage:      x y width height (unit is pixel)
//!
//! This simple application is to test `WindowManagerAlpha` with multiple window overlapping each other,
//! as well as test `WindowComponents` which provides easy-to-use interface for application to enable GUI.
//!
//! User could edit text in this window. Special keys are supported in this simple editor, such as moving up, down, left and right.
//! Other basic operations like backspace and new-line is also supported.
//!
//! This application could also be used to test performance, by uncomment the code block that refreshing chars from `a` to `z`.
//! You would notice that even if refreshing all the chars is slow, it is quite fast when you editing texts, thanks to partial refreshing
//! mechanism supported by both `WindowManagerAlpha` and `WindowComponents`

#![no_std]
#[macro_use]
extern crate log;
extern crate alloc;

extern crate dfqueue;
extern crate event_types;
extern crate hpet;
extern crate keycodes_ascii;
extern crate runqueue;
extern crate spawn;
extern crate window;
extern crate window_manager;

extern crate print;
extern crate window_components;

extern crate frame_buffer;
extern crate frame_buffer_alpha;
extern crate scheduler;
extern crate text_primitive;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Deref;
use event_types::Event;
use frame_buffer::Coord;
use frame_buffer_alpha::FrameBufferAlpha;
use hpet::get_hpet;
use keycodes_ascii::{KeyAction, Keycode};
use text_primitive::TextPrimitive;

const WINDOW_BACKGROUND: u32 = 0x20FFFFFF;
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
        height, // the position and size of window, including the title bar and border
        WINDOW_BACKGROUND,
        &frame_buffer_alpha::new
    ) {
        Ok(m) => m,
        Err(err) => {
            error!("new window components returned err: {}", err);
            return -2;
        }
    };

    // get the actual inner size for user to put components
    let (width_inner, height_inner) = wincomps.inner_size();
    debug!(
        "new window done width: {}, height: {}",
        width_inner, height_inner
    );

    // let mut textarea = match TextPrimitive::new(
    //     width_inner - 8, 
    //     height_inner - 8, 
    //     0xFFFFFF, 
    //     0xFFFFFF
    // ) {
    //     Ok(text) => text,
    //     Err(err) => { 
    //         debug!("Fail to create the generic window: {}", err);
    //         return -3;
    //     }
    // };

    // let _ = wincomps.add_displayable("text", Coord::new(0, 0), Box::new(textarea));

    
    loop {
        // first let WindowComponents to handle basic user inputs, and leaves those unhandled events
        if let Err(err) = wincomps.handle_event() {
            debug!("{}", err); // when user click close button, this will trigger, and simply exit the program
            return 0;
        }
    }
}