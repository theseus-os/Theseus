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
use usb_req::UsbDevReq;
use usb_device::{UsbControlTransfer,UsbDevice,Controller};
use usb_uhci::{box_dev_req,box_config_desc};
use memory::{get_kernel_mmi_ref,MemoryManagementInfo,FRAME_ALLOCATOR,Frame,PageTable, ActivePageTable, PhysicalAddress, VirtualAddress, EntryFlags, MappedPages, allocate_pages ,allocate_frame};
use core::mem::size_of_val;


const TD_PACKET_IN : u8=                    0x69;
const TD_PACKET_OUT : u8=                   0xe1;
const TD_PACKET_SETUP : u8=                 0x2d;
const NO_DATA: u32=                         0x7FF + 1;

/// Initialize the USB host controllers: UHCI & EHCI
pub fn init(active_table: &mut ActivePageTable) -> Result<(), &'static str> {


    if let Err(e) = usb_ehci::init(active_table){
        return Err(e);
    }
    if let Err(e) = usb_uhci::init(active_table){
        return Err(e);
    }


    if let Err(e) = device_init(active_table){
        return Err(e);
    }




    Ok(())
}

/// Initialize the device
/// Have not implemented error check yet, need to use the return of the set up request functions to return error
pub fn device_init(active_table: &mut ActivePageTable) -> Result<(),&'static str>{

    usb_uhci::port1_reset();
    let device = &mut usb_uhci::port1_device_init()?;
    let mut offset:usize = 0;

//    let dev_request_pointer_v = usb_uhci::buffer_pointer_alloc(offset)
//        .ok_or("Couldn't get virtual memory address for the set_address request for device in UHCI!!")?;
//    let dev_request_pointer = active_table.translate(dev_request_pointer_v as usize)
//        .ok_or("Couldn't translate the virtual memory address of the set_address request to phys_addr!!")?;
//    let data_buffer_base = dev_request_pointer;
//    let mut address_request = box_dev_req(active_table,data_buffer_base,0)?;
//    offset += 8;
//    address_request.init(0x00, usb_req::REQ_SET_ADDR, 1, 0,0);
    let (data_buffer_base,new_offset) = build_request(active_table,0x00, usb_req::REQ_SET_ADDR, 1, 0,0,offset)?;
    offset = new_offset;

    let set_add_frame_index = set_device_address(device, data_buffer_base as u32,1,active_table)?;

    let dev_request_pointer_v = usb_uhci::buffer_pointer_alloc(offset)
        .ok_or("Couldn't get virtual memory address for the get_config_desc request for device in UHCI!!")?;
    let dev_request_pointer = active_table.translate(dev_request_pointer_v as usize)
        .ok_or("Couldn't translate the virtual memory address of the get_config_desc request to phys_addr!!")?;
    let mut config_value_request = box_dev_req(active_table,data_buffer_base,offset)?;
    config_value_request.init(0x80, usb_req::REQ_GET_DESC,
                              usb_desc::USB_DESC_CONF, 0,8);
    offset += 8;

    let v_buffer_pointer = usb_uhci::buffer_pointer_alloc(offset)
        .ok_or("Couldn't get virtual memory address for the buffer pointer in get_config_desc request for device in UHCI!!")?;
    let data_buffer_pointer = active_table.translate(v_buffer_pointer as usize)
        .ok_or("Couldn't translate the virtual memory address of the buffer pointer to phys_addr!!")?;

    let config_val_frame_index =
            get_config_value(device, dev_request_pointer as u32,
                             data_buffer_pointer as u32,active_table)?;


    let mut config_desc = box_config_desc(active_table,data_buffer_pointer,offset)?;
    let config_value = config_desc.conf_value.read();
    let config_type = config_desc.config_type.read();
    let config_len = config_desc.len.read();
    let total_len = config_desc.total_len.read();
    let num_interface = config_desc.intf_count.read();
    offset += 9;

    debug!("config value {:x}", config_value);
    debug!("config type {:x}", config_type);
    debug!("config len {:x}", config_len);
    debug!("config total len {:x}", total_len);
    debug!("interface number {:x}", num_interface);

    let dev_request_pointer_v = usb_uhci::buffer_pointer_alloc(offset)
        .ok_or("Couldn't get virtual memory address for the get_config_desc request for device in UHCI!!")?;
    let dev_request_pointer = active_table.translate(dev_request_pointer_v as usize)
        .ok_or("Couldn't translate the virtual memory address of the get_config_desc request to phys_addr!!")?;
    let mut set_config_request = box_dev_req(active_table,data_buffer_base,offset)?;
    set_config_request.init(0x00, usb_req::REQ_SET_CONF,
                              config_value as u16, 0,0);
    offset += 8;

    let set_add_frame_index = set_request(device, dev_request_pointer as u32,active_table)?;









    Ok(())


}

