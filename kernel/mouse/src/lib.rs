//! A basic driver for a mouse connected to the legacy PS/2 port.

#![no_std]
#![feature(abi_x86_interrupt)]

use log::{error, warn, debug};
use spin::Once;
use mpmc::Queue;
use event_types::Event;
use x86_64::structures::idt::InterruptStackFrame;
use mouse_data::{MouseButtons, MouseEvent, MouseMovementRelative};
use ps2::{mouse_id, set_mouse_id, read_mouse_packet, MouseId, MousePacket, status_register};

/// The first PS/2 port for the mouse is connected directly to IRQ 0xC.
/// Because we perform the typical PIC remapping, the remapped IRQ vector number is 0x2C.
const PS2_MOUSE_IRQ: u8 = interrupts::IRQ_BASE_OFFSET + 0xC;

static MOUSE_PRODUCER: Once<Queue<Event>> = Once::new();
static MOUSE_ID: Once<MouseId> = Once::new();

/// Initialize the PS/2 mouse driver and register its interrupt handler.
/// 
/// ## Arguments
/// * `mouse_queue_producer`: the queue onto which the mouse interrupt handler
///    will push new mouse events when a mouse action occurs.
pub fn init(mouse_queue_producer: Queue<Event>) -> Result<(), &'static str> {
    //TODO: set to 3, failed? id = 0, otherwise set to 4, failed? id = 3, otherwise id = 4
    //the current code beneath just tries to set id = 4, so is not final
    // Set Mouse ID to 4.
    if let Err(e) = set_mouse_id(MouseId::Four) {
        error!("Failed to set the mouse id to four: {e}");
        return Err("Failed to set the mouse id to four");
    }
    // Read it back to check that it worked.
    match mouse_id() {
        Ok(id) =>  {
            debug!("The PS/2 mouse ID is: {id:?}");
            if !matches!(id, MouseId::Four) {
                error!("Failed to set the mouse id to four");
                return Err("Failed to set the mouse id to four");
            }
        }
        Err(e) => {
            error!("Failed to read the PS/2 mouse ID: {e}");
            return Err("Failed to read the PS/2 mouse ID");
        }
    }
    MOUSE_ID.call_once(|| MouseId::Four);

    // Register the interrupt handler
    interrupts::register_interrupt(PS2_MOUSE_IRQ, ps2_mouse_handler).map_err(|e| {
        error!("PS/2 mouse IRQ {PS2_MOUSE_IRQ:#X} was already in use by handler {e:#X}! Sharing IRQs is currently unsupported.");
        "PS/2 mouse IRQ was already in use! Sharing IRQs is currently unsupported."
    })?;

    // Final step: set the producer end of the mouse event queue.
    MOUSE_PRODUCER.call_once(|| mouse_queue_producer);
    Ok(())
}

/// The interrupt handler for a PS/2-connected mouse, registered at IRQ 0x2C.
///
/// When a mouse with id 4 is not scrolling, one interrupt without a mouse packet happens (mouse output buffer not full),
/// then one interrupt with a mouse packet.
/// When a mouse with id 4 is scrolling, one interrupt without a mouse packet happens (mouse output buffer not full),
/// then two interrupts with mouse packets (the first one containing only the generic_part, the second one containing the complete packet).
/// 
/// In some cases (e.g. on device init), [the PS/2 controller can also send an interrupt](https://wiki.osdev.org/%228042%22_PS/2_Controller#Interrupts).
extern "x86-interrupt" fn ps2_mouse_handler(_stack_frame: InterruptStackFrame) {
    if let Some(id) = MOUSE_ID.get() {
        if status_register().mouse_output_buffer_full() {
            // NOTE: having read some more forum comments now, if this ever breaks on real hardware,
            // try to redesign this to only get one byte per interrupt instead of the 3-4 bytes we
            // currently get in read_mouse_packet and merge them afterwards
            let mouse_packet = read_mouse_packet(id);
            // warn!("read_mouse_packet: {mouse_packet:?}");
            
            if mouse_packet.always_one() != 1 {
                // this could signal a hardware error or a mouse which doesn't conform to the rule
                warn!("Discarding mouse data packet since its third bit should always be 1.");
            } else if let Err(e) = handle_mouse_input(mouse_packet) {
                error!("ps2_mouse_handler: {e:?}");
            }
        }
    } else {
        warn!("MOUSE_ID wasn't yet initialized, dropping the mouse packet.");
    }

    interrupts::eoi(Some(PS2_MOUSE_IRQ));
}


/// enqueue a Mouse Event according to the data
fn handle_mouse_input(mouse_packet: MousePacket) -> Result<(), &'static str> {
    let buttons = Buttons::from(&mouse_packet).0;
    let movement = Movement::from(&mouse_packet).0;

    let mouse_event = MouseEvent::new(buttons, movement);
    let event = Event::MouseMovementEvent(mouse_event);

    if let Some(producer) = MOUSE_PRODUCER.get() {
        producer.push(event).map_err(|_| "failed to enqueue the mouse event")
    } else {
        warn!("MOUSE_PRODUCER wasn't yet initialized, dropping mouse event {event:?}.");
        Err("MOUSE_PRODUCER wasn't yet initialized")
    }
}

// both MouseMovementRelative and MousePacketBits4 are in different crates, so we need a newtype wrapper:
struct Movement(MouseMovementRelative);
impl From<&MousePacket> for Movement {
    fn from(mouse_packet: &MousePacket) -> Self {
        Self(MouseMovementRelative::new(
            mouse_packet.x_movement(),
            mouse_packet.y_movement(),
            mouse_packet.scroll_movement()
        ))
    }
}

// both MouseButtons and MousePacketBits4 are in different crates, so we need a newtype wrapper:
struct Buttons(MouseButtons);
impl From<&MousePacket> for Buttons {
    fn from(mouse_packet: &MousePacket) -> Self {
        Self(MouseButtons::new()
            .with_left(mouse_packet.button_left())
            .with_right(mouse_packet.button_right())
            .with_middle(mouse_packet.button_middle())
            .with_fourth(mouse_packet.button_4())
            .with_fifth(mouse_packet.button_5())
        )
    }
}
