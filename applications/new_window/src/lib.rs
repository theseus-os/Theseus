//! thie application is to create a new window with given size
//! 

#![no_std]
#[macro_use] extern crate log;
extern crate alloc;

extern crate keycodes_ascii;
extern crate dfqueue;
extern crate spawn;
extern crate runqueue;
extern crate event_types; 
extern crate window_manager_alpha;
extern crate hpet;

extern crate print;
extern crate window_components;

extern crate scheduler;

use event_types::{Event};
use keycodes_ascii::{Keycode, KeyAction};
use alloc::string::{String};
use alloc::vec::Vec;
use core::ops::{Deref};
use hpet::get_hpet;

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {

    if _args.len() != 4 {
        debug!("parameter not 5");
        return 0;
    }

    let x = _args[0].parse::<usize>().unwrap();
    let y = _args[1].parse::<usize>().unwrap();
    let width = _args[2].parse::<usize>().unwrap();
    let height = _args[3].parse::<usize>().unwrap();
    debug!("parameters {:?}", (x, y, width, height));

    let _wincomps = match window_components::WindowComponents::new(
        x, y, width, height
    ) {
        Ok(m) => m,
        Err(err) => { debug!("new window components returned err: {}", err); return -2; }
    };
    let mut wincomps = _wincomps.lock();
    let (width_inner, height_inner) = wincomps.inner_size();
    debug!("new window done width: {}, height: {}", width_inner, height_inner);
    // next add textarea to wincomps
    let _textarea = match window_components::TextArea::new(
        wincomps.bias_x + 4, wincomps.bias_y + 4, width_inner - 8, height_inner - 8,
        &wincomps.winobj, None, None, Some(wincomps.background), None
    ) { 
        Ok(m) => m,
        Err(err) => { debug!("new textarea returned err: {}", err); return -3; }
    };
    // refresh all the charaters to test performance
    let mut textarea = _textarea.lock();
    // for c in ('a' as u8) .. ('z' as u8 + 1) {
    //     for i in 0 .. textarea.x_cnt {
    //         for j in 0 .. textarea.y_cnt {
    //             match textarea.set_char(i, j, c) {
    //                 Ok(_) => {}
    //                 Err(_) => {debug!("set char failed"); return -4; }
    //             }
    //         }
    //     }
    // }

    debug!("all done");

    let mut char_matrix: Vec<u8> = Vec::new();
    let mut text_cursor: usize = 0;
    let text_cnt: usize = textarea.x_cnt * textarea.y_cnt;
    char_matrix.resize(text_cnt, ' ' as u8);
    // for c in 0 as usize .. 256 as usize {
    //     char_matrix[c] = c as u8;
    // }
    // char_matrix[0] = 221;

    const CURSOR_CHAR: u8 = 221;
    const BLINK_INTERVAL: u64 = 50000000;
    let start_time: u64 = get_time();
    let mut cursor_last_char: u8 = ' ' as u8;

    loop {
        wincomps.handle_event();

        // then do my work here
        loop {
            let _event = match wincomps.consumer.peek() {
                Some(ev) => ev,
                _ => { break; }
            };
            match _event.deref() {
                &Event::InputEvent(ref input_event) => {
                    let key_event = input_event.key_event;
                    if key_event.action == KeyAction::Pressed {
                        if key_event.keycode == Keycode::Backspace {
                            char_matrix[(text_cursor + text_cnt - 1) % text_cnt] = ' ' as u8;
                            char_matrix[text_cursor] = cursor_last_char;
                            text_cursor = (text_cursor + text_cnt - 1) % text_cnt;
                            cursor_last_char = char_matrix[text_cursor]
                        } else if key_event.keycode == Keycode::Enter {
                            char_matrix[text_cursor] = cursor_last_char;
                            text_cursor = ((text_cursor / textarea.x_cnt + 1) * textarea.x_cnt) % text_cnt;
                            cursor_last_char = char_matrix[text_cursor]
                        } else if key_event.keycode == Keycode::Up {
                            char_matrix[text_cursor] = cursor_last_char;
                            text_cursor = ((text_cursor / textarea.x_cnt + textarea.y_cnt - 1) * textarea.x_cnt
                                + (text_cursor % textarea.x_cnt)) % text_cnt;
                            cursor_last_char = char_matrix[text_cursor]
                        } else if key_event.keycode == Keycode::Down {
                            char_matrix[text_cursor] = cursor_last_char;
                            text_cursor = ((text_cursor / textarea.x_cnt + 1) * textarea.x_cnt
                                + (text_cursor % textarea.x_cnt)) % text_cnt;
                            cursor_last_char = char_matrix[text_cursor]
                        } else if key_event.keycode == Keycode::Left {
                            char_matrix[text_cursor] = cursor_last_char;
                            text_cursor = ((text_cursor / textarea.x_cnt) * textarea.x_cnt 
                                + (((text_cursor % textarea.x_cnt) + textarea.x_cnt - 1) % textarea.x_cnt)) % text_cnt;
                            cursor_last_char = char_matrix[text_cursor]
                        } else if key_event.keycode == Keycode::Right {
                            char_matrix[text_cursor] = cursor_last_char;
                            text_cursor = ((text_cursor / textarea.x_cnt) * textarea.x_cnt 
                                + (((text_cursor % textarea.x_cnt) + 1) % textarea.x_cnt)) % text_cnt;
                            cursor_last_char = char_matrix[text_cursor]
                        } else {
                            match key_event.keycode.to_ascii(key_event.modifiers) {
                                Some(c) => {
                                    char_matrix[text_cursor] = c as u8;
                                    text_cursor = (text_cursor + 1) % text_cnt;
                                    cursor_last_char = char_matrix[text_cursor]
                                }
                                _ => { } 
                            }
                        }
                    }
                }
                _ => {}
            }
            _event.mark_completed();
        }

        // make cursor blink
        let timidx = (get_time() - start_time) / BLINK_INTERVAL;
        if timidx % 2 == 0 {
            char_matrix[text_cursor] = CURSOR_CHAR;
        } else {
            char_matrix[text_cursor] = ' ' as u8;
        }

        match textarea.set_char_matrix(&char_matrix) {
            Ok(_) => {}
            Err(err) => {debug!("set char matrix failed: {}", err); return -5; }
        }

        scheduler::schedule();  // do nothing
    }
}

fn get_time() -> u64 {
    match get_hpet().as_ref().ok_or("couldn't get HPET timer") {
        Ok(m) => m.get_counter(),
        Err(_) => 0
    }
}
