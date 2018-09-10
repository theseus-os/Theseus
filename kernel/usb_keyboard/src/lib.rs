#![no_std]
#![feature(alloc)]

#![allow(dead_code)]

extern crate alloc;
extern crate owning_ref;
extern crate usb_uhci;
extern crate usb_desc;
extern crate usb_device;
extern crate memory;
extern crate spin;
extern crate volatile;
#[macro_use] extern crate log;

use alloc::boxed::Box;
use spin::{Once, Mutex};
use volatile:: ReadOnly;
use usb_desc::{UsbEndpDesc,UsbDeviceDesc,UsbConfDesc,UsbIntfDesc};
use usb_device::{UsbControlTransfer,UsbDevice,Controller,HIDType};
use usb_uhci::{box_dev_req,box_config_desc,box_device_desc,box_inter_desc,box_endpoint_desc,map,UHCI_STS_PORT,UHCI_CMD_PORT};
use memory::{get_kernel_mmi_ref,MemoryManagementInfo,FRAME_ALLOCATOR,Frame,PageTable, ActivePageTable, PhysicalAddress, VirtualAddress, EntryFlags, MappedPages, allocate_pages ,allocate_frame};
use owning_ref:: BoxRefMut;

static USB_KEYBOARD_INPUT_BUFFER: Once<Mutex<BoxRefMut<MappedPages, [ReadOnly<u8>;8]>>> = Once::new();

const TD_PACKET_IN : u8=                    0x69;
const TD_PACKET_OUT : u8=                   0xe1;

pub fn init(active_table: &mut ActivePageTable, device:& UsbDevice)-> Result<(),&'static str>{

    let v_buffer_pointer = usb_uhci::buffer_pointer_alloc(0)
        .ok_or("Couldn't get virtual memory address for the buffer pointer in get_config_desc request for device in UHCI!!")?;
    let data_buffer_pointer = active_table.translate(v_buffer_pointer as usize)
        .ok_or("Couldn't translate the virtual memory address of the buffer pointer to phys_addr!!")?;

    let _x = recieve_data(active_table,device,data_buffer_pointer as u32)?;
    let k_buffer = box_keyboard_buffer(active_table,data_buffer_pointer,0)?;

    USB_KEYBOARD_INPUT_BUFFER.call_once(||{
        Mutex::new(k_buffer)
    });

    Ok(())

}

pub fn recieve_data(active_table: &mut ActivePageTable, device:& UsbDevice, data_buffer_pointer: u32)-> Result<(),&'static str>{


    let speed= device.speed;
    info!("speed :{:x}", speed);
    let addr = device.addr;
    let max_size = device.maxpacketsize;
    let endpoint = device.interrupt_endpoint;
    let mut toggle = 0;
    let (packet_add,packet_index) = usb_uhci::td_alloc().unwrap()?;
    let packet_add = active_table.translate(packet_add).unwrap();
    usb_uhci::init_td(packet_index,0,0,speed ,addr, endpoint as u32,toggle, TD_PACKET_IN as u32,
                      max_size,data_buffer_pointer);
    info!("td index: {:x}",packet_index);


    let (qh_physical_add,qh_index) = usb_uhci::qh_alloc().unwrap()?;
    let qh_physical_add = active_table.translate(qh_physical_add).unwrap();
    usb_uhci::init_qh(qh_index,usb_uhci::TD_PTR_TERMINATE,packet_add as u32);
    info!("qh index: {:x}",qh_index);
    let frame_index = usb_uhci:: qh_link_to_framelist(qh_physical_add as u32).unwrap()?;

    for _x in 0..10{

        let status = usb_uhci::td_status(packet_index).unwrap()?;
        info!("data status :{:x}",status);

    }

    info!("frame index: {:x}", frame_index);
    info!("see the contends: {:x}", qh_physical_add);
    info!("see the contends: {:x}", usb_uhci::frame_link_pointer(frame_index).unwrap()?);
    info!("\nUHCI USBSTS: {:b}\n", UHCI_STS_PORT.lock().read());
    info!("\nUHCI USBCMD: {:b}\n", UHCI_CMD_PORT.lock().read());

    Ok(())

}


/// Box the the keyboard input data buffer
pub fn box_keyboard_buffer(active_table: &mut ActivePageTable, frame_base: PhysicalAddress, offset: PhysicalAddress)
                      -> Result<BoxRefMut<MappedPages, [ReadOnly<u8>;8]>, &'static str>{


    let buffer: BoxRefMut<MappedPages, [ReadOnly<u8>;8]>  = BoxRefMut::new(Box::new(map(active_table,frame_base)?))
        .try_map_mut(|mp| mp.as_type_mut::<[ReadOnly<u8>;8]>(offset))?;


    Ok(buffer)
}

