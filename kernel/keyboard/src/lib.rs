#![no_std]

extern crate keycodes_ascii;
extern crate spin;
extern crate dfqueue;
extern crate console_types;
#[macro_use] extern crate log;


use keycodes_ascii::{Keycode, KeyboardModifiers, KEY_RELEASED_OFFSET, KeyAction, KeyEvent};
use spin::Once;
use dfqueue::DFQueueProducer;
use console_types::ConsoleEvent;


// TODO: avoid unsafe static mut using the following: https://www.reddit.com/r/rust/comments/1wvxcn/lazily_initialized_statics/cf61im5/
static mut KBD_MODIFIERS: KeyboardModifiers = KeyboardModifiers::default();


static CONSOLE_PRODUCER: Once<DFQueueProducer<ConsoleEvent>> = Once::new();


/// Initialize the keyboard driver. 
/// Arguments: a reference to a queue onto which keyboard events should be enqueued. 
pub fn init(console_queue_producer: DFQueueProducer<ConsoleEvent>) { 
    // assert_has_not_been_called!("keyboard init was called more than once!");
    
    // set keyboard to scancode set 1


    CONSOLE_PRODUCER.call_once(|| {
        console_queue_producer
    });
}



/// returns Ok(()) if everything was handled properly.
/// Otherwise, returns an error string.
pub fn handle_keyboard_input(scan_code: u8, _extended: bool) -> Result<(), &'static str> {
    // SAFE: no real race conditions with keyboard presses
    let modifiers = unsafe { &mut KBD_MODIFIERS };
   
    // debug!("KBD_MODIFIERS before {}: {:?}", scan_code, modifiers);

    // first, update the modifier keys
    match scan_code {
        x if x == Keycode::Control as u8 => { modifiers.control = true }
        x if x == Keycode::Alt     as u8 => { modifiers.alt = true }
        x if x == (Keycode::LeftShift as u8) || x == (Keycode::RightShift as u8) => { modifiers.shift = true }

        // toggle caps lock on press only
        x if x == Keycode::CapsLock as u8 => { modifiers.caps_lock ^= true }

        x if x == Keycode::Control as u8 + KEY_RELEASED_OFFSET => { modifiers.control = false }
        x if x == Keycode::Alt     as u8 + KEY_RELEASED_OFFSET => { modifiers.alt = false }
        x if x == ((Keycode::LeftShift as u8) + KEY_RELEASED_OFFSET) || x == ((Keycode::RightShift as u8) + KEY_RELEASED_OFFSET) => { modifiers.shift = false }

        _ => { } // do nothing
    }

    // debug!("KBD_MODIFIERS after {}: {:?}", scan_code, modifiers);

    // second,  put the keycode and it's action (pressed or released) in the keyboard queue
    match scan_code {
        x => { 
            let (adjusted_scan_code, action) = if x < KEY_RELEASED_OFFSET { 
                (scan_code, KeyAction::Pressed) 
            } else { 
                (scan_code - KEY_RELEASED_OFFSET, KeyAction::Released) 
            };

            let keycode = Keycode::from_scancode(adjusted_scan_code); 
            match keycode {
                Some(keycode) => { // this re-scopes (shadows) keycode
                    let event = ConsoleEvent::new_input_event(KeyEvent::new(keycode, action, modifiers.clone()));
                    if let Some(producer) = CONSOLE_PRODUCER.try() {
                        producer.enqueue(event);
                        Ok(()) // successfully queued up KeyEvent 
                    }
                    else {
                        warn!("handle_keyboard_input(): CONSOLE_PRODUCER wasn't yet initialized, dropping keyboard event {:?}.", event);
                        Err("keyboard event queue not ready")
                    }
                }

                _ => {
                    if scan_code == 0xe0 {
                        Ok(()) //ignore 0xe0 prefix
                    } else { 
                        warn!("handle_keyboard_input(): Unknown keycode: {:?}", keycode);
                        Err("unknown keyboard scancode") 
                    }
                }
            }
        }
    }

}