/// build a setup transaction in TD to assign a device address to the device
/// Return the physical pointer to this transaction
/// Have not implemented error check yet, need to read status to decide the error of the transaction and return that
/// HID request is little different from standard request, check the Intel USB HID doc to debug this
/// function.
/// Currently this function should work, according to the write back status.
/// But the get_config_value which depends on this function is not working, so check this function also.
pub fn set_device_address(dev: &mut UsbDevice, request_pointer: u32,add: u16, active_table: &mut ActivePageTable) -> Result<usize,&'static str>{



    let frame_index = set_request(dev, request_pointer,active_table)?;
    dev.addr = add as u32;
    Ok(frame_index)
}

/// Get the configuration value which is used to set configuration in the device
/// Currently choose the first configuration and first interface inside
pub fn get_config_value(dev: &UsbDevice,request_pointer: u32,data_buffer_pointer: u32,
                        active_table: &mut ActivePageTable)-> Result<usize,&'static str>{


    //read first 6 bytes of the configuration to read the config value
    // read necessary information to build TDs
    let speed = dev.speed;
    let addr = dev.addr;
    let max_size = dev.maxpacketsize;

    // build the set up transaction within a TD
    let (setup_add,setup_index) = usb_uhci::td_alloc().unwrap()?;
    let setup_add = active_table.translate(setup_add).unwrap();
    usb_uhci::init_td(setup_index,0,0,speed ,addr, 0,0, TD_PACKET_SETUP as u32,
                      8,request_pointer);

    // build the following data transaction within a TD
    let (packet_add,packet_index) = usb_uhci::td_alloc().unwrap()?;
    let packet_add = active_table.translate(packet_add).unwrap();
    usb_uhci::init_td(packet_index,0,0,speed ,addr, 0,1, TD_PACKET_IN as u32,
                      8,data_buffer_pointer);
    usb_uhci::td_link_vf(setup_index,0,packet_add as u32);


    // build the end  transaction within a TD
    let (end_add,end_index) = usb_uhci::td_alloc().unwrap()?;
    let end_add = active_table.translate(end_add).unwrap();
    usb_uhci::init_td(end_index,0,1,speed ,addr, 0,1, TD_PACKET_OUT as u32,
                      NO_DATA,0);
    usb_uhci::td_link_vf(packet_index,0,end_add as u32);

    let (qh_physical_add,qh_index) = usb_uhci::qh_alloc().unwrap()?;
    let qh_physical_add = active_table.translate(qh_physical_add).unwrap();
    usb_uhci::init_qh(qh_index,usb_uhci::TD_PTR_TERMINATE,setup_add as u32);

    let frame_index = usb_uhci:: qh_link_to_framelist(qh_physical_add as u32).unwrap()?;

    // wait for the transfer to be completed
    // Currently the get config transfer is stalled
    // wait for the transfer to be completed
    loop{
        let status = usb_uhci::td_status(packet_index).unwrap()?;

        if status & usb_uhci::TD_CS_ACTIVE == 0{
            debug!("The write back status of this get config transfer,{:x}", status);
            break
        }
    }


    Ok(frame_index)


}

