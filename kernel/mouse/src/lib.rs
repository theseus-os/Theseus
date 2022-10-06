//! A basic driver for a mouse connected to the legacy PS2 port.

#![no_std]
#![feature(abi_x86_interrupt)]

#[macro_use] extern crate log;

use spin::Once;
use mpmc::Queue;
use event_types::Event;
use x86_64::structures::idt::InterruptStackFrame;
use mouse_data::{ButtonAction, Displacement, MouseEvent, MouseMovement};
use ps2::{check_mouse_id, init_ps2_port2, set_mouse_id, test_ps2_port2};

/// The first PS2 port for the mouse is connected directly to IRQ 0xC.
/// Because we perform the typical PIC remapping, the remapped IRQ vector number is 0x2C.
const PS2_MOUSE_IRQ: u8 = interrupts::IRQ_BASE_OFFSET + 0xC;

static mut MOUSE_MOVE: MouseMovement = MouseMovement::default();
static mut BUTTON_ACT: ButtonAction = ButtonAction::default();
static mut DISPLACEMENT: Displacement = Displacement::default();

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
    // TODO: return an error if this test fails.
    test_ps2_port2();

    // Set Mouse ID to 4, and read it back to check that it worked.
    let _e = set_mouse_id(4);
    match check_mouse_id() {
        Ok(id) => info!("the initial mouse ID is: {}", id),
        Err(_e) => {
            error!("Failed to read the initial PS2 mouse ID, error: {:?}", _e);
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

/// Print details of the given mouse event.
/// 
/// TODO: this is silly, just impl `fmt::Debug` for `MouseEvent`
#[allow(dead_code)]
fn mouse_to_print(mouse_event: &MouseEvent) {
    let mouse_movement = &mouse_event.mousemove;
    let mouse_buttons = &mouse_event.buttonact;
    let mouse_displacement = &mouse_event.displacement;
    let x = mouse_displacement.x as i8;
    let y = mouse_displacement.y as i8;

    {
        // print direction
        if mouse_movement.right {
            if mouse_movement.up {
                info!("right: {},up: {},\n", x, y);
            } else if mouse_movement.down {
                info!("right: {},down: {},\n", x, y);
            } else {
                info!("right: {}\n", x);
            }
        } else if mouse_movement.left {
            if mouse_movement.up {
                info!("left: {},up : {},\n", x, y);
            } else if mouse_movement.down {
                info!("left: {},down: {},\n", x, y);
            } else {
                info!("left: {}\n", x);
            }
        } else if mouse_movement.up {
            info!("up: {},\n", y);
        } else if mouse_movement.down {
            info!("down: {},\n", y);
        } else if mouse_movement.scrolling_up {
            info!("scrollingup, \n");
        } else if mouse_movement.scrolling_down {
            info!("scrollingdown,\n");
        }
    }
    // print buttons
    {
        if mouse_buttons.left_button_hold {
            info!("left_button_hold");
        }

        if mouse_buttons.right_button_hold {
            info!("right_button_hold");
        }

        if mouse_buttons.fifth_button_hold {
            info!("right_button_hold");
        }

        if mouse_buttons.fourth_button_hold {
            info!("fourth_button_hold");
        }
    }
}



/// The interrupt handler for a ps2-connected mouse, registered at IRQ 0x2C.
extern "x86-interrupt" fn ps2_mouse_handler(_stack_frame: InterruptStackFrame) {

    let indicator = ps2::ps2_status_register();

    // whether there is any data on the port 0x60
    if indicator & 0x01 == 0x01 {
        //whether the data is coming from the mouse
        if indicator & 0x20 == 0x20 {
            let readdata = ps2::handle_mouse_packet();
            if (readdata & 0x80 == 0x80) || (readdata & 0x40 == 0x40) {
                error!("The overflow bits in the mouse data packet's first byte are set! Discarding the whole packet.");
            } else if readdata & 0x08 == 0 {
                error!("Third bit should in the mouse data packet's first byte should be always be 1. Discarding the whole packet since the bit is 0 now.");
            } else {
                let _mouse_event = handle_mouse_input(readdata);
                // mouse_to_print(&_mouse_event);
            }
        }
    }

    interrupts::eoi(Some(PS2_MOUSE_IRQ));
}


/// return a Mouse Event according to the data
fn handle_mouse_input(readdata: u32) -> Result<(), &'static str> {
    let action = unsafe { &mut BUTTON_ACT };
    let mmove = unsafe { &mut MOUSE_MOVE };
    let dis = unsafe { &mut DISPLACEMENT };

    mmove.read_from_data(readdata);
    action.read_from_data(readdata);
    dis.read_from_data(readdata);

    let mouse_event = MouseEvent::new(*action, *mmove, *dis);
    // mouse_to_print(&mouse_event);  // use this to debug
    let event = Event::MouseMovementEvent(mouse_event);

    if let Some(producer) = MOUSE_PRODUCER.get() {
        producer.push(event).map_err(|_e| "Fail to enqueue the mouse event")
    } else {
        warn!("handle_mouse_input(): MOUSE_PRODUCER wasn't yet initialized, dropping mouse event {:?}.", event);
        Err("mouse event queue not ready")
    }
}
