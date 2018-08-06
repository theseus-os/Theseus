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
//use usb_uhci::{UhciTDRegisters, UhciQH};
use memory::{get_kernel_mmi_ref,MemoryManagementInfo,FRAME_ALLOCATOR,Frame,PageTable, ActivePageTable, PhysicalAddress, VirtualAddress, EntryFlags, MappedPages, allocate_pages ,allocate_frame};
use core::mem::size_of_val;


const TD_PACKET_IN :u8=                    0x69;
const TD_PACKET_OUT :u8=                   0xe1;
const TD_PACKET_SETUP :u8=                 0x2d;


/// Initialize the USB 1.1 host controller
pub fn init(active_table: &mut ActivePageTable) -> Result<(), &'static str> {


    if let Err(e) = usb_ehci::init(active_table){
        return Err(e);
    }
    if let Err(e) = usb_uhci::init(active_table){
        return Err(e);
    }


    let device = &mut usb_uhci::port1_device_init()?;


    let set_add_request = &UsbDevReq::new(0x00, usb_req::REQ_SET_ADDR, 1, 0,0);
    let set_device_add = set_device_address(device,set_add_request,1,active_table)?;
//
//    let mut offset:usize = 0;
    //   let get_config_len = &UsbDevReq::new(0x00, usb_req::REQ_GET_DESC, usb_desc::USB_DESC_CONF, 0,4);
//    let a = get_device_description_len(device,get_config_len,v_buffer_pointer,active_table,offset);




//    let add = active_table.translate(18446743523955233792).unwrap();
//    info!("physical address of queue head pool:{:?}",add);


//    if usb_uhci::if_enable_port1(){

//        let add_trans = set_device_address(&mut device,1);
//        let pointer = &add_trans as *const UhciTDRegisters;
//        let index = usb_uhci::frame_number() as PhysicalAddress;
//        let base = usb_uhci::frame_list_base() as PhysicalAddress;
//        let mut frame_pointer = usb_uhci::box_frame_list(active_table,base)?;
//        frame_pointer.write(pointer as u32);
//    }


    Ok(())
}

/// build a setup transaction in TD to assign a device address to the device
/// Return the physical pointer to this transaction
pub fn set_device_address(dev: &mut UsbDevice, dev_request:&UsbDevReq,add: u16, active_table: &mut ActivePageTable) -> Result<usize,&'static str>{


    let (setup_add,setup_index) = usb_uhci::td_alloc().unwrap()?;

    // read necessary information to build TDs
    let speed = dev.speed;
    let addr = dev.addr;
//    let max_size = dev.maxpacketsize;
//    let req_type = dev_request.dev_req_type;
    let len = dev_request.len as u32;
    let request_size = size_of_val(dev_request) as u32;

    // get the data buffer physical pointer
    let data_virtual_add= dev_request as *const UsbDevReq;
    let data_buffer_point = active_table.translate(data_virtual_add as usize).unwrap() as u32;

    // build the set up transaction to set address within a TD
    usb_uhci::init_td(setup_index,0,0,speed ,addr, 0,0, TD_PACKET_SETUP as u32,
        request_size,data_buffer_point);

    let (end_add,end_index) = usb_uhci::td_alloc().unwrap()?;

    usb_uhci::init_td(end_index,0,0,speed ,addr, 0,1, TD_PACKET_IN as u32,
                      len,0);

    usb_uhci::td_link(setup_index,0,end_add as u32);

    let (qh_physical_add,qh_index) = usb_uhci::qh_alloc().unwrap()?;
    usb_uhci::init_qh(qh_index,usb_uhci::TD_PTR_TERMINATE,setup_add as u32);


    dev.addr = add as u32;

    let frame_index = usb_uhci:: link_to_framelist(qh_physical_add as u32).unwrap()?;



}

