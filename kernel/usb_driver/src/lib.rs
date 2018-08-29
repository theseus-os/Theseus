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
extern crate owning_ref;
#[macro_use] extern crate log;

use owning_ref::BoxRefMut;
use usb_desc::{UsbEndpDesc,UsbDeviceDesc,UsbConfDesc,UsbIntfDesc};
use usb_req::UsbDevReq;
use usb_device::{UsbControlTransfer,UsbDevice,Controller};
use usb_uhci::{box_dev_req,box_config_desc,box_device_desc,box_inter_desc,box_endpoint_desc};
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
    device.maxpacketsize = 8;


    let mut offset:usize = 0;

    let (dev_desc,new_offset) = get_device_desc(device,active_table,offset)?;
    offset = new_offset;

    let len = dev_desc.len.read();
    let maxi = dev_desc.max_packet_size.read();
    let class = dev_desc.class.read();
    let sub_class = dev_desc.sub_class.read();
    let config_count = dev_desc.conf_count.read();
    info!("length of this description: {:b}", len);
    info!("the max size of the control pipe of this device: {:b}",maxi);
    info!("device class code: {:b}",class);
    info!("device sub class code: {:b}",sub_class);
    info!("number of possible cofigurations: {:b}",config_count);

    device.maxpacketsize = maxi as u32;


    let add:u16 = 1;
    let new_offset = set_device_address(device,add,active_table,offset)?;
    offset = new_offset;


    let (config_desc,new_offset) = get_config_desc(device,active_table,offset)?;
    offset = new_offset;
    let len = config_desc.len.read();
    let config_value = config_desc.conf_value.read();
    let total_len = config_desc.total_len.read();
    let inter_num = config_desc.intf_count.read();
    info!("length of this description: {:b}", len);
    info!("config value of this configuration:{:b}", config_value);
    info!("total len of the incoming data of get description request:{:b}", total_len);
    info!("Number of interfaces supported by this configuration:{:b}", inter_num);
    let (config_desc,new_offset) = get_all_desc(device,total_len,active_table,offset)?;
    offset = new_offset;


    let (request_pointer,new_offset) = build_request(active_table,0x00, usb_req::REQ_SET_CONF,
                                                     config_value as u16, 0,0,offset)?;
    let set_add_frame_index = set_request(device, request_pointer as u32,active_table)?;

    Ok(())


}

/// build a setup transaction in TD to assign a device address to the device
/// Return the physical pointer to this transaction
/// Have not implemented error check yet, need to read status to decide the error of the transaction and return that
/// HID request is little different from standard request, check the Intel USB HID doc to debug this
/// function.
/// Currently this function should work, according to the write back status.
/// But the get_config_value which depends on this function is not working, so check this function also.
pub fn set_device_address(dev: &mut UsbDevice,add: u16, active_table: &mut ActivePageTable,offset: usize) -> Result<usize,&'static str>{


    let (request_pointer,new_offset) = build_request(active_table,0x00, usb_req::REQ_SET_ADDR, add, 0,0,offset)?;
    let frame_index = set_request(dev, request_pointer as u32,active_table)?;
    dev.addr = add as u32;
    Ok(new_offset)
}

/// Get the configuration description
pub fn get_config_desc(dev: &UsbDevice, active_table: &mut ActivePageTable, offset: usize)-> Result<(BoxRefMut<MappedPages, UsbConfDesc>,usize),&'static str>{

    let (request_pointer,new_offset) = build_request(active_table,0x80, usb_req::REQ_GET_DESC,
                                                         usb_desc::USB_DESC_CONF, 0,9,offset)?;
    let v_buffer_pointer = usb_uhci::buffer_pointer_alloc(new_offset)
        .ok_or("Couldn't get virtual memory address for the buffer pointer in get_config_desc request for device in UHCI!!")?;
    let data_buffer_pointer = active_table.translate(v_buffer_pointer as usize)
        .ok_or("Couldn't translate the virtual memory address of the buffer pointer to phys_addr!!")?;

    let new_off = get_request(dev, request_pointer as u32, data_buffer_pointer as u32,9,new_offset,active_table)?;


    let mut config_desc = box_config_desc(active_table,data_buffer_pointer,new_offset)?;
    Ok((config_desc,new_off))


}