pub fn set_request(dev: &mut UsbDevice, request_pointer: u32, active_table: &mut ActivePageTable) -> Result<usize,&'static str>{

    // read necessary information to build TDs
    let speed = dev.speed;
    let addr = dev.addr;
    let max_size = dev.maxpacketsize;


    // build the setup transaction
    let (setup_add,setup_index) = usb_uhci::td_alloc().unwrap()?;
    let setup_add = active_table.translate(setup_add).unwrap();
    usb_uhci::init_td(setup_index,0,1,speed ,addr, 0,0, TD_PACKET_SETUP as u32,
                      8,request_pointer);


    // build the end transaction
    let (end_add,end_index) = usb_uhci::td_alloc().unwrap()?;
    let end_add = active_table.translate(end_add).unwrap();
    usb_uhci::init_td(end_index,0,1,speed ,addr, 0,1, TD_PACKET_IN as u32,
                      NO_DATA,0);
    usb_uhci::td_link_vf(setup_index,0,end_add as u32);

    //build the queue head
    let (qh_add,qh_index) = usb_uhci::qh_alloc().unwrap()?;
    let qh_add = active_table.translate(qh_add).unwrap();
    usb_uhci::init_qh(qh_index,usb_uhci::TD_PTR_TERMINATE,setup_add as u32);
    let frame_index = usb_uhci:: qh_link_to_framelist(qh_add as u32).unwrap()?;


    // wait for the transfer to be completed
    // Currently no error check
    loop{
        let status = usb_uhci::td_status(end_index).unwrap()?;

        if status & usb_uhci::TD_CS_ACTIVE == 0{
            debug!("The write back status of this set_request transfer,{:x}", status);
            break
        }
    }



    Ok(frame_index)



}

pub fn build_request(active_table: &mut ActivePageTable, req_type: u8, request: u8,
                   value: u16, index: u16, len: u16, offset: usize) -> Result<(usize,usize),&'static str>{

    let dev_request_pointer_v = usb_uhci::buffer_pointer_alloc(offset)
        .ok_or("Couldn't get virtual memory address for the set_address request for device in UHCI!!")?;
    let dev_request_pointer = active_table.translate(dev_request_pointer_v as usize)
        .ok_or("Couldn't translate the virtual memory address of the set_address request to phys_addr!!")?;
    let data_buffer_base = dev_request_pointer;
    let mut address_request = box_dev_req(active_table,data_buffer_base,0)?;
    let new_off = offset + 8;
    address_request.init(0x00, usb_req::REQ_SET_ADDR, 1, 0,0);
    Ok((dev_request_pointer, new_off))
}

//pub fn control_set(active_table: &mut ActivePageTable,request_pointer: u32, dev: &mut UsbDevice){
//
//    // read necessary information to build TDs
//    let speed = dev.speed;
//    let addr = dev.addr;
//    let max_size = dev.maxpacketsize;
//
//
//    // build the set up transaction to set address within a TD
//    let (setup_add,setup_index) = usb_uhci::td_alloc().unwrap()?;
//    let setup_add = active_table.translate(setup_add).unwrap();
//    usb_uhci::init_td(setup_index,0,1,speed ,addr, 0,0, TD_PACKET_SETUP as u32,
//                      8,request_pointer);
//
//
//
//    let (end_add,end_index) = usb_uhci::td_alloc().unwrap()?;
//    let end_add = active_table.translate(end_add).unwrap();
//    usb_uhci::init_td(end_index,0,1,speed ,addr, 0,1, TD_PACKET_IN as u32,
//                      NO_DATA,0);
//    usb_uhci::td_link_vf(setup_index,0,end_add as u32);
//
//
//    let (qh_add,qh_index) = usb_uhci::qh_alloc().unwrap()?;
//    let qh_add = active_table.translate(qh_add).unwrap();
//    usb_uhci::init_qh(qh_index,usb_uhci::TD_PTR_TERMINATE,setup_add as u32);
//    let frame_index = usb_uhci:: qh_link_to_framelist(qh_add as u32).unwrap()?;
//
//
//    // wait for the transfer to be completed
//    // Currently the get config transfer is not working
//    loop{
//        let status = usb_uhci::td_status(end_index).unwrap()?;
//
//        if status & usb_uhci::TD_CS_ACTIVE == 0{
//            debug!("The write back status of this set_request transfer,{:x}", status);
//            break
//        }
//    }
//
//
//}
