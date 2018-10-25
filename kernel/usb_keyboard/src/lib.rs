#![no_std]
#![feature(alloc)]

#![allow(dead_code)]

extern crate keycodes_ascii;
extern crate alloc;
extern crate owning_ref;
extern crate usb_uhci;
extern crate usb_desc;

extern crate memory;
extern crate spin;
extern crate volatile;
#[macro_use] extern crate log;

use keycodes_ascii::Keycode;
use alloc::boxed::Box;
use spin::{Once, Mutex};
use volatile:: {ReadOnly,Volatile};
use usb_desc::{UsbEndpDesc,UsbDeviceDesc,UsbConfDesc,UsbIntfDesc};

use usb_uhci::{map,get_registered_device};
use memory::{ActivePageTable, PhysicalAddress, MappedPages};
use owning_ref:: BoxRefMut;
use alloc::vec::Vec;

pub static USB_KEYBOARD_INPUT_BUFFER: Once<Mutex<BoxRefMut<MappedPages, [Volatile<u8>;8]>>> = Once::new();
static FIRST_INPUT: Once<Mutex<BoxRefMut<MappedPages, [Volatile<u8>;8]>>>= Once::new();
static SECOND_INPUT: Once<Mutex<BoxRefMut<MappedPages, [Volatile<u8>;8]>>> = Once::new();
static USB_KEYBOARD_INPUT_BUFFER_BASE: Once<u32> = Once::new();
pub static USB_KEYBOARD_TD_INDEX: Once<usize> = Once::new();
static USB_KEYBOARD_TD_ADD: Once<usize> = Once::new();
static USB_KEYBOARD_DEVICE_ID: Once<usize> = Once::new();

const TD_PACKET_IN : u8=                    0x69;
const TD_PACKET_OUT : u8=                   0xe1;


/// Initialize the USB keyboard
pub fn init(active_table: &mut ActivePageTable, index: usize)-> Result<(),&'static str>{

    let v_buffer_pointer = usb_uhci::buffer_pointer_alloc(0)
        .ok_or("Couldn't get virtual memory address for the buffer pointer in get_config_desc request for device in UHCI!!")?;
    let data_buffer_pointer = active_table.translate(v_buffer_pointer as usize)
        .ok_or("Couldn't translate the virtual memory address of the buffer pointer to phys_addr!!")?;

    let k_buffer = box_keyboard_buffer(active_table,data_buffer_pointer,0)?;
    let k_buffer_1 = box_keyboard_buffer(active_table,data_buffer_pointer,18)?;
    let k_buffer_2 = box_keyboard_buffer(active_table,data_buffer_pointer,12)?;

    FIRST_INPUT.call_once(||{
        Mutex::new(k_buffer_1)
    });

    SECOND_INPUT.call_once(||{
        Mutex::new(k_buffer_2)
    });

    USB_KEYBOARD_INPUT_BUFFER.call_once(||{
        Mutex::new(k_buffer)
    });

    USB_KEYBOARD_DEVICE_ID.call_once(||{
        index
    });

    USB_KEYBOARD_INPUT_BUFFER_BASE.call_once(||{
        data_buffer_pointer as u32
    });

    let (td_add,td_index) = usb_uhci::td_alloc().ok_or("Cannot allocate a new Transfer Head")?;

    USB_KEYBOARD_TD_INDEX.call_once(||{
        td_index
    });


    let td_add = active_table.translate(td_add).unwrap();

    USB_KEYBOARD_TD_ADD.call_once(||{
        td_add
    });


    init_receive_data()?;




    Ok(())

}


/// Initialize the USB keyboard's data transactions
pub fn init_receive_data() -> Result<(),&'static str>{


    let data_buffer_pointer = USB_KEYBOARD_INPUT_BUFFER_BASE.try().map(|pointer| {

        let pointer = *pointer;
        pointer

    }).ok_or("cannot get the base address of keyboard input buffer")?;

    let td_index = USB_KEYBOARD_TD_INDEX.try().map(|td_index| {

        let td_index = *td_index;
        td_index

    }).ok_or("cannot get the td index for keyboard interrupt transaction")?;

    let td_add = USB_KEYBOARD_TD_ADD.try().map(|td_add| {

        let td_add = *td_add;
        td_add

    }).ok_or("cannot get the td address for keyboard interrupt transaction")?;

    let index = USB_KEYBOARD_DEVICE_ID.try().map(|id| {

        let id = *id;
        id

    }).ok_or("cannot get the usb registered device's index for keyboard interrupt transaction")?;


    let device = get_registered_device(index).ok_or("cannot get the registered usb device")?;
    let speed= device.speed;
    let addr = device.addr;
    let max_size = device.maxpacketsize;
    let endpoint = device.interrupt_endpoint;
    let toggle = 0;


    let packet_add =  td_add;
    let packet_index = td_index;
    usb_uhci::interrupt_td(packet_index,0,0,speed ,addr, endpoint as u32,toggle, TD_PACKET_IN as u32,
                      max_size,data_buffer_pointer);



    let _index = usb_uhci:: td_link_to_framelist(packet_add as u32).ok_or("Cannot assign a frame to usb keyboard transfer.");


    Ok(())

}