/// Get the all descriptions
pub fn get_all_desc(dev: &UsbDevice, total_len: u16, active_table: &mut ActivePageTable, offset: usize)-> Result<(BoxRefMut<MappedPages, UsbConfDesc>,usize),&'static str>{

    let (request_pointer,mut new_offset) = build_request(active_table,0x80, usb_req::REQ_GET_DESC,
                                                     usb_desc::USB_DESC_CONF, 0,total_len,offset)?;
    let v_buffer_pointer = usb_uhci::buffer_pointer_alloc(new_offset)
        .ok_or("Couldn't get virtual memory address for the buffer pointer in get_config_desc request for device in UHCI!!")?;
    let data_buffer_pointer = active_table.translate(v_buffer_pointer as usize)
        .ok_or("Couldn't translate the virtual memory address of the buffer pointer to phys_addr!!")?;

    let new_off = get_request(dev, request_pointer as u32,
                              data_buffer_pointer as u32,total_len as u32,new_offset,active_table)?;


    let mut config_desc = box_config_desc(active_table,data_buffer_pointer,new_offset)?;
    new_offset +=  (config_desc.len.read() as usize);
    let inter_num = config_desc.intf_count.read();
    info!("interface number : {:x}", inter_num);
    info!("config len : {:x}", config_desc.len.read());
    for _x in 0..inter_num{
        info!("woshinibaba");
        let mut inter_desc = box_inter_desc(active_table,data_buffer_pointer,new_offset)?;
        let endpoint_num = inter_desc.endp_count.read();
        new_offset += (inter_desc.len.read() as usize);
        let class_code = inter_desc.class.read();
        let protocal = inter_desc.protocol.read();
        let interface_number = inter_desc.intf_num.read();
        info!("endpoint number : {:x}", endpoint_num);
        info!("interface len : {:x}", inter_desc.len.read());
        info!("base class : {:x}", class_code);
        info!("protocol: {:x}",protocal);
        info!("interface num: {:x}",interface_number);
        for _y in 0..endpoint_num{

            let end_desc = box_endpoint_desc(active_table, data_buffer_pointer,new_offset)?;
            let endpoint_add = end_desc.addr.read();
            let endpoint_len = end_desc.len.read();
            new_offset += (endpoint_len as usize);
            let endpoint_type = end_desc.endp_type.read();
            let attribute = end_desc.attributes.read();
            debug!("the endpoint address: {:x}, and type: {:x} and attribute: {:x}",endpoint_add,endpoint_type,attribute);
        }

    }


    Ok((config_desc,new_off))


}



/// Get the device description
pub fn get_device_desc(dev: &UsbDevice, active_table: &mut ActivePageTable, offset: usize)-> Result<(BoxRefMut<MappedPages, UsbDeviceDesc>,usize),&'static str>{


    let (request_pointer,new_offset) = build_request(active_table,0x80, usb_req::REQ_GET_DESC, usb_desc::USB_DESC_DEVICE, 0,18,offset)?;
    let v_buffer_pointer = usb_uhci::buffer_pointer_alloc(new_offset)
        .ok_or("Couldn't get virtual memory address for the buffer pointer in get_config_desc request for device in UHCI!!")?;
    let data_buffer_pointer = active_table.translate(v_buffer_pointer as usize)
        .ok_or("Couldn't translate the virtual memory address of the buffer pointer to phys_addr!!")?;

    let new_off =
        get_request(dev, request_pointer as u32, data_buffer_pointer as u32,18,new_offset,active_table)?;

    let mut device_desc = box_device_desc(active_table,data_buffer_pointer,new_offset)?;

    Ok((device_desc,new_off))


}

