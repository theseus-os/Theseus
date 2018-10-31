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
extern crate usb_keyboard;
#[macro_use] extern crate log;

use owning_ref::BoxRefMut;
use usb_desc::{UsbDeviceDesc,UsbConfDesc};
use usb_keyboard::box_keyboard_buffer;
use usb_device::{UsbDevice,HIDType};
use usb_uhci::{box_dev_req,box_config_desc,box_device_desc,box_inter_desc,box_endpoint_desc,clean_a_frame};
use memory::{ActivePageTable,MappedPages};



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

    let mut index: usize = 0;
    if let Ok(device_1) = port_1_enum(active_table){
        usb_uhci::device_register(index, device_1);
        match device_1.device_type{

            HIDType::Keyboard => {
                if let Err(e) = usb_keyboard::init(active_table,index){
                    return Err(e);
                }
            },
            HIDType::Mouse =>{

            },
            HIDType::Unknown =>{

                return Err("Only support USB Mice & Keyboards currently")
            }

        }
        index += 1;
    }else{

        info!("No device is attached to port 1")
    }




    if let Ok(device_2) = port_2_enum(active_table){
        usb_uhci::device_register(index, device_2);
        match device_2.device_type{

            HIDType::Keyboard => {
                if let Err(e) = usb_keyboard::init(active_table,index){
                    return Err(e);}
            },
            HIDType::Mouse =>{

            },
            HIDType::Unknown =>{

                return Err("Only support USB Mice & Keyboards currently")
            }

        }

    }else{

        info!("No device is attached to port 2")
    }


    Ok(())


}


/// Configure the device attached to the UCHI's port 1
fn port_1_enum(active_table: &mut ActivePageTable) -> Result<(UsbDevice),&'static str>{

    if let Ok(mut device) = usb_uhci::port1_device_init(){
        info!("port_1_enum: speed {:?}", device.speed);



        device.maxpacketsize = 8;


        let mut offset:usize = 0;


        let (dev_desc,new_offset) = get_device_desc(& device,active_table,offset)?;
        offset = new_offset;

        let maxi = dev_desc.max_packet_size.read();
        let config_count = dev_desc.conf_count.read();
        info!("The max size of the control pipe of this device: {:b}",maxi);
        info!("Number of possible configurations of this device: {:b}",config_count);

        device.maxpacketsize = maxi as u32;




        let add:u16 = 1;
        let new_offset = set_device_address(&mut device,add,active_table,offset)?;
        offset = new_offset;


        let (config_desc,new_offset) = get_config_desc(& device,active_table,offset)?;
        offset = new_offset;






        let total_len = config_desc.total_len.read();
        let (config_desc,new_offset) = set_device(&mut device,total_len,active_table,offset)?;
        offset = new_offset;
        let config_value = config_desc.conf_value.read();
        let inter_num = config_desc.intf_count.read();
        info!("Configuration value : {:x}", config_value);
        info!("Number of interfaces supported by this configuration:{:b}", inter_num);

        let (request_pointer,new_offset) = build_request(active_table,0x00, usb_req::REQ_SET_CONF,
                                                         0 as u16, 0,0,offset)?;
        set_request(&mut device, request_pointer as u32,active_table)?;

        offset = new_offset;


        let (request_pointer,new_offset) = build_request(active_table,0x00, usb_req::REQ_SET_CONF,
                                                         config_value as u16, 0,0,offset)?;
        set_request(&mut device, request_pointer as u32,active_table)?;

        offset = new_offset;

        info!("{:?}", device);
        if device.device_type == HIDType::Mouse || device.device_type == HIDType::Keyboard{

            let (request_pointer,new_offset) = build_request(active_table,0x21, usb_req::REQ_SET_IDLE,
                                                             0, 0,0,offset)?;
            set_request(&mut device, request_pointer as u32,active_table)?;


        }
        info!("USB {:?} is registered",device.device_type);

        Ok(device)
    }else{
        Err("No device attached to port 1 of UHCI")
    }

}

