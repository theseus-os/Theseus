//! A basic driver for a keyboard connected to the legacy PS/2 port.

#![no_std]
#![feature(abi_x86_interrupt)]

use core::sync::atomic::{AtomicBool, Ordering};
use keycodes_ascii::{Keycode, KeyboardModifiers, KEY_RELEASED_OFFSET, KeyAction, KeyEvent};
use log::{error, warn, debug};
use once_cell::unsync::Lazy;
use spin::Once;
use mpmc::Queue;
use event_types::Event;
use ps2::{PS2Keyboard, KeyboardType, LEDState, ScancodeSet};
use x86_64::structures::idt::InterruptStackFrame;

/// The first PS/2 port for the keyboard is connected directly to IRQ 1.
/// Because we perform the typical PIC remapping, the remapped IRQ vector number is 0x21.
const PS2_KEYBOARD_IRQ: u8 = interrupts::IRQ_BASE_OFFSET + 0x1;

// TODO: avoid unsafe static mut
static mut KBD_MODIFIERS: Lazy<KeyboardModifiers> = Lazy::new(KeyboardModifiers::new);

static KEYBOARD: Once<KeyboardInterruptParams> = Once::new();

struct KeyboardInterruptParams {
    keyboard: PS2Keyboard<'static>,
    queue: Queue<Event>,
}

