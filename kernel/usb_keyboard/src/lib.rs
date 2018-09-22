#![no_std]
#![feature(alloc)]

#![allow(dead_code)]

extern crate keycodes_ascii;
extern crate alloc;
extern crate owning_ref;
extern crate usb_uhci;
extern crate usb_desc;
extern crate usb_device;
extern crate memory;
extern crate spin;
extern crate volatile;
#[macro_use] extern crate log;

use keycodes_ascii::Keycode;
use alloc::boxed::Box;
use spin::{Once, Mutex};
use volatile:: ReadOnly;
use usb_desc::{UsbEndpDesc,UsbDeviceDesc,UsbConfDesc,UsbIntfDesc};
use usb_device::{UsbControlTransfer,UsbDevice,Controller,HIDType};
use usb_uhci::{box_dev_req,box_config_desc,box_device_desc,box_inter_desc,box_endpoint_desc,map,UHCI_STS_PORT,UHCI_CMD_PORT,get_registered_device};
use memory::{get_kernel_mmi_ref,MemoryManagementInfo,FRAME_ALLOCATOR,Frame,PageTable, ActivePageTable, PhysicalAddress, VirtualAddress, EntryFlags, MappedPages, allocate_pages ,allocate_frame};
use owning_ref:: BoxRefMut;

pub static USB_KEYBOARD_INPUT_BUFFER: Once<Mutex<BoxRefMut<MappedPages, [ReadOnly<u8>;8]>>> = Once::new();
static USB_KEYBOARD_INPUT_BUFFER_BASE: Once<u32> = Once::new();
pub static USB_KEYBOARD_TD_INDEX: Once<usize> = Once::new();
static USB_KEYBOARD_TD_ADD: Once<usize> = Once::new();
static USB_KEYBOARD_DEVICE_ID: Once<usize> = Once::new();

const TD_PACKET_IN : u8=                    0x69;
const TD_PACKET_OUT : u8=                   0xe1;

pub fn init(active_table: &mut ActivePageTable, index: usize)-> Result<(),&'static str>{

    let v_buffer_pointer = usb_uhci::buffer_pointer_alloc(0)
        .ok_or("Couldn't get virtual memory address for the buffer pointer in get_config_desc request for device in UHCI!!")?;
    let data_buffer_pointer = active_table.translate(v_buffer_pointer as usize)
        .ok_or("Couldn't translate the virtual memory address of the buffer pointer to phys_addr!!")?;

    let k_buffer = box_keyboard_buffer(active_table,data_buffer_pointer,0)?;

    USB_KEYBOARD_DEVICE_ID.call_once(||{
        index
    });


    USB_KEYBOARD_INPUT_BUFFER.call_once(||{
        Mutex::new(k_buffer)
    });

    USB_KEYBOARD_INPUT_BUFFER_BASE.call_once(||{
        data_buffer_pointer as u32
    });

    let (td_add,td_index) = usb_uhci::td_alloc().unwrap()?;

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
    let mut toggle = 0;


    let packet_add = td_add;
    let packet_index = td_index;
    info!("keyboard packet_add:{:x}", packet_add);
    usb_uhci::interrupt_td(packet_index,0,0,speed ,addr, endpoint as u32,toggle, TD_PACKET_IN as u32,
                      max_size,data_buffer_pointer);



    let frame_index = usb_uhci:: td_link_to_framelist(packet_add as u32).unwrap()?;


    Ok(())

}





/// Box the the keyboard input data buffer
pub fn box_keyboard_buffer(active_table: &mut ActivePageTable, frame_base: PhysicalAddress, offset: PhysicalAddress)
                      -> Result<BoxRefMut<MappedPages, [ReadOnly<u8>;8]>, &'static str>{


    let buffer: BoxRefMut<MappedPages, [ReadOnly<u8>;8]>  = BoxRefMut::new(Box::new(map(active_table,frame_base)?))
        .try_map_mut(|mp| mp.as_type_mut::<[ReadOnly<u8>;8]>(offset))?;


    Ok(buffer)
}

///Keyboard data handler

pub fn data_handler(){
    let a = USB_KEYBOARD_INPUT_BUFFER.try().map(|td_index| {

        for x in td_index.lock().iter(){

            let code = x.read();
            info!("the key :{:?}", Keycode::from_scancode_usb(code) );


        }

    });

}