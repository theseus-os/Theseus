extern crate keycodes_ascii; // our own crate in "libs/" dir
// extern crate queue; // using my own no_std version of queue now
// #[macro_use]
// extern crate lazy_static;


use keycodes_ascii::{Keycode, KeyboardModifiers, KEY_RELEASED_OFFSET};
use spin::{Mutex, Once};
use drivers::keyboard::queue::Queue;  // why is this "self" in front?



static KBD_QUEUE_SIZE: usize = 256;

lazy_static! {
    static ref KBD_MODIFIERS: Mutex<KeyboardModifiers> = Mutex::new( KeyboardModifiers::new() );
    static ref KBD_QUEUE: Mutex<Queue<KeyEvent>> = { 
        let mut q = Queue::with_capacity(KBD_QUEUE_SIZE);
        q.set_capacity(KBD_QUEUE_SIZE);
        Mutex::new( q ) // return this to KBD_QUEUE
    };
    // static KBD_SCANCODE_QUEUE // if we want a separate queue to buffer the raw scancodes...
}



// impl KeyboardManager {
//     pub fn new() -> KeyboardManager {
//         let mut bq: Queue<KeyEvent> = Queue::with_capacity(KBD_QUEUE_SIZE);
//         bq.set_capacity(KBD_QUEUE_SIZE); // max size KBD_QUEUE_SIZE

//         println!("Created new KEYBOARD_MGR with buffer size {}", KBD_QUEUE_SIZE);

//         KeyboardManager {
//             modifiers: Mutex::new(KeyboardModifiers::new()),
//             buffer_queue: Mutex::new(bq),
//         }
//     }
// }

#[derive(Debug, Copy, Clone)]
pub enum KeyAction {
    Pressed,
    Released,
}

/// the KeyEvent that should be delivered to applications upon a keyboard action
#[derive(Debug, Copy, Clone)]
pub struct KeyEvent {
    pub keycode: Keycode,
    pub action: KeyAction,
    pub modifiers: KeyboardModifiers,
}

impl KeyEvent {
    pub fn new(keycode: Keycode, action: KeyAction, modifiers: KeyboardModifiers,) -> KeyEvent {
        KeyEvent {
            keycode, 
            action,
            modifiers,
        }
    }
}


#[derive(Debug)]
pub enum KeyboardInputError {
    QueueFull,
    UnknownScancode,
}


/// returns Ok(()) if everything was handled properly.
/// returns KeyboardInputError 
pub fn handle_keyboard_input(scan_code: u8) -> Result<(), KeyboardInputError> {
    match scan_code {
        x if x == Keycode::Control as u8 => { KBD_MODIFIERS.lock().control = true }
        x if x == Keycode::Alt     as u8 => { KBD_MODIFIERS.lock().alt = true }
        x if x == (Keycode::LeftShift as u8) || x == (Keycode::RightShift as u8) => { KBD_MODIFIERS.lock().shift = true }
        
        x if x == Keycode::Control as u8 + KEY_RELEASED_OFFSET => { KBD_MODIFIERS.lock().control = false }
        x if x == Keycode::Alt     as u8 + KEY_RELEASED_OFFSET => { KBD_MODIFIERS.lock().alt = false }
        x if x == ((Keycode::LeftShift as u8) + KEY_RELEASED_OFFSET) || x == ((Keycode::RightShift as u8) + KEY_RELEASED_OFFSET) => { KBD_MODIFIERS.lock().shift = false }

        // if not a modifier key, just put the keycode and it's action (pressed or released) in the buffer
        x => { 
            let (adjusted_scan_code, action) = 
                if x < KEY_RELEASED_OFFSET { 
                    (scan_code, KeyAction::Pressed) 
                } else { 
                    (scan_code - KEY_RELEASED_OFFSET, KeyAction::Released) 
                };

            
            let keycode = Keycode::from_scancode(adjusted_scan_code); 
            match keycode {
                Some(keycode) => { // this re-scopes keycode              
                    let result = KBD_QUEUE.lock().queue( 
                        KeyEvent::new(keycode, action, KBD_MODIFIERS.lock().clone())); 
                    match result {
                        Ok(n) => { 
                            // println!("kbd buffer front: {:?}", KEYBOARD_MGR.buffer_queue.lock().peek());
                            return Ok(()); 
                        } 
                        Err(_) => { 
                            println!("Error: keyboard queue is full, discarding {}!", scan_code);
                            return Err(KeyboardInputError::QueueFull);
                        }
                    }
                }

                _ => { return Err(KeyboardInputError::UnknownScancode); }
            }
        }
    }

    Ok(())
}



pub fn pop_key_event() -> Option<KeyEvent> {
    KBD_QUEUE.lock().dequeue()
}


