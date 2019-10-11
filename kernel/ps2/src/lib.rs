#![no_std]

#[macro_use]
extern crate log;
extern crate alloc;
extern crate port_io;
extern crate spin;

use alloc::vec::Vec;
use port_io::Port;
use spin::Mutex;

static PS2_PORT: Mutex<Port<u8>> = Mutex::new(Port::new(0x60));
static PS2_COMMAND_PORT: Mutex<Port<u8>> = Mutex::new(Port::new(0x64));

/// clean the PS2 data port (0x60) output buffer
/// also return the vec of data previously in the buffer which may be useful
pub fn ps2_clean_buffer() -> Vec<u8> {
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
pub fn ps2_write_command(value: u8) {
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
pub fn ps2_read_config() -> u8 {
    ps2_write_command(0x20);
    let config = ps2_read_data();
    config
}

/// write the new config to the ps2 command port (0x64)
pub fn ps2_write_config(value: u8) {
    ps2_write_command(0x60);
    unsafe { PS2_PORT.lock().write(value) };
}

/// initialize the first ps2 data port
pub fn init_ps2_port1() {
    //disable PS2 ports first
    ps2_write_command(0xA7);
    ps2_write_command(0xAD);

    // clean the output buffer of ps2 to avoid conflicts
    ps2_clean_buffer();

    // set the configuration
    {
        // read the old configuration
        let old_config = ps2_read_config();

        // set the new configuration
        let new_config = (old_config & 0xEF) | 0x01;
        ps2_write_config(new_config);
    }

    ps2_write_command(0xAE);
}

/// test the first ps2 data port
pub fn test_ps2_port1() {
    // test the ps2 controller
    {
        ps2_write_command(0xAA);
        let test_flag = ps2_read_data();
        if test_flag == 0xFC {
            warn!("ps2 controller test failed!!!")
        } else if test_flag == 0x55 {
            info!("ps2 controller test pass!!!")
        }
    }

    // test the first ps2 data port
    {
        ps2_write_command(0xAB);
        let test_flag = ps2_read_data();
        match test_flag {
            0x00 => info!("first ps2 port test pass!!!"),
            0x01 => warn!("first ps2 port clock line stuck low"),
            0x02 => warn!("first ps2 port clock line stuck high"),
            0x03 => warn!("first ps2 port data line stuck low"),
            0x04 => warn!("first ps2 port data line stuck high"),
            _ => {}
        }
    }

    // enable PS2 interrupt and see the new config
    {
        let config = ps2_read_config();
        if config & 0x01 == 0x01 {
            info!("first ps2 port interrupt enabled")
        } else {
            warn!("first ps2 port interrupt disabled")
        }
        if config & 0x10 != 0x10 {
            info!("first ps2 data port enabled")
        } else {
            warn!("first ps2 data port disabled")
        }
    }
}

/// initialize the second ps2 data port
pub fn init_ps2_port2() {
    //disable PS2 ports first
    {
        ps2_write_command(0xA7);
        ps2_write_command(0xAD);
    }

    // clean the output buffer of ps2
    ps2_clean_buffer();
    // set the configuration
    {
        // read the old configuration
        let old_config = ps2_read_config();

        // set the new configuration
        let new_config = (old_config & 0xDF) | 0x02;
        ps2_write_config(new_config);
    }
    {
        ps2_write_command(0xAE);
        ps2_write_command(0xA8);
    }
}

/// test the second ps2 data port
pub fn test_ps2_port2() {
    // test the ps2 controller
    {
        ps2_write_command(0xAA);
        let test_flag = ps2_read_data();
        if test_flag == 0xFC {
            warn!("ps2 controller test failed!!!")
        } else if test_flag == 0x55 {
            info!("ps2 controller test pass!!!")
        }
    }
    // test the second ps2 data port
    {
        ps2_write_command(0xA9);
        let test_flag = ps2_read_data();
        match test_flag {
            0x00 => info!("second ps2 port test pass!!!"),
            0x01 => warn!("second ps2 port clock line stuck low"),
            0x02 => warn!("second ps2 port clock line stuck high"),
            0x03 => warn!("second ps2 port data line stuck low"),
            0x04 => warn!("second ps2 port data line stuck high"),
            _ => {}
        }
    }
    // enable PS2 interrupt and see the new config
    {
        let config = ps2_read_config();
        if config & 0x02 == 0x02 {
            info!("second ps2 port interrupt enabled")
        } else {
            warn!("second ps2 port interrupt disabled")
        }
        if config & 0x20 != 0x20 {
            info!("second ps2 data port enabled")
        } else {
            warn!("second ps2 data port disabled")
        }
    }
}

/// write data to the first ps2 data port and return the response
pub fn data_to_port1(value: u8) -> u8 {
    unsafe { PS2_PORT.lock().write(value) };
    let response = ps2_read_data();
    response
}

/// write data to the second ps2 data port and return the response
pub fn data_to_port2(value: u8) -> u8 {
    ps2_write_command(0xD4);
    data_to_port1(value)
}

/// write command to the keyboard and return the result
pub fn command_to_keyboard(value: u8) -> Result<(), &'static str> {
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
pub fn command_to_mouse(value: u8) -> Result<(), &'static str> {
    // if the mouse doesn't acknowledge the command return error
    let response = data_to_port2(value);
    if response != 0xFA {
        for _x in 0..14 {
            let response = ps2_read_data();

            if response == 0xFA {
                //                info!("mouse command is responded !!!!");
                return Ok(());
            }
        }
        warn!("mouse command to second port is not accepted");
        return Err("mouse command is not responded!!!");
    }
    //    info!("mouse command is responded !!!!");
    Ok(())
}

/// write data to the second ps2 output buffer
pub fn write_to_second_output_buffer(value: u8) {
    ps2_write_command(0xD3);
    unsafe { PS2_PORT.lock().write(value) };
}

/// read mouse data packet
pub fn handle_mouse_packet() -> u32 {
    // since nowadays almost every mouse has scroll, so there is a 4 byte packet
    // which means that only mouse with ID 3 or 4 can be applied this function
    // because the mouse is initialized to have ID 3 or 4, so this function can
    // handle the mouse data packet

    let byte_1 = ps2_read_data() as u32;
    let byte_2 = ps2_read_data() as u32;
    let byte_3 = ps2_read_data() as u32;
    let byte_4 = ps2_read_data() as u32;
    let readdata = (byte_4 << 24) | (byte_3 << 16) | (byte_2 << 8) | byte_1;
    readdata
}

/// set ps2 mouse's sampling rate
pub fn set_sampling_rate(value: u8) -> Result<(), &'static str> {
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
        return Err("set mouse id without re-starting the streaming, please open it manually");
    } else {
        //        info!("set mouse id properly without error");
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
            return Err(e);
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
            return Ok(id_num);
        }
    }
}

