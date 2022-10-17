//! A basic driver for a mouse connected to the legacy PS2 port.

#![no_std]
#![feature(abi_x86_interrupt)]

use log::{info, error, warn};
use spin::Once;
use mpmc::Queue;
use event_types::Event;
use x86_64::structures::idt::InterruptStackFrame;
use mouse_data::{ButtonAction, MouseEvent, MouseMovementRelative};
use ps2::{mouse_id, init_ps2_port2, set_mouse_id, test_ps2_port2, read_mouse_packet, MousePacketBits4, MouseId};

/// The first PS2 port for the mouse is connected directly to IRQ 0xC.
/// Because we perform the typical PIC remapping, the remapped IRQ vector number is 0x2C.
const PS2_MOUSE_IRQ: u8 = interrupts::IRQ_BASE_OFFSET + 0xC;

static MOUSE_PRODUCER: Once<Queue<Event>> = Once::new();

/// Initialize the PS2 mouse driver and register its interrupt handler.
/// 
/// ## Arguments
/// * `mouse_queue_producer`: the queue onto which the mouse interrupt handler
///    will push new mouse events when a mouse action occurs.
pub fn init(mouse_queue_producer: Queue<Event>) -> Result<(), &'static str> {
    // Init the second ps2 port, which is used for the mouse.
    init_ps2_port2();
    // Test the second port.
    test_ps2_port2()?;

    // Set Mouse ID to 4, and read it back to check that it worked.
    if let Err(e) = set_mouse_id(MouseId::Four) {
        error!("{e}");
    }
    match mouse_id() {
        Ok(id) => info!("the initial mouse ID is: {}", id),
        Err(e) => {
            error!("Failed to read the initial PS2 mouse ID, error: {:?}", e);
            return Err("Failed to read the initial PS2 mouse ID");
        }
    }

    // Register the interrupt handler
    interrupts::register_interrupt(PS2_MOUSE_IRQ, ps2_mouse_handler).map_err(|e| {
        error!("PS2 mouse IRQ {:#X} was already in use by handler {:#X}! Sharing IRQs is currently unsupported.", 
            PS2_MOUSE_IRQ, e,
        );
        "PS2 mouse IRQ was already in use! Sharing IRQs is currently unsupported."
    })?;

    // Final step: set the producer end of the mouse event queue.
    MOUSE_PRODUCER.call_once(|| mouse_queue_producer);
    Ok(())
}

/// The interrupt handler for a ps2-connected mouse, registered at IRQ 0x2C.
extern "x86-interrupt" fn ps2_mouse_handler(_stack_frame: InterruptStackFrame) {
    let mouse_packet = read_mouse_packet();
    if mouse_packet.x_overflow() || mouse_packet.y_overflow() {
        error!("The overflow bits in the mouse data packet's first byte are set! Discarding the whole packet.");
    } else if mouse_packet.always_one() != 1 {
        // it's very likely that the PS/2 controller send us an [interrupt](https://wiki.osdev.org/%228042%22_PS/2_Controller#Interrupts)
        // error!("Third bit in the mouse data packet's first byte should always be 1. Discarding the whole packet since the bit is 0.");
    } else {
        let _mouse_event = handle_mouse_input(mouse_packet);
    }

    interrupts::eoi(Some(PS2_MOUSE_IRQ));
}


/// return a Mouse Event according to the data
fn handle_mouse_input(mouse_packet: MousePacketBits4) -> Result<(), &'static str> {
    let action = button_action_from(&mouse_packet);
    let mmove = mouse_movement_from(&mouse_packet);

    let mouse_event = MouseEvent::new(action, mmove);
    let event = Event::MouseMovementEvent(mouse_event);

    if let Some(producer) = MOUSE_PRODUCER.get() {
        producer.push(event).map_err(|_e| "Fail to enqueue the mouse event")
    } else {
        warn!("handle_mouse_input(): MOUSE_PRODUCER wasn't yet initialized, dropping mouse event {:?}.", event);
        Err("mouse event queue not ready")
    }
}


// NOTE: This crate depends on mouse_data and ps2, so I'm doing this here
fn mouse_movement_from(mouse_packet: &MousePacketBits4) -> MouseMovementRelative {
    MouseMovementRelative::new(
        mouse_packet.x_movement(),
        mouse_packet.y_movement(),
        mouse_packet.scroll_movement()
    )
}

// NOTE: This crate depends on mouse_data and ps2, so I'm doing this here
fn button_action_from(mouse_packet: &MousePacketBits4) -> ButtonAction {
    ButtonAction::new(
        mouse_packet.button_left(),
        mouse_packet.button_right(),
        mouse_packet.button_middle(),
        mouse_packet.button_4(),
        mouse_packet.button_5(),
    )
}