/// Configure the device attached to the UCHI's port 2
fn port_2_enum(active_table: &mut ActivePageTable) -> Result<(UsbDevice),&'static str>{

    if let Ok(mut device) = usb_uhci::port2_device_init(){

        device.maxpacketsize = 8;


        let mut offset:usize = 0;


        let (dev_desc,new_offset) = get_device_desc(& device,active_table,offset)?;
        offset = new_offset;

        let maxi = dev_desc.max_packet_size.read();
        let config_count = dev_desc.conf_count.read();
        info!("The max size of the control pipe of this device: {:b}",maxi);
        info!("Number of possible configurations of this device: {:b}",config_count);

        device.maxpacketsize = maxi as u32;




        let add:u16 = 2;
        let new_offset = set_device_address(&mut device,add,active_table,offset)?;
        offset = new_offset;


        let (config_desc,new_offset) = get_config_desc(& device,active_table,offset)?;
        offset = new_offset;






        let total_len = config_desc.total_len.read();
        let (config_desc,new_offset) = set_device(&mut device,total_len,active_table,offset)?;
        offset = new_offset;
        let config_value = config_desc.conf_value.read();
        let inter_num = config_desc.intf_count.read();
        info!("Configuration value : {:x}", config_value);
        info!("Number of interfaces supported by this configuration:{:b}", inter_num);

        let (request_pointer,new_offset) = build_request(active_table,0x00, usb_req::REQ_SET_CONF,
                                                         0 as u16, 0,0,offset)?;
        set_request(&mut device, request_pointer as u32,active_table)?;

        offset = new_offset;


        let (request_pointer,new_offset) = build_request(active_table,0x00, usb_req::REQ_SET_CONF,
                                                         config_value as u16, 0,0,offset)?;
        set_request(&mut device, request_pointer as u32,active_table)?;

        offset = new_offset;


        info!("{:?}", device);
        if device.device_type == HIDType::Mouse || device.device_type == HIDType::Keyboard{

            let (request_pointer,new_offset) = build_request(active_table,0x21, usb_req::REQ_SET_IDLE,
                                                             0, 0,0,offset)?;
            set_request(&mut device, request_pointer as u32,active_table)?;

        }
        info!("USB {:?} is registered",device.device_type);

        Ok(device)
    }else{
        Err("No device attached to port 2 of UHCI")
    }

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
    let _result = set_request(dev, request_pointer as u32,active_table)?;
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


    let config_desc = box_config_desc(active_table,data_buffer_pointer,new_offset)?;
    Ok((config_desc,new_off))


}

/// Get the value of current current configuration
pub fn get_config(dev: &UsbDevice, active_table: &mut ActivePageTable, offset: usize)-> Result<(u8,u8,usize),&'static str>{

    let (request_pointer,new_offset) = build_request(active_table,0x80, usb_req::REQ_GET_CONF,
                                                     0, 0,1,offset)?;

    let v_buffer_pointer = usb_uhci::buffer_pointer_alloc(new_offset)
        .ok_or("Couldn't get virtual memory address for the buffer pointer in get_config_desc request for device in UHCI!!")?;
    let data_buffer_pointer = active_table.translate(v_buffer_pointer as usize)
        .ok_or("Couldn't translate the virtual memory address of the buffer pointer to phys_addr!!")?;

    let value_1 =  box_dev_req(active_table,data_buffer_pointer,new_offset)?.dev_req_type.read();

    let new_off = get_request(dev, request_pointer as u32, data_buffer_pointer as u32,1,new_offset,active_table)?;

    let value =  box_dev_req(active_table,data_buffer_pointer,new_offset)?.dev_req_type.read();

    Ok((value_1,value,new_off))


}

