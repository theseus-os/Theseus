//! A basic driver for a keyboard connected to the legacy PS2 port.

#![no_std]
#![feature(abi_x86_interrupt)]

#[macro_use] extern crate log;

use core::sync::atomic::{AtomicBool, Ordering};
use keycodes_ascii::{Keycode, KeyboardModifiers, KEY_RELEASED_OFFSET, KeyAction, KeyEvent};
use spin::Once;
use mpmc::Queue;
use event_types::Event;
use ps2::{init_ps2_port1, test_ps2_port1, keyboard_led, keyboard_detect, KeyboardType};
use x86_64::structures::idt::InterruptStackFrame;

/// The first PS2 port for the keyboard is connected directly to IRQ 1.
/// Because we perform the typical PIC remapping, the remapped IRQ vector number is 0x21.
const PS2_KEYBOARD_IRQ: u8 = interrupts::IRQ_BASE_OFFSET + 0x1;

// TODO: avoid unsafe static mut using the following: https://www.reddit.com/r/rust/comments/1wvxcn/lazily_initialized_statics/cf61im5/
static mut KBD_MODIFIERS: KeyboardModifiers = KeyboardModifiers::new();


static KEYBOARD_PRODUCER: Once<Queue<Event>> = Once::new();

/// Bitmask for the Scroll Lock keyboard LED
const SCROLL_LED: u8 = 0b001;
/// Bitmask for the Num Lock keyboard LED
const NUM_LED: u8 = 0b010;
/// Bitmask for the Caps Lock keyboard LED
const CAPS_LED: u8 = 0b100;

/// Initialize the PS2 keyboard driver and register its interrupt handler.
/// 
/// ## Arguments
/// * `keyboard_queue_producer`: the queue onto which the keyboard interrupt handler
///    will push new keyboard events when a key action occurs.
pub fn init(keyboard_queue_producer: Queue<Event>) -> Result<(), &'static str> {
    // Init the first ps2 port, which is used for the keyboard.
    init_ps2_port1();
    // Test the first port.
    // TODO: return an error if this test fails.
    test_ps2_port1();
    // Detect which kind of keyboard is connected.
    // TODO: actually do something with the keyboard type.
    match keyboard_detect() {
        Ok(KeyboardType::AncientATKeyboard) => info!("Ancient AT Keyboard with translator enabled in the PS/2 Controller"),
        Ok(KeyboardType::MF2Keyboard) => info!("MF2Keyboard"),
        Ok(KeyboardType::MF2KeyboardWithPSControllerTranslator) => info!("MF2 Keyboard with translator enabled in PS/2 Controller"),
        Err(e) => {
            error!("Failed to detect the Ps2 keyboard type, error: {} ", e);
            return Err("Failed to detect the PS2 keyboard type");
        }
    }

    // TODO: set keyboard to scancode set 1, since that's the only one we support (?)

    // Register the interrupt handler
    interrupts::register_interrupt(PS2_KEYBOARD_IRQ, ps2_keyboard_handler).map_err(|e| {
        error!("PS2 keyboard IRQ {:#X} was already in use by handler {:#X}! Sharing IRQs is currently unsupported.", 
            PS2_KEYBOARD_IRQ, e,
        );
        "PS2 keyboard IRQ was already in use! Sharing IRQs is currently unsupported."
    })?;

    // Final step: set the producer end of the keyboard event queue.
    KEYBOARD_PRODUCER.call_once(|| keyboard_queue_producer);
    Ok(())
}

/// The interrupt handler for a ps2-connected keyboard, registered at IRQ 0x21.
extern "x86-interrupt" fn ps2_keyboard_handler(_stack_frame: InterruptStackFrame) {
    // see this: https://forum.osdev.org/viewtopic.php?f=1&t=32655
    static EXTENDED_SCANCODE: AtomicBool = AtomicBool::new(false);

    let indicator = ps2::ps2_status_register();

    // whether there is any data on the port 0x60
    if indicator & 0x01 == 0x01 {
        // Skip this if the PS2 event came from the mouse, not the keyboard
        if indicator & 0x20 != 0x20 {
            // in this interrupt, we must read the PS2_PORT scancode register before acknowledging the interrupt.
            let scan_code = ps2::ps2_read_data();
            // trace!("PS2_PORT interrupt: raw scan_code {:#X}", scan_code);


            let extended = EXTENDED_SCANCODE.load(Ordering::SeqCst);

            // 0xE0 indicates an extended scancode, so we must wait for the next interrupt to get the actual scancode
            if scan_code == 0xE0 {
                if extended {
                    error!("PS2_PORT interrupt: got two extended scancodes (0xE0) in a row! Shouldn't happen.");
                }
                // mark it true for the next interrupt
                EXTENDED_SCANCODE.store(true, Ordering::SeqCst);
            } else if scan_code == 0xE1 {
                error!("PAUSE/BREAK key pressed ... ignoring it!");
                // TODO: handle this, it's a 6-byte sequence (over the next 5 interrupts)
                EXTENDED_SCANCODE.store(true, Ordering::SeqCst);
            } else { // a regular scancode, go ahead and handle it
                // if the previous interrupt's scan_code was an extended scan_code, then this one is not
                if extended {
                    EXTENDED_SCANCODE.store(false, Ordering::SeqCst);
                }
                if scan_code != 0 {  // a scan code of zero is a PS2_PORT error that we can ignore
                    if let Err(e) = handle_keyboard_input(scan_code, extended) {
                        error!("ps2_keyboard_handler: error handling PS2_PORT input: {:?}", e);
                    }
                }
            }
        }
    }
    
    interrupts::eoi(Some(PS2_KEYBOARD_IRQ));
}



/// Called from the keyboard interrupt handler when a keystroke is recognized.
/// 
/// Returns Ok(()) if everything was handled properly.
/// Otherwise, returns an error string.
fn handle_keyboard_input(scan_code: u8, extended: bool) -> Result<(), &'static str> {
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

    // debug!("KBD_MODIFIERS after {}: {:?}", scan_code, modifiers);

    // second,  put the keycode and it's action (pressed or released) in the keyboard queue
    let (adjusted_scan_code, action) = if scan_code < KEY_RELEASED_OFFSET { 
        (scan_code, KeyAction::Pressed) 
    } else { 
        (scan_code - KEY_RELEASED_OFFSET, KeyAction::Released) 
    };

    if let Some(keycode) = Keycode::from_scancode(adjusted_scan_code) {
        let event = Event::new_keyboard_event(KeyEvent::new(keycode, action, modifiers.clone()));
        if let Some(producer) = KEYBOARD_PRODUCER.get() {
            producer.push(event).map_err(|_e| "keyboard input queue is full")
        } else {
            warn!("handle_keyboard_input(): KEYBOARD_PRODUCER wasn't yet initialized, dropping keyboard event {:?}.", event);
            Err("keyboard event queue not ready")
        }
    } else {
        if scan_code == 0xE0 {
            Ok(()) //ignore 0xE0 prefix
        } else { 
            error!("handle_keyboard_input(): Unknown scancode: {:?}, adjusted scancode: {:?}",
                scan_code, adjusted_scan_code
            );
            Err("unknown keyboard scancode") 
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
