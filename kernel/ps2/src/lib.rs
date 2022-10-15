#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use log::{warn, info};
use port_io::Port;
use spin::Mutex;

static PS2_PORT: Mutex<Port<u8>> = Mutex::new(Port::new(0x60));
static PS2_COMMAND_PORT: Mutex<Port<u8>> = Mutex::new(Port::new(0x64));

/// clean the PS2 data port (0x60) output buffer
/// also return the vec of data previously in the buffer which may be useful
fn ps2_clean_buffer() -> Vec<u8> {
    let mut buffer_data = Vec::new();
    loop {
        // if no more data in the output buffer
        if ps2_status_register() & 0x01 == 0 {
            break;
        } else {
            buffer_data.push(ps2_read_data());
        }
    }
    buffer_data
}

/// write command to the command ps2 port (0x64)
fn ps2_write_command(value: u8) {
    unsafe {
        PS2_COMMAND_PORT.lock().write(value);
    }
}

/// read the ps2 status register
pub fn ps2_status_register() -> u8 {
    PS2_COMMAND_PORT.lock().read()
}

/// read dat from ps2 data port (0x60)
pub fn ps2_read_data() -> u8 {
    PS2_PORT.lock().read()
}

/// read the config of the ps2 port
fn ps2_read_config() -> u8 {
    ps2_write_command(0x20);
    ps2_read_data()
}

/// write the new config to the ps2 command port (0x64)
fn ps2_write_config(value: u8) {
    ps2_write_command(0x60);
    unsafe { PS2_PORT.lock().write(value) };
}

/// initialize the first ps2 data port
pub fn init_ps2_port1() {
    init_ps2_port(PS2Port::One);
}

/// initialize the second ps2 data port
pub fn init_ps2_port2() {
    init_ps2_port(PS2Port::Two);
}
enum PS2Port {
    One,
    Two,
}

// see https://wiki.osdev.org/%228042%22_PS/2_Controller#Initialising_the_PS.2F2_Controller
fn init_ps2_port(port: PS2Port) {
    // Step 3: Disable Devices
    ps2_write_command(0xA7);
    ps2_write_command(0xAD);

    // Step 4: Flush The Output Buffer
    ps2_clean_buffer();

    // Step 5: Set the Controller Configuration Byte
    let mut config = ps2_read_config();
    match port {
        PS2Port::One => {
            config = (config & 0xEF) | 0x01;
        }
        PS2Port::Two => {
            config = (config & 0xDF) | 0x02;
        }
    }
    ps2_write_config(config);

    // Step 9: Enable Devices
    ps2_write_command(0xAE);
    match port {
        PS2Port::Two => ps2_write_command(0xA8),
        PS2Port::One => (),
    };
}

/// test the first ps2 data port
pub fn test_ps2_port1() {
    test_ps2_port(PS2Port::One);
}

// test the second ps2 data port
pub fn test_ps2_port2() {
    test_ps2_port(PS2Port::Two);
}

// see https://wiki.osdev.org/%228042%22_PS/2_Controller#Initialising_the_PS.2F2_Controller
fn test_ps2_port(port: PS2Port) {
    // test the ps2 controller
    ps2_write_command(0xAA);
    let test_flag = ps2_read_data();
    if test_flag == 0xFC {
        warn!("ps2 controller test failed!!!")
    } else if test_flag == 0x55 {
        info!("ps2 controller test pass!!!")
    }

    // test the ps2 data port
    match port {
        PS2Port::One => ps2_write_command(0xAB),
        PS2Port::Two => ps2_write_command(0xA9),
    }
    let test_flag = ps2_read_data();
    match test_flag {
        0x00 => info!("ps2 port {} test pass!!!", port as u8 + 1),
        0x01 => warn!("ps2 port {} clock line stuck low", port as u8 + 1),
        0x02 => warn!("ps2 port {} clock line stuck high", port as u8 + 1),
        0x03 => warn!("ps2 port {} data line stuck low", port as u8 + 1),
        0x04 => warn!("ps2 port {} data line stuck high", port as u8 + 1),
        _ => {}
    }

    // enable PS2 interrupt and see the new config
    let config = ps2_read_config();
    let port_interrupt_enabled = match port {
        PS2Port::One => config & 0x01 == 0x01,
        PS2Port::Two => config & 0x02 == 0x02,
    };
    let clock_enabled = match port {
        PS2Port::One => config & 0x10 != 0x10,
        PS2Port::Two => config & 0x20 != 0x20,
    };

    if port_interrupt_enabled {
        info!("ps2 port {} interrupt enabled", port as u8 + 1)
    } else {
        warn!("ps2 port {} interrupt disabled", port as u8 + 1)
    }
    if clock_enabled {
        info!("ps2 port {} enabled", port as u8 + 1)
    } else {
        warn!("ps2 port {} disabled", port as u8 + 1)
    }
}