/// Get the data report of a hid device
/// Use to test whether the device is ready to send data
pub fn get_report(dev: &UsbDevice, active_table: &mut ActivePageTable, offset: usize)-> Result<(usize), &'static str>{

    let data_size: u16 = 8;
    let (request_pointer,new_offset) = build_request(active_table,0xA1, 1,
                                                     0x100, 0,data_size,offset)?;
    info!("offset in get report: {:x}", offset);
    info!("new offset for data buffer in get report: {:x}", new_offset);

    let v_buffer_pointer = usb_uhci::buffer_pointer_alloc(new_offset)
        .ok_or("Couldn't get virtual memory address for the buffer pointer in get_config_desc request for device in UHCI!!")?;
    let data_buffer_pointer = active_table.translate(v_buffer_pointer as usize)
        .ok_or("Couldn't translate the virtual memory address of the buffer pointer to phys_addr!!")? ;

    let buffer = box_keyboard_buffer(active_table,data_buffer_pointer,new_offset)?;
    debug!("see whether there data coming: {:?}", buffer);

    let mut new_off = new_offset;
    let speed = dev.speed;
    let addr = dev.addr;
    let endpoint = dev.interrupt_endpoint  as u32;


    let mut toggle = 0;

    // build the set up transaction within a TD
    let (setup_add,setup_index) = usb_uhci::td_alloc().ok_or("Cannot allocate a new Transfer Head")?;
    let setup_add = active_table.translate(setup_add).unwrap();
    usb_uhci::init_td(setup_index,0,0,speed ,addr, 0,toggle, TD_PACKET_SETUP as u32,
                      8,request_pointer as u32);

    toggle ^= 1;
    let (packet_add,packet_index) = usb_uhci::td_alloc().ok_or("Cannot allocate a new Transfer Head")?;
    let packet_add = active_table.translate(packet_add).unwrap();
    usb_uhci::init_td(packet_index,0,0,speed ,addr, endpoint,toggle, TD_PACKET_IN as u32,
                      data_size as u32,data_buffer_pointer as u32);
    usb_uhci::td_link_vf(setup_index,0,packet_add as u32);
    new_off += data_size as usize;

    let (qh_physical_add,qh_index) = usb_uhci::qh_alloc().ok_or("Cannot allocate new uchi queue head")?;
    let qh_physical_add = active_table.translate(qh_physical_add).unwrap();
    usb_uhci::init_qh(qh_index,usb_uhci::TD_PTR_TERMINATE,setup_add as u32);


    let _frame_index = usb_uhci:: qh_link_to_framelist(qh_physical_add as u32).ok_or("Cannot find available frame for the queue head")?;

    loop{
        let status = usb_uhci::td_status(setup_index).ok_or("Cannot read the td status")?;

        if status & usb_uhci::TD_CS_ACTIVE == 0{
            info!("get report last packet status bowen: {:x}", status);
            break
        }
    }

    let buffer = box_keyboard_buffer(active_table,data_buffer_pointer,new_offset)?;
    debug!("see whether there data coming: {:?}", buffer);


    Ok(new_off)
}

