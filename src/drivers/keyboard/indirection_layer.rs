extern crate keycodes_ascii; // our own crate in "libs/" dir
// extern crate queue; // using my own no_std version of queue now
// #[macro_use]
// extern crate lazy_static;


use keycodes_ascii::{Keycode, KeyboardModifiers, KEY_RELEASED_OFFSET};
use spin::{Mutex, Once};
use drivers::keyboard::queue::Queue;  // why is this "self" in front?



static KBD_QUEUE_SIZE: usize = 256;

lazy_static! {
    static ref KEYBOARD_MGR: KeyboardManager = KeyboardManager::new(); 
}

#[derive(Debug)]
/// should be a singleton. 
/// The modifiers and buffer_queue are each protected by their own Mutex,
/// such that one can be accessed without locking the other
struct KeyboardManager {
    modifiers: Mutex<KeyboardModifiers>,
    buffer_queue: Mutex<Queue<KeyEvent>>, 
    // pressed_keys: BTreeSet<Keycode>, // probably don't need to save all pressed keys
}

impl KeyboardManager {
    pub fn new() -> KeyboardManager {
        let mut bq: Queue<KeyEvent> = Queue::with_capacity(KBD_QUEUE_SIZE);
        bq.set_capacity(KBD_QUEUE_SIZE); // max size KBD_QUEUE_SIZE

        println!("Created new KEYBOARD_MGR with buffer size {}", KBD_QUEUE_SIZE);

        KeyboardManager {
            modifiers: Mutex::new(KeyboardModifiers::new()),
            buffer_queue: Mutex::new(bq),
        }
    }
}

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



pub enum KeyboardInputError {
    QueueFull,
    UnknownScancode,
}


/// returns Ok(()) if everything was handled properly.
/// returns KeyboardInputError 
pub fn handle_keyboard_input(scan_code: u8) -> Result<(), KeyboardInputError> {
    match scan_code {
        x if x == Keycode::Control as u8 => { KEYBOARD_MGR.modifiers.lock().control = true }
        x if x == Keycode::Alt     as u8 => { KEYBOARD_MGR.modifiers.lock().alt = true }
        x if x == (Keycode::LeftShift as u8) || x == (Keycode::RightShift as u8) => { KEYBOARD_MGR.modifiers.lock().shift = true }
        
        x if x == Keycode::Control as u8 + KEY_RELEASED_OFFSET => { KEYBOARD_MGR.modifiers.lock().control = false }
        x if x == Keycode::Alt     as u8 + KEY_RELEASED_OFFSET => { KEYBOARD_MGR.modifiers.lock().alt = false }
        x if x == ((Keycode::LeftShift as u8) + KEY_RELEASED_OFFSET) || x == ((Keycode::RightShift as u8) + KEY_RELEASED_OFFSET) => { KEYBOARD_MGR.modifiers.lock().shift = false }

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
                    let result = KEYBOARD_MGR.buffer_queue.lock().queue( 
                        KeyEvent::new(keycode, action, KEYBOARD_MGR.modifiers.lock().clone())); 
                    match result {
                        Ok(n) => { return Ok(()); } 
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
    KEYBOARD_MGR.buffer_queue.lock().dequeue()
}


