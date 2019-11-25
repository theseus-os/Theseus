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
extern crate window_manager_alpha;

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
use window::Window;
use text_primitive::TextPrimitive;

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

    // create the instance of WindowComponents, which provides basic drawing of a window and basic response to user input
    let framebuffer = match FrameBufferAlpha::new(width, height, None) {
        Ok(fb) => fb,
        Err(err) => {
            error!("{}", err);
            return -2;
        }
    };
    let mut wincomps = match window_components::WindowComponents::new(
        coordinate,
        Box::new(framebuffer), // the position and size of window, including the title bar and border
        WINDOW_BACKGROUND
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

    // refresh all the characters to test performance,
    // for c in ('a' as u8) .. ('z' as u8 + 1) {
    //     for i in 0 .. textarea.get_x_cnt() {
    //         for j in 0 .. textarea.get_y_cnt() {
    //             match textarea.set_char(i, j, c) {
    //                 Ok(_) => {}
    //                 Err(_) => {debug!("set char failed"); return -4; }
    //             }
    //         }
    //     }
    // }

//     // prepare for display chars
//     let mut char_matrix: Vec<u8> = Vec::new(); // the text that should be displayed
//     let text_cnt: usize = textarea.get_x_cnt() * textarea.get_y_cnt(); // the total count of chars in textarea
//     char_matrix.resize(text_cnt, ' ' as u8); // fill in the textarea with blank char

//     // prepare for user-friendly cursor display
//     let mut text_cursor: usize = 0; // the current cursor position
//     const CURSOR_CHAR: u8 = 221; // cursor char, refer to font.rs
//     const BLINK_INTERVAL: u64 = 50000000; // the interval to display a blink of cursor, for better user experience
//     let start_time: u64 = get_time(); // used to compute blink of cursor
//     let mut cursor_last_char: u8 = ' ' as u8; // store the char that is overwritten by cursor, to support arbitrary cursor movement

//     loop {
//         // first let WindowComponents to handle basic user inputs, and leaves those unhandled events
//         if let Err(err) = wincomps.handle_event() {
//             debug!("{}", err); // when user click close button, this will trigger, and simply exit the program
//             return 0;
//         }

//         // handle events of application, like user input text, moving cursor, etc.
//         loop {
//             let _event = match wincomps.consumer.peek() {
//                 Some(ev) => ev,
//                 _ => {
//                     break;
//                 }
//             };
//             match _event.deref() {
//                 &Event::KeyboardEvent(ref input_event) => {
//                     let key_event = input_event.key_event;
//                     if key_event.action == KeyAction::Pressed {
//                         // first handle special keys that allows user to move the cursor and delete chars
//                         if key_event.keycode == Keycode::Backspace {
//                             let new_cursor = (text_cursor + text_cnt - 1) % text_cnt;
//                             char_matrix[new_cursor] = ' ' as u8; // set last char to ' '
//                             move_mouse_restore_old(
//                                 &mut char_matrix,
//                                 &mut text_cursor,
//                                 &mut cursor_last_char,
//                                 new_cursor,
//                             );
//                         } else if key_event.keycode == Keycode::Enter {
//                             let new_cursor = ((text_cursor / textarea.get_x_cnt() + 1)
//                                 * textarea.get_x_cnt())
//                                 % text_cnt;
//                             move_mouse_restore_old(
//                                 &mut char_matrix,
//                                 &mut text_cursor,
//                                 &mut cursor_last_char,
//                                 new_cursor,
//                             );
//                         } else if key_event.keycode == Keycode::Up {
//                             let new_cursor =
//                                 ((text_cursor / textarea.get_x_cnt() + textarea.get_y_cnt() - 1)
//                                     * textarea.get_x_cnt()
//                                     + (text_cursor % textarea.get_x_cnt()))
//                                     % text_cnt;
//                             move_mouse_restore_old(
//                                 &mut char_matrix,
//                                 &mut text_cursor,
//                                 &mut cursor_last_char,
//                                 new_cursor,
//                             );
//                         } else if key_event.keycode == Keycode::Down {
//                             let new_cursor = ((text_cursor / textarea.get_x_cnt() + 1)
//                                 * textarea.get_x_cnt()
//                                 + (text_cursor % textarea.get_x_cnt()))
//                                 % text_cnt;
//                             move_mouse_restore_old(
//                                 &mut char_matrix,
//                                 &mut text_cursor,
//                                 &mut cursor_last_char,
//                                 new_cursor,
//                             );
//                         } else if key_event.keycode == Keycode::Left {
//                             let new_cursor = ((text_cursor / textarea.get_x_cnt())
//                                 * textarea.get_x_cnt()
//                                 + (((text_cursor % textarea.get_x_cnt()) + textarea.get_x_cnt()
//                                     - 1)
//                                     % textarea.get_x_cnt()))
//                                 % text_cnt;
//                             move_mouse_restore_old(
//                                 &mut char_matrix,
//                                 &mut text_cursor,
//                                 &mut cursor_last_char,
//                                 new_cursor,
//                             );
//                         } else if key_event.keycode == Keycode::Right {
//                             let new_cursor = ((text_cursor / textarea.get_x_cnt())
//                                 * textarea.get_x_cnt()
//                                 + (((text_cursor % textarea.get_x_cnt()) + 1)
//                                     % textarea.get_x_cnt()))
//                                 % text_cnt;
//                             move_mouse_restore_old(
//                                 &mut char_matrix,
//                                 &mut text_cursor,
//                                 &mut cursor_last_char,
//                                 new_cursor,
//                             );
//                         } else {
//                             match key_event.keycode.to_ascii(key_event.modifiers) {
//                                 Some(c) => {
//                                     // for normal keys, just display them and move the cursor forward
//                                     char_matrix[text_cursor] = c as u8;
//                                     text_cursor = (text_cursor + 1) % text_cnt;
//                                     cursor_last_char = char_matrix[text_cursor]
//                                 }
//                                 _ => {}
//                             }
//                         }
//                     }
//                 }
//                 _ => {}
//             }
//             _event.mark_completed(); // always consume the event, and ignore those unknown ones
//         }

//         // make cursor blink by computing the time from start
//         let timidx = (get_time() - start_time) / BLINK_INTERVAL;
//         if timidx % 2 == 0 {
//             char_matrix[text_cursor] = CURSOR_CHAR;
//         } else {
//             char_matrix[text_cursor] = ' ' as u8;
//         }

//         // update char matrix for textarea to display, this is efficient that will only redraw the changed chars
//         if let Err(err) = textarea.set_char_matrix(&char_matrix) {
//             error!("set char matrix failed: {}", err);
//             return -5;
//         }

//         // be nice to other applications
//         scheduler::schedule();
//     }
// }

// // get current time for cursor blinking
// fn get_time() -> u64 {
//     match get_hpet().as_ref() {
//         Some(m) => m.get_counter(),
//         _ => {
//             error!("couldn't get HPET timer");
//             0
//         }
//     }
// }

// // set cursor to a new position and restore the old one
// fn move_mouse_restore_old(
//     char_matrix: &mut Vec<u8>,
//     text_cursor: &mut usize,
//     cursor_last_char: &mut u8,
//     new_cursor: usize,
// ) {
//     char_matrix[*text_cursor] = *cursor_last_char; // first restore the previous char
//     *text_cursor = new_cursor; // update new cursor
//     *cursor_last_char = char_matrix[*text_cursor]; // record the current char for later restore
// }
