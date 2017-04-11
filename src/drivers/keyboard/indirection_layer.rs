extern crate keycodes_ascii; // our own crate in "libs/" dir
// extern crate queue; // using my own no_std version of queue now
// #[macro_use]
// extern crate lazy_static;


use keycodes_ascii::{Keycode, KeyboardModifiers, KEY_RELEASED_OFFSET};
use spin::{Mutex, Once};
// use drivers::keyboard::queue::Queue;  // why is this "self" in front?
use core::cell::{Ref, RefMut, RefCell};
use collections::Vec;


static KBD_QUEUE_SIZE: usize = 256;


/// the singleton instance of KeyboardState, constructed in init() below
static KBD_STATE: Once<KeyboardState> = Once::new();


struct KeyboardState {
    queue: RefCell<Vec<KeyEvent>>,
    modifiers: RefCell<KeyboardModifiers>,
}


pub fn init() {
    KBD_STATE.call_once( || {
         KeyboardState {
            queue:  RefCell::from(Vec::with_capacity(KBD_QUEUE_SIZE)),  // return this to "queue"
            modifiers: RefCell::from(KeyboardModifiers::new())
        }
    });
}


// lazy_static! {
//     static ref KBD_MODIFIERS: Mutex<KeyboardModifiers> = Mutex::new( KeyboardModifiers::new() );
//     static ref KBD_QUEUE: Mutex<Queue<KeyEvent>> = { 
//         let mut q = Queue::with_capacity(KBD_QUEUE_SIZE);
//         q.set_capacity(KBD_QUEUE_SIZE);
//         Mutex::new( q ) // return this to KBD_QUEUE
//     };
//     // static KBD_SCANCODE_QUEUE // if we want a separate queue to buffer the raw scancodes...
// }



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
    TryAcquireFailed,
}


/// returns Ok(()) if everything was handled properly.
/// returns KeyboardInputError 
pub fn handle_keyboard_input(scan_code: u8) -> Result<(), KeyboardInputError> {
    let kbd_state = KBD_STATE.try();
    if kbd_state.is_none() {
        println!("Error: KBD_STATE.try() failed, discarding {}!", scan_code);
        return Err(KeyboardInputError::TryAcquireFailed);
    }
    let kbd_state: &KeyboardState = kbd_state.unwrap(); // safe, cuz we already checked for is_none()

    match scan_code {
        x if x == Keycode::Control as u8 => { kbd_state.modifiers.borrow_mut().control = true }
        x if x == Keycode::Alt     as u8 => { kbd_state.modifiers.borrow_mut().alt = true }
        x if x == (Keycode::LeftShift as u8) || x == (Keycode::RightShift as u8) => { kbd_state.modifiers.borrow_mut().shift = true }
        
        x if x == Keycode::Control as u8 + KEY_RELEASED_OFFSET => { kbd_state.modifiers.borrow_mut().control = false }
        x if x == Keycode::Alt     as u8 + KEY_RELEASED_OFFSET => { kbd_state.modifiers.borrow_mut().alt = false }
        x if x == ((Keycode::LeftShift as u8) + KEY_RELEASED_OFFSET) || x == ((Keycode::RightShift as u8) + KEY_RELEASED_OFFSET) => { kbd_state.modifiers.borrow_mut().shift = false }

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
                Some(keycode) => { // this re-scopes (shadows) keycode
                    if kbd_state.queue.borrow().len() < KBD_QUEUE_SIZE {
                        kbd_state.queue.borrow_mut().push(KeyEvent::new(keycode, action, kbd_state.modifiers.clone().into_inner())); 
                        return Ok(());  // successfully queued up KeyEvent 
                    }
                    else {
                        println!("Error: keyboard queue is full, discarding {}!", scan_code);
                        return Err(KeyboardInputError::QueueFull);
                    }
                }

                _ => { return Err(KeyboardInputError::UnknownScancode); }
            }
        }
    }

    Ok(())
}



pub fn pop_key_event() -> Option<KeyEvent> {
    // this approach avoids a mutex by basically saying it we cannot get a mutable reference to the kbd queue,
    // then just do nothing until we can. 
    if let Some(ks) = KBD_STATE.try() {
        let res = ks.queue.try_borrow_mut(); 
        match res {
            Ok(mut qref) => {
                let ref mut q = *qref;
                if q.len() > 0 {
                    Some(q.remove(0)) // pop the first item
                }
                else { 
                    None // queue vector is empty
                }
            }
            Err(e) => { 
                println!("couldn't borrow queue as mut: {}", e);
                None
            }
        }
    }
    else {
        None
    }
}