/// Get the complete USB device descriptions
pub fn set_device(dev: &mut UsbDevice, total_len: u16, active_table: &mut ActivePageTable, offset: usize)-> Result<(BoxRefMut<MappedPages, UsbConfDesc>,usize),&'static str>{

    let (request_pointer,mut new_offset) = build_request(active_table,0x80, usb_req::REQ_GET_DESC,
                                                     usb_desc::USB_DESC_CONF, 0,total_len,offset)?;
    let v_buffer_pointer = usb_uhci::buffer_pointer_alloc(new_offset)
        .ok_or("Couldn't get virtual memory address for the buffer pointer in get_config_desc request for device in UHCI!!")?;
    let data_buffer_pointer = active_table.translate(v_buffer_pointer as usize)
        .ok_or("Couldn't translate the virtual memory address of the buffer pointer to phys_addr!!")?;

    let new_off = get_request(dev, request_pointer as u32,
                              data_buffer_pointer as u32,total_len as u32,new_offset,active_table)?;


    let config_desc = box_config_desc(active_table,data_buffer_pointer,new_offset)?;
    new_offset +=  config_desc.len.read() as usize;



    let inter_desc = box_inter_desc(active_table,data_buffer_pointer,new_offset)?;
    let endpoint_num = inter_desc.endp_count.read() + 2;
    new_offset += inter_desc.len.read() as usize;
    let class_code = inter_desc.class.read();
    let sub_class_code = inter_desc.sub_class.read();
    let protocal = inter_desc.protocol.read();
    if class_code == 3 && sub_class_code == 1{
        if protocal == 1{
            dev.device_type = HIDType::Keyboard;
        }
        else if protocal == 2{
            dev.device_type = HIDType::Mouse;
        }else{
            return Err("The usb driver right now only supports the HID device: Mouse and Keyboard");
        }
    }else{

        return Err("The usb driver right now only supports the HID device: Mouse and Keyboard");
    }
    for _y in 0..endpoint_num{
        let end_desc = box_endpoint_desc(active_table, data_buffer_pointer,new_offset)?;
        let endpoint_len = end_desc.len.read();
        let endpoint_add = end_desc.addr.read() & 0xf;


        new_offset += endpoint_len as usize;
        let desc_type = end_desc.endp_type.read();
        let attribute = end_desc.attributes.read()  & 0b11;

        if desc_type == 5{
            match attribute{
                0b00 => dev.control_endpoint = endpoint_add,

                0b01 => dev.iso_endpoint = endpoint_add,
                0b11 => dev.interrupt_endpoint = endpoint_add,
                _ => {},
            }
        }

    }




    Ok((config_desc,new_off))


}



/// Get the USB device description
pub fn get_device_desc(dev: &UsbDevice, active_table: &mut ActivePageTable, offset: usize)-> Result<(BoxRefMut<MappedPages, UsbDeviceDesc>,usize),&'static str>{


    let (request_pointer,new_offset) = build_request(active_table,0x80, usb_req::REQ_GET_DESC, usb_desc::USB_DESC_DEVICE, 0,18,offset)?;
    let v_buffer_pointer = usb_uhci::buffer_pointer_alloc(new_offset)
        .ok_or("Couldn't get virtual memory address for the buffer pointer in get_config_desc request for device in UHCI!!")?;
    let data_buffer_pointer = active_table.translate(v_buffer_pointer as usize)
        .ok_or("Couldn't translate the virtual memory address of the buffer pointer to phys_addr!!")?;

    let new_off =
        get_request(dev, request_pointer as u32, data_buffer_pointer as u32,18,new_offset,active_table)?;

    let device_desc = box_device_desc(active_table,data_buffer_pointer,new_offset)?;

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
    let (setup_add,setup_index) = usb_uhci::td_alloc().ok_or("Cannot allocate a new Transfer Head")?;
    let setup_add = active_table.translate(setup_add).unwrap();
    usb_uhci::init_td(setup_index,0,0,speed ,addr, 0,toggle, TD_PACKET_SETUP as u32,
                      8,request_pointer);

    // build the following data transaction within a TD
    let mut data_size = data_size;
    let mut link_index = setup_index;
    let report_index: usize;

    loop{
        toggle ^= 1;
        if data_size > max_size{

            let (packet_add,packet_index) = usb_uhci::td_alloc().ok_or("Cannot allocate a new Transfer Head")?;
            let packet_add = active_table.translate(packet_add).unwrap();
            usb_uhci::init_td(packet_index,0,0,speed ,addr, 0,toggle, TD_PACKET_IN as u32,
                              max_size,data_buffer_pointer);
            usb_uhci::td_link_vf(link_index,0,packet_add as u32);
            data_buffer_pointer += max_size;
            new_off += max_size as usize;

            data_size -= max_size;
            link_index = packet_index;




        }else{
            let (packet_add,packet_index) = usb_uhci::td_alloc().ok_or("Cannot allocate a new Transfer Head")?;
            let packet_add = active_table.translate(packet_add).unwrap();
            usb_uhci::init_td(packet_index,0,0,speed ,addr, 0,toggle, TD_PACKET_IN as u32,
                              data_size,data_buffer_pointer);
            usb_uhci::td_link_vf(link_index,0,packet_add as u32);
            new_off += data_size as usize;


            link_index = packet_index;
            let (end_add,end_index) = usb_uhci::td_alloc().ok_or("Cannot allocate a new Transfer Head")?;
            let end_add = active_table.translate(end_add).unwrap();
            usb_uhci::init_td(end_index,0,1,speed ,addr, 0,1, TD_PACKET_OUT as u32,
                              NO_DATA,0);
            usb_uhci::td_link_vf(packet_index,0,end_add as u32);
            report_index = end_index;



            break;

        }

    }

    let (qh_physical_add,qh_index) = usb_uhci::qh_alloc().ok_or("Cannot allocate new uchi queue head")?;
    let qh_physical_add = active_table.translate(qh_physical_add).unwrap();
    usb_uhci::init_qh(qh_index,usb_uhci::TD_PTR_TERMINATE,setup_add as u32);

    let frame_index = usb_uhci:: qh_link_to_framelist(qh_physical_add as u32).ok_or("Cannot find available frame for the queue head")?;

    // wait for the transfer to be completed
    // Currently the get config transfer is stalled
    // wait for the transfer to be completed
    loop{
        let status = usb_uhci::td_status(link_index).ok_or("Cannot read the td status")?;
        if status & usb_uhci::TD_CS_ACTIVE == 0{
            break
        }
    }
    loop{
        let status = usb_uhci::td_status(report_index).ok_or("Cannot read the td status")?;

        if status & usb_uhci::TD_CS_ACTIVE == 0{
            break
        }
    }
    clean_a_frame(frame_index);
    Ok(new_off)


}



