#![no_std]
#![feature(alloc)]
#[macro_use] extern crate log;

extern crate mouse_data;
extern crate ps2;


use mouse_data::{ButtonAction, MouseMovement,MouseEvent};
use ps2::{init_ps2_port2,test_ps2_port2,set_mouse_id,check_mouse_id};

static mut MOUSE_MOVE: MouseMovement = MouseMovement::default();
static mut BUTTON_ACT: ButtonAction = ButtonAction::default();

/// Initialize the mouse driver.
pub fn init() {

    // init the second ps2 port for mouse
    init_ps2_port2();
    // test the second ps2 port
    test_ps2_port2();
    // set Mouse ID to 4
    let _e = set_mouse_id(4);
    // check the ID
    let id = check_mouse_id();
    info!("the initial mouse ID is: {}",id);

}

/// print the mouse actions
pub fn mouse_to_print(mouse_event:&MouseEvent) {
    let mouse_movement = &mouse_event.mousemove;
    let mouse_buttons = &mouse_event.buttonact;
    // print direction
    {
        if mouse_movement.right {
            if mouse_movement.up {
                info!("right,up,\n");
            } else if mouse_movement.down {
                info!("right,down,\n");
            } else {
                info!("right\n");
            }
        } else if mouse_movement.left {
            if mouse_movement.up {
                info!("left,up,\n");
            } else if mouse_movement.down {
                info!("left,down,\n");
            } else {
                info!("left\n");
            }
        } else if mouse_movement.up {
            info!("up,\n");
        } else if mouse_movement.down {
            info!("down,\n");
        } else if mouse_movement.scrolling_up {
            info!("scrollingup, \n");
        } else if mouse_movement.scrolling_down {
            info!("scrollingdown,\n");
        }
    }
    // print buttons
    {
        if mouse_buttons.left_button_hold{
            info!("left_button_hold");
        }

        if mouse_buttons.right_button_hold{
            info!("right_button_hold");
        }

        if mouse_buttons.fifth_button_hold{
            info!("right_button_hold");
        }

        if mouse_buttons.fourth_button_hold{
            info!("fourth_button_hold");
        }

    }
}

/// return a Mouse Event according to the data
pub fn handle_mouse_input(readdata: u32) -> MouseEvent{
    let action = unsafe{ &mut BUTTON_ACT};
    let mmove = unsafe{&mut MOUSE_MOVE};

    mmove.read_from_data(readdata);
    action.read_from_data(readdata);
    MouseEvent::new(*action, *mmove)

}