/// Initialize the PS/2 keyboard driver and register its interrupt handler.
/// 
/// ## Arguments
/// * `keyboard`: a wrapper around keyboard functionality, used by the keyboard interrupt handler.
/// * `keyboard_queue_producer`: the queue onto which the keyboard interrupt handler
///    will push new keyboard events when a key action occurs.
pub fn init(keyboard: PS2Keyboard<'static>, keyboard_queue_producer: Queue<Event>) -> Result<(), &'static str> {
    // Detect which kind of keyboard is connected.
    // TODO: actually do something with the keyboard type.
    match keyboard.keyboard_detect() {
        Ok(KeyboardType::AncientATKeyboard) => debug!("The PS/2 keyboard type is: Ancient AT Keyboard with translator enabled in the PS/2 Controller"),
        Ok(KeyboardType::MF2Keyboard) => debug!("The PS/2 keyboard type is: MF2Keyboard"),
        Ok(KeyboardType::MF2KeyboardWithPSControllerTranslator) => debug!("The PS/2 keyboard type is: MF2 Keyboard with translator enabled in PS/2 Controller"),
        Err(e) => {
            error!("Failed to detect the PS/2 keyboard type: {e}");
            return Err("Failed to detect the PS/2 keyboard type");
        }
    }

    // TODO: figure out what we should do, for now using set 1
    keyboard.keyboard_scancode_set(ScancodeSet::Set1)?;

    // Register the interrupt handler
    interrupts::register_interrupt(PS2_KEYBOARD_IRQ, ps2_keyboard_handler).map_err(|e| {
        error!("PS/2 keyboard IRQ {PS2_KEYBOARD_IRQ:#X} was already in use by handler {e:#X}! Sharing IRQs is currently unsupported.");
        "PS/2 keyboard IRQ was already in use! Sharing IRQs is currently unsupported."
    })?;

    // Final step: set the producer end of the keyboard event queue.
    // Also add the keyboard struct for access during interrupts.
    KEYBOARD.call_once(|| KeyboardInterruptParams { keyboard, queue: keyboard_queue_producer });
    Ok(())
}

/// The interrupt handler for a PS/2-connected keyboard, registered at IRQ 0x21.
extern "x86-interrupt" fn ps2_keyboard_handler(_stack_frame: InterruptStackFrame) {
    // Some of the scancodes are "extended", which means they generate two different interrupts,
    // the first handling the E0 byte, the second handling their second byte.
    static EXTENDED_SCANCODE: AtomicBool = AtomicBool::new(false);

    if let Some(KeyboardInterruptParams { keyboard, queue }) = KEYBOARD.get() {
        let scan_code = keyboard.read_scancode();
        let extended = EXTENDED_SCANCODE.load(Ordering::SeqCst);

        // 0xE0 indicates an extended scancode, so we must wait for the next interrupt to get the actual scancode
        if scan_code == 0xE0 {
            if extended {
                error!("ps2_keyboard_handler: got two extended scancodes (0xE0) in a row! Shouldn't happen.");
            }
            // mark it true for the next interrupt
            EXTENDED_SCANCODE.store(true, Ordering::SeqCst);
        } else if scan_code == 0xE1 {
            error!("ps2_keyboard_handler: PAUSE/BREAK key pressed ... ignoring it!");
            // TODO: handle this, it's a 6-byte sequence (over the next 5 interrupts)
            EXTENDED_SCANCODE.store(true, Ordering::SeqCst);
        } else { // a regular scancode, go ahead and handle it
            // if the previous interrupt's scan_code was an extended scan_code, then this one is not
            if extended {
                EXTENDED_SCANCODE.store(false, Ordering::SeqCst);
            }
            // a scan code of zero is a PS2_PORT error that we can ignore,
            // a scan code of 0xFA is a command ACK response, already handled in polling (when sending a command, see ps2 crate)
            if scan_code != 0 && scan_code != 0xFA {
                if let Err(e) = handle_keyboard_input(keyboard, queue, scan_code, extended) {
                    error!("ps2_keyboard_handler: error handling PS2_PORT input: {e:?}");
                }
            }
        }
    } else {
        warn!("ps2_keyboard_handler(): KEYBOARD isn't initialized yet, skipping interrupt.");
    }
    
    interrupts::eoi(Some(PS2_KEYBOARD_IRQ));
}



/// Called from the keyboard interrupt handler when a keystroke is recognized.
/// 
/// Returns Ok(()) if everything was handled properly.
/// Otherwise, returns an error string.
fn handle_keyboard_input(keyboard: &PS2Keyboard, queue: &Queue<Event>, scan_code: u8, extended: bool) -> Result<(), &'static str> {
    // SAFE: no real race conditions with keyboard presses
    let modifiers = unsafe { &mut KBD_MODIFIERS };
    // debug!("KBD_MODIFIERS before {}: {:?}", scan_code, modifiers);

    // first, update the modifier keys
    match scan_code.try_into() {
        Ok(Keycode::Control) => {
            modifiers.insert(if extended {
                KeyboardModifiers::CONTROL_RIGHT
            } else {
                KeyboardModifiers::CONTROL_LEFT
            });
        }
        Ok(Keycode::Alt) => {
            modifiers.insert(KeyboardModifiers::ALT);
        }
        Ok(Keycode::LeftShift) => {
            modifiers.insert(KeyboardModifiers::SHIFT_LEFT);
        }
        Ok(Keycode::RightShift) => {
            modifiers.insert(KeyboardModifiers::SHIFT_RIGHT);
        }
        Ok(Keycode::SuperKeyLeft) => {
            modifiers.insert(KeyboardModifiers::SUPER_KEY_LEFT);
        }
        Ok(Keycode::SuperKeyRight) => {
            modifiers.insert(KeyboardModifiers::SUPER_KEY_RIGHT);
        }

        Ok(Keycode::ControlReleased) => {
            modifiers.remove(if extended {
                KeyboardModifiers::CONTROL_RIGHT
            } else {
                KeyboardModifiers::CONTROL_LEFT
            });
        }
        Ok(Keycode::AltReleased) => {
            modifiers.remove(KeyboardModifiers::ALT);
        }
        Ok(Keycode::LeftShiftReleased) => {
            modifiers.remove(KeyboardModifiers::SHIFT_LEFT);
        }
        Ok(Keycode::RightShiftReleased) => {
            modifiers.remove(KeyboardModifiers::SHIFT_RIGHT);
        }
        Ok(Keycode::SuperKeyLeftReleased) => {
            modifiers.remove(KeyboardModifiers::SUPER_KEY_LEFT);
        }
        Ok(Keycode::SuperKeyRightReleased) => {
            modifiers.remove(KeyboardModifiers::SUPER_KEY_RIGHT);
        }

        // The "*Lock" keys are toggled only upon being pressed, not when released.
        Ok(Keycode::CapsLock) => {
            modifiers.toggle(KeyboardModifiers::CAPS_LOCK);
            set_keyboard_led(keyboard, modifiers);
        }
        Ok(Keycode::ScrollLock) => {
            modifiers.toggle(KeyboardModifiers::SCROLL_LOCK);
            set_keyboard_led(keyboard, modifiers);
        }
        Ok(Keycode::NumLock) => {
            modifiers.toggle(KeyboardModifiers::NUM_LOCK);
            set_keyboard_led(keyboard, modifiers);
        }

        _ => {} // do nothing
    }

    // debug!("KBD_MODIFIERS after {}: {:?}", scan_code, modifiers);

    // second,  put the keycode and it's action (pressed or released) in the keyboard queue
    let (adjusted_scan_code, action) = if scan_code < KEY_RELEASED_OFFSET { 
        (scan_code, KeyAction::Pressed) 
    } else { 
        (scan_code - KEY_RELEASED_OFFSET, KeyAction::Released) 
    };

    if let Ok(keycode) = Keycode::try_from(adjusted_scan_code) {
        let event = Event::new_keyboard_event(KeyEvent::new(keycode, action, **modifiers));
        queue.push(event).map_err(|_| "keyboard input queue is full")
    } else {
        error!("handle_keyboard_input(): Unknown scancode: {scan_code:?}, adjusted scancode: {adjusted_scan_code:?}");
        Err("unknown keyboard scancode")
    }
}


fn set_keyboard_led(keyboard: &PS2Keyboard, modifiers: &KeyboardModifiers) {
    if let Err(e) = keyboard.set_keyboard_led(
        LEDState::new()
            .with_scroll_lock(modifiers.is_scroll_lock())
            .with_number_lock(modifiers.is_num_lock())
            .with_caps_lock(modifiers.is_caps_lock()),
    ) {
        error!("{e}");
    }
}
