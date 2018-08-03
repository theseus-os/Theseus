#![no_std]
#![feature(alloc)]

#![allow(dead_code)]
extern crate usb_device;
extern crate usb_desc;
extern crate usb_req;
extern crate usb_uhci;
extern crate usb_ehci;
extern crate memory;
extern crate alloc;
#[macro_use] extern crate log;

use usb_desc::{UsbEndpDesc,UsbDeviceDesc,UsbConfDesc,UsbIntfDesc};
use usb_req::{UsbDevReq};
use usb_device::{UsbControlTransfer,UsbDevice,Controller};
use usb_uhci::{UhciTDRegisters, UhciQH};
use memory::{get_kernel_mmi_ref,MemoryManagementInfo,FRAME_ALLOCATOR,Frame,PageTable, ActivePageTable, PhysicalAddress, VirtualAddress, EntryFlags, MappedPages, allocate_pages ,allocate_frame};
use core::mem::size_of_val;
use alloc::Vec;


const TD_PACKET_IN :u8=                    0x69;
const TD_PACKET_OUT :u8=                   0xe1;
const TD_PACKET_SETUP :u8=                 0x2d;


/// Initialize the USB 1.1 host controller
pub fn init(active_table: &mut ActivePageTable) -> Result<(), &'static str> {


    if let Err(e) = usb_ehci::init(active_table){
        return Err(e);
    }
    if let Err(e) = usb_uhci::init(){
        return Err(e);
    }




    if usb_uhci::if_enable_port1(){
        let device = usb_uhci::port1_device_init()?;
        let add_trans = set_device_address(&device,1);
        let pointer = &add_trans as *const UhciTDRegisters;
        let index = usb_uhci::frame_number() as PhysicalAddress;
        let base = usb_uhci::frame_list_base() as PhysicalAddress;
        let mut frame_pointer = usb_uhci::frame_pointer(active_table,index,base)?;
        frame_pointer.write(pointer as u32);
    }


    Ok(())
}

/// build a setup transaction in TD to assign a device address to the device
pub fn set_device_address(dev: &UsbDevice, add: u16) -> UhciTDRegisters{


    let dev_request = &UsbDevReq::new(0x00, usb_req::REQ_SET_ADDR, add, 0,0);

    let frame = allocate_frame();
    // read necessary information to build TDs
    let speed = dev.speed as u32;
    let addr = dev.addr;
    let max_size = dev.maxpacketsize;
    let req_type = dev_request.dev_req_type;
    let mut len = dev_request.len as u32;
    let request_size = size_of_val(dev_request) as u32;
    let mut toggle: u32 = 0;

    // get the data buffer physical pointer
    let data_virtual_add= dev_request as *const UsbDevReq;
    let data_buffer_point = translate_add(data_virtual_add as usize).unwrap();

    // build the set up transaction to set address within a TD
    let set_add_transaction = UhciTDRegisters::
    init(0,speed ,addr, 0,0, TD_PACKET_SETUP as u32, request_size,data_buffer_point);

    set_add_transaction
}

/// According to the device information and request to build a setup transaction in TD
pub fn uhci_dev_request(dev: &UsbDevice, dev_req_type: u8,dev_request: u8, value: u16,
                       index: u16, len: u16) -> UhciTDRegisters{
    let dev_request = &UsbDevReq::new(dev_req_type, dev_request, value, index, len);
    uhci_control_transfer(dev,dev_request)

}

/// According to the request and device information to build a Control Transfer
/// Return: Vec<UhciTDRegisters> (a Vec contains Control transfer's transactions)
pub fn uhci_control_transfer(dev: &UsbDevice, dev_request: &UsbDevReq)-> UhciTDRegisters{


    let frame = allocate_frame();
    // read necessary information to build TDs
    let speed = dev.speed as u32;
    let addr = dev.addr;
    let max_size = dev.maxpacketsize;
    let req_type = dev_request.dev_req_type;
    let mut len = dev_request.len as u32;
    let request_size = size_of_val(dev_request) as u32;
    let mut toggle: u32 = 0;
//    let mut td_list:Vec<&mut UhciTDRegisters> = Vec::new();

    // get the data buffer physical pointer
    let data_virtual_add= dev_request as *const UsbDevReq;
    let data_buffer_point = translate_add(data_virtual_add as usize).unwrap();

    // build the set up transaction within a TD
    let setup_transaction = UhciTDRegisters::
    init(0,speed ,addr, 0,0, TD_PACKET_SETUP as u32, request_size,data_buffer_point);

    setup_transaction
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