/// reset the mouse
pub fn reset_mouse() -> Result<(), &'static str> {
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
pub fn mouse_resend() -> Result<(), &'static str> {
    if let Err(_e) = command_to_mouse(0xFE) {
        Err("mouse resend request failled, please request again")
    } else {
        // mouse resend request succeeded
        Ok(())
    }
}

/// enable or disable the packet streaming
/// also return the vec of data previously in the buffer which may be useful
pub fn mouse_packet_streaming(enable: bool) -> Result<Vec<u8>, &'static str> {
    if enable {
        if let Err(_e) = command_to_mouse(0xf4) {
            warn!("enable streaming failed");
            Err("enable mouse streaming failed")
        } else {
            // info!("enable streaming succeeded!!");
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
pub fn mouse_resolution(value: u8) -> Result<(), &'static str> {
    if let Err(_e) = command_to_mouse(0xE8) {
        warn!("command set mouse resolution is not accepted");
        Err("set mouse resolution failed!!!")
    } else {
        if let Err(_e) = command_to_mouse(value) {
            warn!("the resolution value is not accepted");
            Err("set mouse resolution failed!!!")
        } else {
            // info!("set mouse resolution succeeded!!!");
            Ok(())
        }
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
pub fn keyboard_scancode_set(value: u8) -> Result<(), &'static str> {
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
    //    let reply = data_to_port1(0xF2);
    //    info!("{:x}",reply);
    //    match reply {
    //        0xAB => Ok("MF2 keyboard with translation enabled in the PS/Controller"),
    //        0x41 => Ok("MF2 keyboard with translation enabled in the PS/Controller"),
    //        0xC1 => Ok("MF2 keyboard with translation enabled in the PS/Controller"),
    //        0x83 => Ok("MF2 keyboard"),
    //        _=> Err("Ancient AT keyboard with translation enabled in the PS/Controller (not possible for the second PS/2 port)")
    //    }
    if let Err(e) = command_to_keyboard(0xF2) {
        return Err(e);
    } else {
        let reply = ps2_read_data();
        match reply {
            0xAB => Ok(KeyboardType::MF2KeyboardWithPSControllerTranslator),
            0x41 => Ok(KeyboardType::MF2KeyboardWithPSControllerTranslator),
            0xC1 => Ok(KeyboardType::MF2KeyboardWithPSControllerTranslator),
            0x83 => Ok(KeyboardType::MF2Keyboard),
            _ => Ok(KeyboardType::AncientATKeyboard),
        }
    }
}
