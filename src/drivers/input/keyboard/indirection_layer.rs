extern crate keycodes_ascii; // our own crate in "libs/" dir
// extern crate queue; // using my own no_std version of queue now
// #[macro_use]
// extern crate lazy_static;


use keycodes_ascii::{Keycode, KeyboardModifiers, KEY_RELEASED_OFFSET};
// use spin::{Mutex, Once, RwLock};
// use core::cell::{Ref, RefMut, RefCell};
use collections::VecDeque;



// TODO: avoid unsafe static mut using the following: https://www.reddit.com/r/rust/comments/1wvxcn/lazily_initialized_statics/cf61im5/




static KBD_QUEUE_SIZE: usize = 256;


static mut KBD_QUEUE: Option<VecDeque<KeyEvent>> = None;
static mut KBD_MODIFIERS: Option<KeyboardModifiers> = None; 

// impl KeyboardState {
//     pub fn new() -> KeyboardState {
//         println!("Created new KeyboardState with buffer size {}", KBD_QUEUE_SIZE);
        
//         KeyboardState {
//             queue:      Mutex::new(Vec::with_capacity(KBD_QUEUE_SIZE)),
//             modifiers:  Mutex::new(KeyboardModifiers::new()),
//         }
//     }
// }

// pub fn init(&mut kbd_state: &KeyboardState) {

pub fn init() { 
    assert_has_not_been_called!("keyboard init was called more than once!");
    
    unsafe {
        KBD_QUEUE = Some(VecDeque::with_capacity(KBD_QUEUE_SIZE));
        KBD_MODIFIERS = Some(KeyboardModifiers::new());
    }

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





#[derive(Debug, Copy, Clone, PartialEq)]
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
    // TryAcquireFailed,
}




/// returns Ok(()) if everything was handled properly.
/// returns KeyboardInputError 
pub fn handle_keyboard_input(scan_code: u8) -> Result<(), KeyboardInputError> {
    // let kbd_state = KBD_STATE.try_read();
    // if kbd_state.is_none() {
    //     println!("Error: KBD_STATE.try_read() failed, discarding {}!", scan_code);
    //     return Err(KeyboardInputError::TryAcquireFailed);
    // }
    // let kbd_state = kbd_state.unwrap(); // safe, cuz we already checked for is_none()
    let mut modifiers = unsafe { KBD_MODIFIERS.as_mut().expect("Error: KBD_MODIFIERS was uninitialized") };
   
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
                    let mut queue = unsafe{ KBD_QUEUE.as_mut().expect("KBD_QUEUE was uninitialized") };
                    if queue.len() < KBD_QUEUE_SIZE {
                        queue.push_back(KeyEvent::new(keycode, action, modifiers.clone())); 
                        return Ok(());  // successfully queued up KeyEvent 
                    }
                    else {
                        // println!("Error: keyboard queue is full, discarding {}!", scan_code);
                        return Err(KeyboardInputError::QueueFull);
                    }
                }

                _ => { return Err(KeyboardInputError::UnknownScancode); }
            }
        }
    }

}



pub fn pop_key_event() -> Option<KeyEvent> {
    let kq = unsafe { KBD_QUEUE.as_mut() };

    if let Some(queue) = kq {
        queue.pop_front()
    }
    else {
        None
    }
}