/// Get the configuration value which is used to set configuration in the device
/// Currently choose the first configuration and first interface inside
pub fn get_request(dev: &UsbDevice,request_pointer: u32,data_buffer_pointer: u32, data_size: u32, offset: usize,
                        active_table: &mut ActivePageTable)-> Result<usize,&'static str>{


    //read first 6 bytes of the configuration to read the config value
    // read necessary information to build TDs
    let mut data_buffer_pointer = data_buffer_pointer;
    let mut new_off = offset;
    let speed = dev.speed;
    let addr = dev.addr;
    let max_size = dev.maxpacketsize;

    let mut toggle = 0;

    // build the set up transaction within a TD
    let (setup_add,setup_index) = usb_uhci::td_alloc().unwrap()?;
    let setup_add = active_table.translate(setup_add).unwrap();
    usb_uhci::init_td(setup_index,0,0,speed ,addr, 0,toggle, TD_PACKET_SETUP as u32,
                      8,request_pointer);

    // build the following data transaction within a TD
    let mut data_size = data_size;
    let mut link_index = setup_index;
    let mut report_index: usize;
    let mut last_index: usize;
    loop{
        toggle ^= 1;
        if data_size > max_size{

            let (packet_add,packet_index) = usb_uhci::td_alloc().unwrap()?;
            let packet_add = active_table.translate(packet_add).unwrap();
            usb_uhci::init_td(packet_index,0,0,speed ,addr, 0,toggle, TD_PACKET_IN as u32,
                              max_size,data_buffer_pointer);
            usb_uhci::td_link_vf(link_index,0,packet_add as u32);
            data_buffer_pointer += max_size;
            new_off += max_size as usize;

            data_size -= max_size;
            link_index = packet_index;




        }else{
            let (packet_add,packet_index) = usb_uhci::td_alloc().unwrap()?;
            let packet_add = active_table.translate(packet_add).unwrap();
            usb_uhci::init_td(packet_index,0,0,speed ,addr, 0,toggle, TD_PACKET_IN as u32,
                              data_size,data_buffer_pointer);
            usb_uhci::td_link_vf(link_index,0,packet_add as u32);
            new_off += data_size as usize;



            let (end_add,end_index) = usb_uhci::td_alloc().unwrap()?;
            let end_add = active_table.translate(end_add).unwrap();
            usb_uhci::init_td(end_index,0,1,speed ,addr, 0,1, TD_PACKET_OUT as u32,
                              NO_DATA,0);
            usb_uhci::td_link_vf(packet_index,0,end_add as u32);
            report_index = end_index;
            last_index = packet_index;


            break;

        }

    }

    let (qh_physical_add,qh_index) = usb_uhci::qh_alloc().unwrap()?;
    let qh_physical_add = active_table.translate(qh_physical_add).unwrap();
    usb_uhci::init_qh(qh_index,usb_uhci::TD_PTR_TERMINATE,setup_add as u32);

    let frame_index = usb_uhci:: qh_link_to_framelist(qh_physical_add as u32).unwrap()?;

    // wait for the transfer to be completed
    // Currently the get config transfer is stalled
    // wait for the transfer to be completed
    loop{
        let status = usb_uhci::td_status(last_index).unwrap()?;

        if status & usb_uhci::TD_CS_ACTIVE == 0{
            debug!("The write back status of the last data,{:x}", status);
            break
        }
    }
    loop{
        let status = usb_uhci::td_status(report_index).unwrap()?;

        if status & usb_uhci::TD_CS_ACTIVE == 0{
            debug!("The write back status of this get request transfer,{:x}", status);
            break
        }
    }


    Ok(new_off)


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
    let mut address_request = box_dev_req(active_table,dev_request_pointer,offset)?;
    let new_off = offset + 8;
    address_request.init(req_type, request, value, index,len);
    Ok((dev_request_pointer, new_off))
}


