#![no_std]
#[macro_use]
extern crate log;

extern crate mpmc;
extern crate event_types;
extern crate mouse_data;
extern crate ps2;
extern crate spin;

use mpmc::Queue;
use event_types::Event;
use spin::Once;

use mouse_data::{ButtonAction, Displacement, MouseEvent, MouseMovement};
use ps2::{check_mouse_id, init_ps2_port2, set_mouse_id, test_ps2_port2};

static mut MOUSE_MOVE: MouseMovement = MouseMovement::default();
static mut BUTTON_ACT: ButtonAction = ButtonAction::default();
static mut DISPLACEMENT: Displacement = Displacement::default();

static MOUSE_PRODUCER: Once<Queue<Event>> = Once::new();

/// Initialize the mouse driver.
pub fn init(mouse_queue_producer: Queue<Event>) {
    // init the second ps2 port for mouse
    init_ps2_port2();
    // test the second ps2 port
    test_ps2_port2();

    // set Mouse ID to 4
    let _e = set_mouse_id(4);
    // check the ID
    let id = check_mouse_id();
    match id {
        Err(_e) => error!("fail to read the initial mouse ID"),

        Ok(id) => {
            info!("the initial mouse ID is: {}", id);
        }
    }

    MOUSE_PRODUCER.call_once(|| mouse_queue_producer);
}

/// print the mouse actions
pub fn mouse_to_print(mouse_event: &MouseEvent) {
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

/// return a Mouse Event according to the data
pub fn handle_mouse_input(readdata: u32) -> Result<(), &'static str> {
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
        warn!("handle_keyboard_input(): MOUSE_PRODUCER wasn't yet initialized, dropping keyboard event {:?}.", event);
        Err("keyboard event queue not ready")
    }
}
