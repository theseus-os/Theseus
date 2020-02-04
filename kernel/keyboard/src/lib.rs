#![no_std]

extern crate keycodes_ascii;
extern crate spin;
extern crate event_types;
extern crate ps2;
extern crate mpmc;
#[macro_use] extern crate log;


use keycodes_ascii::{Keycode, KeyboardModifiers, KEY_RELEASED_OFFSET, KeyAction, KeyEvent};
use spin::Once;
use mpmc::Queue;
use event_types::Event;
use ps2::{init_ps2_port1,test_ps2_port1,keyboard_led,keyboard_detect,KeyboardType};


// TODO: avoid unsafe static mut using the following: https://www.reddit.com/r/rust/comments/1wvxcn/lazily_initialized_statics/cf61im5/
static mut KBD_MODIFIERS: KeyboardModifiers = KeyboardModifiers::new();


static KEYBOARD_PRODUCER: Once<Queue<Event>> = Once::new();

/// Bitmask for the Scroll Lock keyboard LED
const SCROLL_LED: u8 = 0b001;
/// Bitmask for the Num Lock keyboard LED
const NUM_LED: u8 = 0b010;
/// Bitmask for the Caps Lock keyboard LED
const CAPS_LED: u8 = 0b100;

/// Initialize the keyboard driver. 
/// Arguments: a reference to a queue onto which keyboard events should be enqueued. 
pub fn init(keyboard_queue_producer: Queue<Event>) { 
    // set keyboard to scancode set 1

    //init the first ps2 port for keyboard
    init_ps2_port1();
    //test the first port
    test_ps2_port1();
    match keyboard_detect(){
        Err(e) => { 
            error!("failed to read keyboard type due to: {} ", e)
        },
        Ok(s) => {
            match s {
                KeyboardType::AncientATKeyboard => info!("Ancient AT Keyboard with translator enabled in the PS/2 Controller"),
                KeyboardType::MF2Keyboard => info!("MF2Keyboard"),
                KeyboardType::MF2KeyboardWithPSControllerTranslator => info!("MF2 Keyboard with translator enabled in PS/2 Controller"),
            }
        }
    }
    KEYBOARD_PRODUCER.call_once(|| {
        keyboard_queue_producer
    });
}



/// returns Ok(()) if everything was handled properly.
/// Otherwise, returns an error string.
pub fn handle_keyboard_input(scan_code: u8, extended: bool) -> Result<(), &'static str> {
    // SAFE: no real race conditions with keyboard presses
    let modifiers = unsafe { &mut KBD_MODIFIERS };
    // debug!("KBD_MODIFIERS before {}: {:?}", scan_code, modifiers);

    // first, update the modifier keys
    match scan_code {
        x if x == Keycode::Control        as u8                       => { 
            modifiers.insert(if extended { KeyboardModifiers::CONTROL_RIGHT } else { KeyboardModifiers::CONTROL_LEFT});
        }
        x if x == Keycode::Alt            as u8                       => { modifiers.insert(KeyboardModifiers::ALT);              }
        x if x == Keycode::LeftShift      as u8                       => { modifiers.insert(KeyboardModifiers::SHIFT_LEFT);       }
        x if x == Keycode::RightShift     as u8                       => { modifiers.insert(KeyboardModifiers::SHIFT_RIGHT);      }
        x if x == Keycode::SuperKeyLeft   as u8                       => { modifiers.insert(KeyboardModifiers::SUPER_KEY_LEFT);   }
        x if x == Keycode::SuperKeyRight  as u8                       => { modifiers.insert(KeyboardModifiers::SUPER_KEY_RIGHT);  }

        x if x == Keycode::Control        as u8 + KEY_RELEASED_OFFSET => {
            modifiers.remove(if extended { KeyboardModifiers::CONTROL_RIGHT } else { KeyboardModifiers::CONTROL_LEFT});
        }
        x if x == Keycode::Alt            as u8 + KEY_RELEASED_OFFSET => { modifiers.remove(KeyboardModifiers::ALT);              }
        x if x == Keycode::LeftShift      as u8 + KEY_RELEASED_OFFSET => { modifiers.remove(KeyboardModifiers::SHIFT_LEFT);       }
        x if x == Keycode::RightShift     as u8 + KEY_RELEASED_OFFSET => { modifiers.remove(KeyboardModifiers::SHIFT_RIGHT);      }
        x if x == Keycode::SuperKeyLeft   as u8 + KEY_RELEASED_OFFSET => { modifiers.remove(KeyboardModifiers::SUPER_KEY_LEFT);   }
        x if x == Keycode::SuperKeyRight  as u8 + KEY_RELEASED_OFFSET => { modifiers.remove(KeyboardModifiers::SUPER_KEY_RIGHT);  }

        // The "*Lock" keys are toggled only upon being pressed, not when released.
        x if x == Keycode::CapsLock as u8 => {
            modifiers.toggle(KeyboardModifiers::CAPS_LOCK);
            set_keyboard_led(&modifiers);
        }
        x if x == Keycode::ScrollLock as u8 => {
            modifiers.toggle(KeyboardModifiers::SCROLL_LOCK);
            set_keyboard_led(&modifiers);
        }
        x if x == Keycode::NumLock as u8 => {
            modifiers.toggle(KeyboardModifiers::NUM_LOCK);
            set_keyboard_led(&modifiers);
        }

        _ => { } // do nothing
    }

//    debug!("KBD_MODIFIERS after {}: {:?}", scan_code, modifiers);

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
                Some(keycode) => {
                    let event = Event::new_keyboard_event(KeyEvent::new(keycode, action, modifiers.clone()));
                    if let Some(producer) = KEYBOARD_PRODUCER.try() {
                        producer.push(event).map_err(|_e| "keyboard input queue is full")
                    }
                    else {
                        warn!("handle_keyboard_input(): KEYBOARD_PRODUCER wasn't yet initialized, dropping keyboard event {:?}.", event);
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


fn set_keyboard_led(modifiers: &KeyboardModifiers) {
    let mut led_bitmask: u8 = 0; 
    if modifiers.is_caps_lock() {
        led_bitmask |= CAPS_LED;
    }
    if modifiers.is_num_lock() {
        led_bitmask |= NUM_LED;
    }
    if modifiers.is_scroll_lock() {
        led_bitmask |= SCROLL_LED;
    }

    keyboard_led(led_bitmask);
}