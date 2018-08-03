#![feature(alloc)]

#![allow(dead_code)]
extern crate usb_uhci;
extern crate usb_device;
extern crate usb_desc;
extern crate usb_req;
extern crate memory;

use usb_desc::{UsbEndpDesc,UsbDeviceDesc,UsbConfDesc,UsbIntfDesc};
use usb_req::{UsbDevReq};
use usb_device::{UsbTransfer,UsbDevice,Controller};
use usb_uhci::{UhciTDRegisters};
use memory::{get_kernel_mmi_ref,FRAME_ALLOCATOR, MemoryManagementInfo, PhysicalAddress, Frame, PageTable, EntryFlags, FrameAllocator, allocate_pages, MappedPages,FrameIter};
use std::mem::size_of_val;


const TD_PACKET_IN :u8=                    0x69;
const TD_PACKET_OUT :u8=                   0xe1;
const TD_PACKET_SETUP :u8=                 0x2d;


/// Set up the device request and control transfer that contains the request data
/// Use proper Controller to send or response to the control transfer
pub fn usb_dev_request(dev: UsbDevice, dev_req_type: u8,request: u8, value: u16,
                       index: u16, len: u16) {
    let dev_request = UsbDevReq::new(dev_req_type, request, value, index, len);
    let control_transfer = UsbTransfer::new(0, dev_request, len, false, false);
    match dev.controller {
        Controller::EHCI => {},

        Controller::UCHI => {},

        _ => {}
    }
}

/// According to the request and device information to build control transfer
/// within proper data structures
pub fn uhci_dev_control(dev: &UsbDevice, trans: &UsbTransfer){

    let speed = dev.speed as u32;
    let addr = dev.addr;
    let size = dev.maxpacketsize;
    let req_type = trans.request.dev_req_type;
    let len = trans.request.len as u32;
    let request_size = size_of_val(trans.request) as u32;


    let data_virtual_add: *const UsbDevReq = &trans.request;
    if let Some(data_buffer_point) = translate_add(data_virtual_add as usize){

        let mut control_transfer = UhciTDRegisters::
        init(0,speed ,addr,0,0,
             TD_PACKET_SETUP as u32, request_size,data_buffer_point);

        // if the length field is not zero need TDs to read or send the required data
        if len != 0{

            //check the direction of following data
            let mut direction: u8;
            if req_type & usb_req::RT_DEV_TO_HOST = usb_req::RT_DEV_TO_HOST{

                direction = TD_PACKET_IN;

            }else if req_type & usb_req::RT_HOST_TO_DEV =usb_req::RT_HOST_TO_DEV{

                direction = TD_PACKET_OUT;

            }

        }






    }












}

/// translate virtual address to physical address
pub fn translate_add(v_addr : usize) -> Option<usize> {

    // get a reference to the kernel's memory mapping information
    let kernel_mmi_ref = get_kernel_mmi_ref().expect("e1000: translate_v2p couldnt get ref to kernel mmi");
    let mut kernel_mmi_locked = kernel_mmi_ref.lock();
    // destructure the kernel's MMI so we can access its page table
    let MemoryManagementInfo {
        page_table: ref mut kernel_page_table,
        ..  // don't need to access other stuff in kernel_mmi
    } = *kernel_mmi_locked;
    match kernel_page_table {
        &mut PageTable::Active(ref mut active_table) => {

            //let phys = try!(active_table.translate(v_addr).ok_or("e1000:translatev2p couldnt translate v addr"));
            //return Ok(phys);
            return active_table.translate(v_addr);

        }
        _ => {
            //return Err("kernel page table wasn't an ActivePageTable!");
            return None;
        }

    }

}