pub fn set_request(dev: &mut UsbDevice, request_pointer: u32, active_table: &mut ActivePageTable) -> Result<(),&'static str>{

    // read necessary information to build TDs
    let speed = dev.speed;
    let addr = dev.addr;



    // build the setup transaction
    let (setup_add,setup_index) = usb_uhci::td_alloc().ok_or("Cannot allocate a new Transfer Head")?;
    let setup_add = active_table.translate(setup_add).unwrap();
    usb_uhci::init_td(setup_index,0,1,speed ,addr, 0,0, TD_PACKET_SETUP as u32,
                      8,request_pointer);


    // build the end transaction
    let (end_add,end_index) = usb_uhci::td_alloc().ok_or("Cannot allocate a new Transfer Head")?;
    let end_add = active_table.translate(end_add).unwrap();
    usb_uhci::init_td(end_index,0,1,speed ,addr, 0,1, TD_PACKET_IN as u32,
                      NO_DATA,0);
    usb_uhci::td_link_vf(setup_index,0,end_add as u32);

    //build the queue head
    let (qh_add,qh_index) = usb_uhci::qh_alloc().ok_or("Cannot allocate new uchi queue head")?;
    let qh_add = active_table.translate(qh_add).unwrap();
    usb_uhci::init_qh(qh_index,usb_uhci::TD_PTR_TERMINATE,setup_add as u32);
    let frame_index = usb_uhci:: qh_link_to_framelist(qh_add as u32).ok_or("Cannot find available frame for the queue head")?;


    // wait for the transfer to be completed
    // Currently no error check
    loop{
        let status = usb_uhci::td_status(setup_index).ok_or("Cannot read the td status")?;

        if status & usb_uhci::TD_CS_ACTIVE == 0{
            break
        }
    }


    clean_a_frame(frame_index);



    Ok(())



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


