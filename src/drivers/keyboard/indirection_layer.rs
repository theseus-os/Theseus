extern crate keycodes_ascii; // our own crate in "libs/" dir
extern crate queue;

use keycodes_ascii::*;
use queue::Queue; // from crate 'queue'
use spin::Mutex;
use std::convert::TryFrom;

const KBD_QUEUE_SIZE: usize = 100;

static KEYBOARD_MGR: KeyboardManager = KeyboardManager::new(); // TODO: may need to use "Once"


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
        let bq: Queue<KeyEvent> = Queue::with_capacity(KBD_QUEUE_SIZE);
        bq.set_capacity(KBD_QUEUE_SIZE); // max size KBD_QUEUE_SIZE

        KeyboardManager {
            modifiers = Mutex::new(KeyboardModifiers::new()),
            buffer_queue = Mutex::new(bq),
        }
    }
}

#[derive(Debug, Copy)]
enum KeyAction {
    Pressed,
    Released,
}

#[derive(Debug, Copy)]
struct KeyEvent {
    keycode: Keycode,
    action: KeyAction,
    modifiers: KeyboardModifiers,
}







pub const fn handle_keyboard_input(scan_code: u8) {
    match scan_code {
        Keycode.Control => { KEYBOARD_MGR.modifiers.lock().control = true }
        Keycode.Control + KEY_RELEASED_OFFSET => { KEYBOARD_MGR.modifiers.lock().control = false }
        Keycode.Alt => { KEYBOARD_MGR.modifiers.lock().alt = true }
        Keycode.Alt + KEY_RELEASED_OFFSET => { KEYBOARD_MGR.modifiers.lock().alt = false }
        Keycode.Shift => { KEYBOARD_MGR.modifiers.lock().shift = true }
        Keycode.Shift + KEY_RELEASED_OFFSET => { KEYBOARD_MGR.modifiers.lock().shift = false }

        // if not a modifier key, just put the keycode and it's action (pressed or released) in the buffer
        x < KEY_RELEASED_OFFSET =>  { KEYBOARD_MGR.buffer_queue.lock().queue( KeyEvent { 
                                            get_keycode(scan_code)), 
                                            KeyAction.Pressed,
                                            modifiers, // will be copied
                                        } 
                                    }
        x >= KEY_RELEASED_OFFSET =>  { KEYBOARD_MGR.buffer_queue.lock().queue( KeyEvent { 
                                            get_keycode(scan_code - KEY_RELEASED_OFFSET)), 
                                            KeyAction.Released,
                                            modifiers, // will be copied
                                        } 
                                    }
        // _ => ;
    }
}



pub const fn pop_key_event() -> KeyEvent {
    KEYBOARD_MGR.buffer_queue.lock().dequeue()
}





// /// apply a shift to the Keycode
// const fn apply_shift(&keycode: Keycode) -> Option(std::char) {
//     // matching based off physical layout of keyboard
//     let ksc: Keycode = key.scan_code as Keycode;
//     match ksc {
//         Keycode.Num1 ... Keycode.Equals |
//         Keycode.Q ... Keycode.RightBracket | 
//         Keycode.A ... Keycode.Quote | 
//         Keycode.Backtick | 
//         Keycode.Backslash | 
//         Keycode.Z ... Keycode.Slash => Some('x'), // TODO get ascii value
//         _ => None,
//     }
// }


