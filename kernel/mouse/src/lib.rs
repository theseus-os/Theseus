#![no_std]
#![feature(alloc)]
#[macro_use] extern crate log;

extern crate port_io;
extern crate mouse_data;
extern crate spin;

use spin::Mutex;

use port_io::Port;
use mouse_data::{ButtonAction, MouseMovement,MouseEvent};
use spin::Once;




// TODO: avoid unsafe static mut using the following: https://www.reddit.com/r/rust/comments/1wvxcn/lazily_initialized_statics/cf61im5/
static mut MOUSE_MOVE: MouseMovement = MouseMovement::default();
static mut BUTTON_ACT: ButtonAction = ButtonAction::default();
static PS2_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(0x60));
static PS2_COMMAND_PORT: Mutex<Port<u8>> = Mutex::new(Port::new(0x64));


// write data to second PS2 port, since mouse uses the second one
// it is called write to mouse
pub fn write_data_to_mouse(value:u8)->u8{
    unsafe { PS2_COMMAND_PORT.lock().write(0xD4) };
    unsafe { PS2_PORT.lock().write(value) };
    PS2_PORT.lock().read()
}


fn set_sampling_rate(value:u8){
    write_data_to_mouse(0xF3);
    write_data_to_mouse(value);
}


// set the mouse ID to 4
fn set_mouse_id_4(){
    set_sampling_rate(200);
    set_sampling_rate(100);
    set_sampling_rate(80);
    set_sampling_rate(200);
    set_sampling_rate(200);
    set_sampling_rate(80);
}
/// Initialize the mouse driver.
pub fn init() {


    // set Mouse ID to 4
    set_mouse_id_4();
    // check the ID
    {
        unsafe { PS2_COMMAND_PORT.lock().write(0xD4) };

        unsafe { PS2_PORT.lock().write(0xF5) };
        loop {
            let read = PS2_PORT.lock().read();
            if read == 0xFA {
                unsafe { PS2_COMMAND_PORT.lock().write(0xD4) };
                unsafe { PS2_PORT.lock().write(0xF2) };
                loop {
                    let read1 = PS2_PORT.lock().read();
                    if read1 == 0xFA {
                        let firstbyte = PS2_PORT.lock().read();
                        info!("check the mouse ID \n ,mouse ID:{:x}", firstbyte);
                        break;
                    }
                }

                break;
            }
        }
    }


    write_data_to_mouse(0xF4);

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






