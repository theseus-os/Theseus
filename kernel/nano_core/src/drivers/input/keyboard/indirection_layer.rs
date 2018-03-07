use keycodes_ascii::{Keycode, KeyboardModifiers, KEY_RELEASED_OFFSET, KeyAction, KeyEvent};
use spin::Once;
use dfqueue::DFQueueProducer;
use console::ConsoleEvent;


// TODO: avoid unsafe static mut using the following: https://www.reddit.com/r/rust/comments/1wvxcn/lazily_initialized_statics/cf61im5/
static mut KBD_MODIFIERS: KeyboardModifiers = KeyboardModifiers::default();


static CONSOLE_PRODUCER: Once<DFQueueProducer<ConsoleEvent>> = Once::new();


pub fn init(console_queue_producer: DFQueueProducer<ConsoleEvent>) { 
    assert_has_not_been_called!("keyboard init was called more than once!");
    
    CONSOLE_PRODUCER.call_once(|| {
        console_queue_producer
    });
}




#[derive(Debug)]
pub enum KeyboardInputError {
    UnknownScancode,
    EventQueueNotReady,
}




/// returns Ok(()) if everything was handled properly.
/// returns KeyboardInputError 
pub fn handle_keyboard_input(scan_code: u8) -> Result<(), KeyboardInputError> {
    // SAFE: no real race conditions with keyboard presses
    let modifiers = unsafe { &mut KBD_MODIFIERS };
   
    // debug!("KBD_MODIFIERS before {}: {:?}", scan_code, modifiers);

    // first, update the modifier keys
    match scan_code {
        x if x == Keycode::Control as u8 => { modifiers.control = true }
        x if x == Keycode::Alt     as u8 => { modifiers.alt = true }
        x if x == (Keycode::LeftShift as u8) || x == (Keycode::RightShift as u8) => { modifiers.shift = true }

        // trigger caps lock on press only
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
            let (adjusted_scan_code, action) = 
                if x < KEY_RELEASED_OFFSET { 
                    (scan_code, KeyAction::Pressed) 
                } else { 
                    (scan_code - KEY_RELEASED_OFFSET, KeyAction::Released) 
                };

            let keycode = Keycode::from_scancode(adjusted_scan_code); 
            match keycode {
                Some(keycode) => { // this re-scopes (shadows) keycode
                    if let Some(producer) = CONSOLE_PRODUCER.try() {
                        producer.enqueue(ConsoleEvent::new_input_event(KeyEvent::new(keycode, action, modifiers.clone())));
                        Ok(()) // successfully queued up KeyEvent 
                    }
                    else {
                        warn!("handle_keyboard_input(): CONSOLE_PRODUCER wasn't yet initialized, dropping keyboard event.");
                        Err(KeyboardInputError::EventQueueNotReady)
                    }
                }

                _ => { 
                    warn!("handle_keyboard_input(): Unknown keycode: {:?}", keycode);
                    Err(KeyboardInputError::UnknownScancode) 
                }
            }
        }
    }

}
