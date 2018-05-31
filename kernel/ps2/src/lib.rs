#![no_std]
#![feature(alloc)]

#[macro_use] extern crate log;
extern crate port_io;
extern crate spin;



use spin::Mutex;
use port_io::Port;


static PS2_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(0x60));
static PS2_COMMAND_PORT: Mutex<Port<u8>> = Mutex::new(Port::new(0x64));



/// clean the ps2 data port input buffer
pub fn ps2_clean_buffer() {
    loop {
        if ps2_status_register() & 0x01 == 0 {
            break;
        } else {
            ps2_read_data();
        }
    }
}

/// write command to the command ps2 port
pub fn ps2_write_command(value: u8) {
    unsafe { PS2_COMMAND_PORT.lock().write(value); }
}

/// read the ps2 status register
pub fn ps2_status_register()-> u8{
    PS2_COMMAND_PORT.lock().read()
}

/// read dat from ps2 data port
pub fn ps2_read_data()->u8{
    PS2_PORT.lock().read()
}
/// read the config of the ps2 port
pub fn ps2_read_config() -> u8 {
    ps2_write_command(0x20);
    let config = ps2_read_data();
    config
}

/// write the new config tothe ps2 port
pub fn ps2_write_config(value: u8) {
    ps2_write_command(0x60);
    data_to_port1(value);
}


///initialize the first ps2 data port
pub fn init_ps2_port1() {

    //disable PS2 ports first

    ps2_write_command(0xA7);
    ps2_write_command(0xAD);

    ps2_clean_buffer();
    // clean the output buffer of ps2

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

///initialize the second ps2 data port
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
    ps2_read_data()
}

/// write data to the second ps2 data port and return the response
pub fn data_to_port2(value: u8) -> u8 {
    ps2_write_command(0xD4);
    unsafe { PS2_PORT.lock().write(value) };
    ps2_read_data()
}


///handle mouse data packet
/// return 0 means failed
pub fn handle_mouse_packet() -> u32{


    let byte_1 = ps2_read_data() as u32;
    let byte_2 = ps2_read_data()  as u32;
    let byte_3 = ps2_read_data()  as u32;
    let byte_4 = ps2_read_data()  as u32;
    mouse_packet_streaming(1);
    ps2_clean_buffer();
    let mut readdata:u32 = 0;
    let id = check_mouse_id();
    info!("mouse ID{}",id);
    if id == 4 || id == 3 {
        readdata = (byte_4 << 24) | (byte_3 << 16) | (byte_2 << 8) | byte_1;
    }else if id == 0{
        readdata = (byte_3 << 16) | (byte_2 << 8) | byte_1;
        unsafe{
            let byte = byte_4 as u8;
            PS2_COMMAND_PORT.lock().write(0xD3);
            PS2_PORT.lock().write(byte);
        }

    }
    mouse_packet_streaming(0);
    readdata

}

/// set ps2 mouse's sampling rate
pub fn set_sampling_rate(value:u8){
    let response = data_to_port2(0xF3);
    if response == 0xFA {
        data_to_port2(value);
    }else {
        warn!("mouse command is not accepted!");

        }
}

/// set the mouse ID (3 or 4 ) by magic sequence
pub fn set_mouse_id(id:u8){
    let response = data_to_port2(0xF5);
    if response == 0xFA{
        if id == 3 {
            set_sampling_rate(200);
            set_sampling_rate(100);
            set_sampling_rate(80);
        }
            else if id == 4{
                set_sampling_rate(200);
                set_sampling_rate(100);
                set_sampling_rate(80);
                set_sampling_rate(200);
                set_sampling_rate(200);
                set_sampling_rate(80);
            }else{
                warn!("invalid id");
            }

        data_to_port2(0xF4);
    }else{
        warn!("mouse command is not accepted!");
    }

    
}

/// check mouse's id
pub fn check_mouse_id()->u8{
    mouse_packet_streaming(1);
    let id_num :u8;
    data_to_port2(0xF2);
    id_num = ps2_read_data();
    mouse_packet_streaming(0);
    id_num
}