/// write data to the first ps2 data port and return the response
fn data_to_port1(value: u8) -> u8 {
    unsafe { PS2_PORT.lock().write(value) };
    ps2_read_data()
}

/// write data to the second ps2 data port and return the response
fn data_to_port2(value: u8) -> u8 {
    ps2_write_command(0xD4);
    data_to_port1(value)
}

/// write command to the keyboard and return the result
fn command_to_keyboard(value: u8) -> Result<(), &'static str> {
    let response = data_to_port1(value);

    match response {
        0xFA => {
            // keyboard acknowledges the command
            Ok(())
        }

        0xFE => {
            // keyboard doesn't acknowledge the command
            Err("Fail to send the command to the keyboard")
        }

        _ => {
            for _x in 0..14 {
                // wait for response
                let response = ps2_read_data();
                if response == 0xFA {
                    return Ok(());
                } else if response == 0xfe {
                    return Err("Please resend the command to the keyboard");
                }
            }
            Err("command is not acknowledged")
        }
    }
}

/// write command to the mouse and return the result
fn command_to_mouse(value: u8) -> Result<(), &'static str> {
    // if the mouse doesn't acknowledge the command return error
    let response = data_to_port2(value);
    if response != 0xFA {
        for _x in 0..14 {
            let response = ps2_read_data();

            if response == 0xFA {
                return Ok(());
            }
        }
        warn!("mouse command to second port is not accepted");
        return Err("mouse command is not responded!!!");
    }
    Ok(())
}

/// write data to the second ps2 output buffer
fn write_to_second_output_buffer(value: u8) {
    ps2_write_command(0xD3);
    unsafe { PS2_PORT.lock().write(value) };
}

/// read mouse data packet; will work for mouse with ID 3 or 4
pub fn handle_mouse_packet() -> u32 {
    u32::from_le_bytes([
        ps2_read_data(),
        ps2_read_data(),
        ps2_read_data(),
        ps2_read_data(),
    ])
}

/// set ps2 mouse's sampling rate
fn set_sampling_rate(value: u8) -> Result<(), &'static str> {
    // if command is not acknowledged
    if let Err(_e) = command_to_mouse(0xF3) {
        Err("set mouse sampling rate failled, please try again")
    } else {
        // if second byte command is not acknowledged
        if let Err(_e) = command_to_mouse(value) {
            Err("set mouse sampling rate failled, please try again")
        } else {
            Ok(())
        }
    }
}

/// set the mouse ID (3 or 4 ) by magic sequence
/// 3 means that the mouse has scroll
/// 4 means that the mouse has scroll, fourth and fifth buttons
pub fn set_mouse_id(id: u8) -> Result<(), &'static str> {
    // stop the mouse's streaming before trying to set the ID
    // if fail to stop the streaming, return error
    if let Err(_e) = mouse_packet_streaming(false) {
        warn!("fail to stop streaming, before trying to read mouse id");
        return Err("fail to set the mouse id!!!");
    } else {
        // set the id to 3
        if id == 3 {
            if let Err(_e) = set_sampling_rate(200) {
                warn!("fail to stop streaming, before trying to read mouse id");
                return Err("fail to set the mouse id!!!");
            }
            if let Err(_e) = set_sampling_rate(100) {
                warn!("fail to stop streaming, before trying to read mouse id");
                return Err("fail to set the mouse id!!!");
            }
            if let Err(_e) = set_sampling_rate(80) {
                warn!("fail to stop streaming, before trying to read mouse id");
                return Err("fail to set the mouse id!!!");
            }
        }
        // set the id to 4
        else if id == 4 {
            if let Err(_e) = set_sampling_rate(200) {
                warn!("fail to stop streaming, before trying to read mouse id");
                return Err("fail to set the mouse id!!!");
            }
            if let Err(_e) = set_sampling_rate(100) {
                warn!("fail to stop streaming, before trying to read mouse id");
                return Err("fail to set the mouse id!!!");
            }
            if let Err(_e) = set_sampling_rate(80) {
                warn!("fail to stop streaming, before trying to read mouse id");
                return Err("fail to set the mouse id!!!");
            }
            if let Err(_e) = set_sampling_rate(200) {
                warn!("fail to stop streaming, before trying to read mouse id");
                return Err("fail to set the mouse id!!!");
            }
            if let Err(_e) = set_sampling_rate(200) {
                warn!("fail to stop streaming, before trying to read mouse id");
                return Err("fail to set the mouse id!!!");
            }
            if let Err(_e) = set_sampling_rate(80) {
                warn!("fail to stop streaming, before trying to read mouse id");
                return Err("fail to set the mouse id!!!");
            }
        } else {
            return Err("invalid id to be set to a mouse, please re-enter the id number");
        }
    }
    if let Err(_e) = mouse_packet_streaming(true) {
        Err("set mouse id without re-starting the streaming, please open it manually")
    } else {
        Ok(())
    }
}