pub fn get_config_value(dev: &UsbDevice,dev_request:&UsbDevReq,
                        active_table: &mut ActivePageTable,offset:usize)-> Result<usize,&'static str>{


    //read first 4 bytes of the configuration

    // read necessary information to build TDs
    let speed = dev.speed;
    let addr = dev.addr;
    let max_size = dev.maxpacketsize;
    let req_type = dev_request.dev_req_type;
    let len = dev_request.len as u32;
    let request_size = size_of_val(dev_request) as u32;
    let mut toggle: u32 = 0;


    // get the data buffer physical pointer
    let data_virtual_add= dev_request as *const UsbDevReq;
    let data_buffer_point = active_table.translate(data_virtual_add as usize).unwrap() as u32;

    // build the set up transaction to set address within a TD
    let (setup_add,setup_index) = usb_uhci::td_alloc().unwrap()?;
    usb_uhci::init_td(setup_index,0,0,speed ,addr, 0,0, TD_PACKET_SETUP as u32,
                      request_size,data_buffer_point);


    let v_buffer_pointer = usb_uhci::buffer_pointer_alloc(offset).unwrap()?;
    let data_buffer_point = active_table.translate(v_buffer_pointer as usize).unwrap() as u32;
    // build the set up transaction to set address within a TD
    let (packet_add,packet_index) = usb_uhci::td_alloc().unwrap()?;
    usb_uhci::init_td(packet_index,0,0,speed ,addr, 0,1, TD_PACKET_IN as u32,
                      len,data_buffer_point);

    usb_uhci::td_link(setup_index,0,packet_add as u32);



    let (end_add,end_index) = usb_uhci::td_alloc().unwrap()?;
    usb_uhci::init_td(end_index,0,0,speed ,addr, 0,1, TD_PACKET_OUT as u32,
                      0,0);

    usb_uhci::td_link(packet_index,0,end_add as u32);

    let (qh_physical_add,qh_index) = usb_uhci::qh_alloc().unwrap()?;
    usb_uhci::init_qh(qh_index,usb_uhci::TD_PTR_TERMINATE,setup_add as u32);


    let frame_index = usb_uhci:: link_to_framelist(qh_physical_add as u32).unwrap()?;

    for x in 0..5{

    };


    let frame_pointer: BoxRefMut<MappedPages, UsbConfDesc>  = BoxRefMut::new(Box::new(usb_uhci::map(active_table,frame_base)?))
        .try_map_mut(|mp| mp.as_type_mut::<[Volatile<u32>;1024]>(0))?;





    Ok(frame_index)


}
///// According to the device information and request to build a setup transaction in TD
//pub fn uhci_dev_request(dev: &UsbDevice, dev_req_type: u8,dev_request: u8, value: u16,
//                       index: u16, len: u16) -> UhciTDRegisters{
//    let dev_request = &UsbDevReq::new(dev_req_type, dev_request, value, index, len);
//    uhci_control_transfer(dev,dev_request)
//
//}

///// According to the request and device information to build a Control Transfer
///// Return: Vec<UhciTDRegisters> (a Vec contains Control transfer's transactions)
//pub fn uhci_control_transfer(dev: &UsbDevice, dev_request: &UsbDevReq)-> UhciTDRegisters{
//
//    ;
//    // read necessary information to build TDs
//    let speed = dev.speed as u32;
//    let addr = dev.addr;
//    let max_size = dev.maxpacketsize;
//    let req_type = dev_request.dev_req_type;
//    let mut len = dev_request.len as u32;
//    let request_size = size_of_val(dev_request) as u32;
//    let mut toggle: u32 = 0;
////    let mut td_list:Vec<&mut UhciTDRegisters> = Vec::new();
//
//    // get the data buffer physical pointer
//    let data_virtual_add= dev_request as *const UsbDevReq;
//    let data_buffer_point = translate_add(data_virtual_add as usize).unwrap();
//
//     build the set up transaction within a TD
//    let setup_transaction = UhciTDRegisters::
//    init(0,speed ,addr, 0,0, TD_PACKET_SETUP as u32,
//         request_size,data_buffer_point as u32);
//
//    setup_transaction
//}

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