/// reset the mouse
pub fn reset_mouse(){
    let response = data_to_port1(0xFF);
    if response != 0xFA{
        let mut count:u8 = 0;
        loop {
            let response = ps2_read_data();
            count += 1;
            if response == 0xFA{
                break;
            }
            if count == 15{
                warn!("disable streaming failed");
                break;
            }
        }

    }
    let mut count:u8 = 0;
    loop{
            if count == 20{
                warn!("mouse reset failed!");
                return;
            }
            let more_bytes = ps2_read_data();
            if more_bytes == 0xAA{
                info!("mouse is reset");
                break;
            }
            count += 1;
        }
}

///resend the most recent packet again
pub fn mouse_resend(){
    let response = data_to_port2(0xFE);
    if response != 0xFA{
        warn!("resend command is not accepted");
    }
}

///set the mouse to the default
pub fn mouse_default(){
    let response = data_to_port2(0xf6);
    if response != 0xFA{
        warn!("default command is not accepted");
    }
}

///enable or disable the packet streaming
/// 0 enable, 1 disable
pub fn mouse_packet_streaming(value:u8) {
    if value == 1 {
        let response = data_to_port2(0xf5);
        if response != 0xFA {
            let mut count:u8 = 0;
            loop {
                let response = ps2_read_data();
                count += 1;
                if response == 0xFA{
                    break;
                }
                if count == 15{
                    warn!("disable streaming failed");
                    break;
                }
            }

        }
    } else if value == 0 {
        let response = data_to_port2(0xf4);
        if response != 0xFA {
            let mut count:u8 = 0;
            loop {
                let response = ps2_read_data();
                count += 1;
                if response == 0xFA{
                    break;
                }
                if count == 15{
                    warn!("enable streaming failed");
                    break;
                }
            }

        }
    }else{
        warn!("invalid parameter");
    }
}

///set the resolution of the mouse
/// 0x00: 1 count/mm; 0x01 2 count/mm;
/// 0x02 4 count/mm; 0x03 8 count/mm;
pub fn mouse_resolution(value:u8){
    let response = data_to_port2(0xE8);
    if response == 0xFA {
        let ack = data_to_port2(value);
        if ack != 0xFA{
            warn!("failed to set the resolution");
        }
    }else {
        warn!("command is not accepted!");
    }
}

///set LED status of the keyboard
/// 0: ScrollLock; 1: NumberLock; 2: CapsLock
pub fn keyboard_led(value:u8){
    let response = data_to_port1(0xED);
    if response == 0xFA{
        let ack = data_to_port1(value);
        if ack != 0xFA{
            warn!("set LED failed")
        }
    }else if response == 0xFE{
        warn!("plz send the set led command again!");
    }else{
        warn!("command is not accepted")
    }
}

/// set the scancode set of the keyboard
/// 0: get the current set; 1: set 1
/// 2: set 2; 3: set 3
pub fn keyboard_scancode_set(value:u8){
    let response = data_to_port1(0xF0);
    if response == 0xFA{
        let ack = data_to_port1(value);
        if ack != 0xFA && ack != 0xFE{
            warn!("request failed")
        }
    } else if response == 0xFE{
        warn!("plz resend the command again!");
    }else{
        warn!("command is not accepted!");
    }

}

///detect the keyboard's type
pub fn keyboard_detect(){
    let response = data_to_port1(0xF2);
        if response == 0xFA{
            let reply = ps2_read_data();
            match reply {
                0xAB => info!("MF2 keyboard with translation enabled in the PS/Controller"),
                0x41 => info!("MF2 keyboard with translation enabled in the PS/Controller"),
                0xAB => info!("MF2 keyboard with translation enabled in the PS/Controller"),
                0xC1 => info!("MF2 keyboard with translation enabled in the PS/Controller"),
                0x83 => info!("MF2 keyboard"),
                _=> info!("Ancient AT keyboard with translation enabled in the PS/Controller (not possible for the second PS/2 port)")
            }
        }else {
            warn!("command is not accepted!")
        }

}