/// check the mouse's id
pub fn check_mouse_id() -> Result<u8, &'static str> {
    // stop the streaming before trying to read the mouse's id
    let result = mouse_packet_streaming(false);
    match result {
        Err(e) => {
            warn!("check_mouse_id(): please try read the mouse id later, error: {}", e);
            Err(e)
        }
        Ok(_buffer_data) => {
            let id_num: u8;
            // check whether the command is acknowledged
            if let Err(e) = command_to_mouse(0xF2) {
                warn!("check_mouse_id(): command not accepted, please try again.");
                return Err(e);
            } else {
                id_num = ps2_read_data();
            }
            // begin streaming again
            if let Err(e) = mouse_packet_streaming(true) {
                warn!("the streaming of mouse is disabled,please open it manually");
                return Err(e);
            }
            Ok(id_num)
        }
    }
}

/// reset the mouse
fn reset_mouse() -> Result<(), &'static str> {
    if let Err(_e) = command_to_mouse(0xFF) {
        Err("reset mouse failled please try again")
    } else {
        for _x in 0..14 {
            let more_bytes = ps2_read_data();
            if more_bytes == 0xAA {
                // command reset mouse succeeded
                return Ok(());
            }
        }
        warn!("disable streaming failed");
        Err("fail to command reset mouse fail!!!")
    }
}

/// resend the most recent packet again
fn mouse_resend() -> Result<(), &'static str> {
    if let Err(_e) = command_to_mouse(0xFE) {
        Err("mouse resend request failled, please request again")
    } else {
        // mouse resend request succeeded
        Ok(())
    }
}

/// enable or disable the packet streaming
/// also return the vec of data previously in the buffer which may be useful
fn mouse_packet_streaming(enable: bool) -> Result<Vec<u8>, &'static str> {
    if enable {
        if let Err(_e) = command_to_mouse(0xf4) {
            warn!("enable streaming failed");
            Err("enable mouse streaming failed")
        } else {
            Ok(Vec::new())
        }
    }
    else {
        let mut buffer_data = Vec::new();
        if data_to_port2(0xf5) != 0xfa {
            for x in 0..15 {
                if x == 14 {
                    warn!("disable mouse streaming failed");
                    return Err("disable mouse streaming failed");
                }
                let response = ps2_read_data();

                if response != 0xfa {
                    buffer_data.push(response);
                } else {
                    return Ok(buffer_data);
                }
            }
        }
        Ok(buffer_data)
    } 
}

/// set the resolution of the mouse
/// parameter :
/// 0x00: 1 count/mm; 0x01 2 count/mm;
/// 0x02 4 count/mm; 0x03 8 count/mm;
fn mouse_resolution(value: u8) -> Result<(), &'static str> {
    if let Err(_e) = command_to_mouse(0xE8) {
        warn!("command set mouse resolution is not accepted");
        Err("set mouse resolution failed!!!")
    } else if let Err(_e) = command_to_mouse(value) {
        warn!("the resolution value is not accepted");
        Err("set mouse resolution failed!!!")
    } else {
        Ok(())
    }
}

///set LED status of the keyboard
/// parameter :
/// 0: ScrollLock; 1: NumberLock; 2: CapsLock
pub fn keyboard_led(value: u8) {
    if let Err(_e) = command_to_keyboard(0xED) {
        warn!("failed to set the keyboard led");
    } else if let Err(_e) = command_to_keyboard(value) {
        warn!("failed to set the keyboard led");
    }
}

/// set the scancode set of the keyboard
/// 0: get the current set; 1: set 1
/// 2: set 2; 3: set 3
fn keyboard_scancode_set(value: u8) -> Result<(), &'static str> {
    if let Err(_e) = command_to_keyboard(0xF0) {
        return Err("failed to set the keyboard scancode set");
    } else if let Err(_e) = command_to_keyboard(value) {
        return Err("failed to set the keyboard scancode set");
    }
    Ok(())
}

pub enum KeyboardType {
    MF2Keyboard,
    MF2KeyboardWithPSControllerTranslator,
    AncientATKeyboard,
}

///detect the keyboard's type
pub fn keyboard_detect() -> Result<KeyboardType, &'static str> {
    command_to_keyboard(0xF2)?; 
    let reply = ps2_read_data();
    match reply {
        0xAB => Ok(KeyboardType::MF2KeyboardWithPSControllerTranslator),
        0x41 => Ok(KeyboardType::MF2KeyboardWithPSControllerTranslator),
        0xC1 => Ok(KeyboardType::MF2KeyboardWithPSControllerTranslator),
        0x83 => Ok(KeyboardType::MF2Keyboard),
        _ => Ok(KeyboardType::AncientATKeyboard),
    }
}
