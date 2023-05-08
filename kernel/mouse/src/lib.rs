//! A basic driver for a mouse connected to the legacy PS/2 port.

#![no_std]
#![feature(abi_x86_interrupt)]

use log::{error, warn};
use spin::{Mutex, Once};
use mpmc::Queue;
use event_types::Event;
use x86_64::structures::idt::InterruptStackFrame;
use mouse_data::{MouseButtons, MouseEvent, MouseMovementRelative};
use ps2::{PS2Mouse, MousePacket};

/// The first PS/2 port for the mouse is connected directly to IRQ 0xC.
/// Because we perform the typical PIC remapping, the remapped IRQ vector number is 0x2C.
const PS2_MOUSE_IRQ: u8 = interrupts::IRQ_BASE_OFFSET + 0xC;

const PS2_MAX_MOUSE_BYTES: usize = 4;

static MOUSE: Once<MouseInterruptParams> = Once::new();

/// Everything we need in [`ps2_mouse_handler`].
struct MouseInterruptParams {
    mouse: PS2Mouse<'static>,
    queue: Queue<Event>,
    packet_bytes: PacketBytes,
}

/// Somewhat like an array/vec mixture to allow pushing single bytes
/// of a mouse packet per interrupt into an array.
/// This can handle MouseId 0 (3 bytes) and 3, 4 (4 bytes).
struct PacketBytes {
    len: usize,
    inner: Mutex<[u8; PS2_MAX_MOUSE_BYTES]>,
    // where to push the next element
    cursor: Mutex<usize>,
}

impl PacketBytes {
    const fn new(len: usize) -> Self {
        Self { len, inner: Mutex::new([0; PS2_MAX_MOUSE_BYTES]), cursor: Mutex::new(0) }
    }
    // TODO: we can use a u32 once we switch to the bilge crate
    fn push(&self, value: u8) {
        let mut cursor = self.cursor.lock();
        if *cursor < PS2_MAX_MOUSE_BYTES {
            self.inner.lock()[*cursor] = value;
            *cursor += 1;
        }
    }

    /// Return the packet bytes if they're filled
    fn filled_bytes(&self) -> Option<[u8; 4]> {
        let mut cursor = self.cursor.lock();

        if *cursor == self.len {
            *cursor = 0;
            Some(*self.inner.lock())
        } else {
            None
        }
    }
}

/// Initialize the PS/2 mouse driver and register its interrupt handler.
/// 
/// ## Arguments
/// * `mouse`: a wrapper around mouse functionality and id, used by the mouse interrupt handler.
/// * `mouse_queue_producer`: the queue onto which the mouse interrupt handler
///    will push new mouse events when a mouse action occurs.
pub fn init(mut mouse: PS2Mouse<'static>, mouse_queue_producer: Queue<Event>) -> Result<(), &'static str> {
    // Set MouseId to the highest possible one
    if let Err(e) = mouse.set_mouse_id() {
        error!("Failed to set the mouse id: {e}");
        return Err("Failed to set the mouse id");
    }

    // Register the interrupt handler
    interrupts::register_interrupt(PS2_MOUSE_IRQ, ps2_mouse_handler).map_err(|e| {
        error!("PS/2 mouse IRQ {PS2_MOUSE_IRQ:#X} was already in use by handler {e:#X}! Sharing IRQs is currently unsupported.");
        "PS/2 mouse IRQ was already in use! Sharing IRQs is currently unsupported."
    })?;

    // Initialize the mouse packet bytes, which will be filled by 3-4 interrupts,
    // depending on the MouseId
    let packet_bytes = PacketBytes::new(mouse.packet_size());

    // Final step: set the producer end of the mouse event queue.
    // Also add the mouse struct for access during interrupts.
    MOUSE.call_once(|| MouseInterruptParams { mouse, queue: mouse_queue_producer, packet_bytes });
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
    if let Some(MouseInterruptParams { mouse, queue, packet_bytes }) = MOUSE.get() {
        // using `while` here didn't interact well with the window manager and increases handler runtime
        if mouse.is_output_buffer_full() {
            packet_bytes.push(mouse.read_packet_byte());

            if let Some(bytes) = packet_bytes.filled_bytes() {
                let mouse_packet = mouse.packet_from_bytes(bytes);

                if mouse_packet.always_one() != 1 {
                    // this could signal a hardware error or a mouse which doesn't conform to the rule
                    warn!("ps2_mouse_handler(): Discarding mouse data packet since its third bit should always be 1.");
                } else if let Err(e) = handle_mouse_input(mouse_packet, queue) {
                    error!("ps2_mouse_handler(): {e:?}");
                }
            }
        }
    } else {
        warn!("ps2_mouse_handler(): MOUSE isn't initialized yet, skipping interrupt.");
    }

    interrupts::eoi(Some(PS2_MOUSE_IRQ));
}


/// enqueue a Mouse Event according to the data
fn handle_mouse_input(mouse_packet: MousePacket, queue: &Queue<Event>) -> Result<(), &'static str> {
    let buttons = Buttons::from(&mouse_packet).0;
    let movement = Movement::from(&mouse_packet).0;

    let mouse_event = MouseEvent::new(buttons, movement);
    let event = Event::MouseMovementEvent(mouse_event);

    queue.push(event).map_err(|_| "failed to enqueue the mouse event")
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