/// Box the the keyboard input data buffer
pub fn box_keyboard_buffer(active_table: &mut ActivePageTable, frame_base: PhysicalAddress, offset: PhysicalAddress)
                      -> Result<BoxRefMut<MappedPages, [Volatile<u8>;8]>, &'static str>{


    let buffer: BoxRefMut<MappedPages, [Volatile<u8>;8]>  = BoxRefMut::new(Box::new(map(active_table,frame_base)?))
        .try_map_mut(|mp| mp.as_type_mut::<[Volatile<u8>;8]>(offset))?;


    Ok(buffer)
}

///Read the current input of the usb keyboard
/// Return an array contains the scancodes (last 6 bytes)
fn read_current_input() -> [u8;6]{

    let mut list = [0;6];
    USB_KEYBOARD_INPUT_BUFFER.try().map(|current_input| {

        for x in 2..8 {


            let code = current_input.lock()[x].read();
            list[x-2] = code;

        }


    });

    list

}

///Read and return the current modifier of the input
fn read_modifier() -> Result<u8,&'static str>{


    USB_KEYBOARD_INPUT_BUFFER.try().map(|current_input| {

        let code = current_input.lock()[0].read();
        code
    }).ok_or("cannot read the usb keyboard modifier")


}

///Read the previous input of the usb keyboard
/// Return an array contains the scancodes (last 6 bytes)
fn read_previous_input() -> [u8;6]{

    let mut list = [0;6];
    FIRST_INPUT.try().map(|previous_input| {

        for x in 0..6 {


            let code = previous_input.lock()[x].read();
            list[x] = code;

        }


    });

    list

}

///Read the oldest input of the usb keyboard
/// Return an array contains the scancodes (last 6 bytes)
fn read_oldest_input() -> [u8;6]{

    let mut list = [0;6];
    SECOND_INPUT.try().map(|oldest_input| {

        for x in 0..6 {


            let code = oldest_input.lock()[x].read();
            list[x] = code;

        }


    });

    list

}

///Update the previous input to be the current input
fn update_previous_input(list: [u8;6]){

    FIRST_INPUT.try().map(|previous_input| {

        for x in 0..6 {


            previous_input.lock()[x].write(list[x]);

        }


    });
}

///Clean the previous input
fn clean_previous_input(){

    SECOND_INPUT.try().map(|oldest_input| {

        for x in 0..6 {


            oldest_input.lock()[x].write(0);

        }


    });
}

///Update the oldest input to be the previous input
fn update_oldest_input(list: [u8;6]){

    SECOND_INPUT.try().map(|oldest_input| {

        for x in 0..6 {


            oldest_input.lock()[x].write(list[x]);

        }


    });
}

///Clean the previous input
fn clean_oldest_input(){

    SECOND_INPUT.try().map(|oldest_input| {

        for x in 0..6 {


            oldest_input.lock()[x].write(0);

        }


    });
}

///If no new scancode appears in the current input compare to the
///previous input, return false, else return true
fn check_input_1(current: [u8;6], previous: [u8;6]) -> Vec<u8>{

    let mut new_codes: Vec<u8> = Vec::new();
    for i in 0..6{
        let code = current[i];
        let mut flag = true;
        for j in 0..6{
            if code == previous[j]{
                flag = false;
            }
        }

        if flag{
            new_codes.push(code);
        }
    }

    new_codes
}

/// If three consecutive inputs are same, return true, else return false
fn check_input_2(list: [u8;6], list_1: [u8;6], list_2: [u8;6]) -> bool{

    let mut flag = false;
    if list.eq(&list_2) && list.eq(&list_1){
        flag = true
    }

    flag
}

/// Handle the input data from usb keyboard and print the key
pub fn data_handler() -> Result<(),&'static str>{

    let current_input = read_current_input();

    //if no data, print nothing
    if current_input[0] == 0{
        clean_oldest_input();
        clean_previous_input();
        return Ok(());
    }

    let modi = read_modifier()?;
    let previous_input = read_previous_input();
    let oldest_input = read_oldest_input();
    let new_codes = check_input_1(current_input,previous_input);


    //whether a single key is typed in consecutive times
    let mut only_one = true;
    for i in 1..6{
        if current_input[i] != 0 || previous_input[i] != 0{
            only_one = false;
            break;
        }
    }

    if only_one{
        if let Some(keycode) = Keycode::from_scancode_usb(current_input[0]) {
            if let Some(modifier) = Keycode::from_modifier_usb(modi) {
                info!("the modifier :{:?}", modifier);
            }

            info!("the key :{:?}", keycode);
//           info!("one");
            update_oldest_input(previous_input);
            update_previous_input(current_input);

                return Ok(());

        }
    }else if check_input_2(current_input, previous_input,oldest_input){
        //if the same input is sent three consecutive times, some keys are being typed
        //print them
        if let Some(modifier) = Keycode::from_modifier_usb(modi) {
            info!("the modifier :{:?}", modifier);
        }
        for i in 0..6{
            if let Some(keycode) = Keycode::from_scancode_usb(current_input[i]) {
                info!("the key :{:?}", keycode);
//                info!("duociyiyang");
            }
        }
    }else if new_codes.len() != 0{
        //if no new scan codes, some keys are typed too fast so that they are typed at same time
        //unconsciously. Only print them once
        if let Some(modifier) = Keycode::from_modifier_usb(modi) {
            info!("the modifier :{:?}", modifier);
        }
        for i in 0..new_codes.len(){
            if let Some(keycode) = Keycode::from_scancode_usb(new_codes[i]) {
                info!("the key :{:?}", keycode);
//                info!("faixanchongfu");
            }
        }

    }
    update_oldest_input(previous_input);
    update_previous_input(current_input);

    Ok(())




}