#![no_std]
#![feature(alloc)]

#![allow(dead_code)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate volatile;
extern crate owning_ref;
extern crate irq_safety;
extern crate atomic_linked_list;
extern crate memory;
extern crate spin;
extern crate kernel_config;
extern crate port_io;
extern crate spawn;
extern crate usb_device;
extern crate usb_desc;
extern crate usb_req;


use core::ops::DerefMut;
use volatile::{Volatile, ReadOnly, WriteOnly};
use alloc::boxed::Box;
use port_io::Port;
use owning_ref::{BoxRef, BoxRefMut};
use spin::{Once, Mutex};
use memory::{Frame,PageTable, ActivePageTable, PhysicalAddress, VirtualAddress, EntryFlags, MappedPages, allocate_pages,allocate_frame,FRAME_ALLOCATOR};
use usb_device::{UsbDevice,Controller,HIDType};
use usb_desc::{UsbDeviceDesc,UsbConfDesc, UsbIntfDesc, UsbEndpDesc};
use usb_req::UsbDevReq;

pub static UHCI_CMD_PORT:  Mutex<Port<u16>> = Mutex::new(Port::new(0xC040));
pub static UHCI_STS_PORT:  Mutex<Port<u16>> = Mutex::new(Port::new(0xC042));
static UHCI_INT_PORT:  Mutex<Port<u16>> = Mutex::new(Port::new(0xC044));
static UHCI_FRNUM_PORT:  Mutex<Port<u16>> = Mutex::new(Port::new(0xC046));
static UHCI_FRBASEADD_PORT:  Mutex<Port<u32>> = Mutex::new(Port::new(0xC048));
static UHCI_SOFMD_PORT:  Mutex<Port<u16>> = Mutex::new(Port::new(0xC04C));
static REG_PORT1:  Mutex<Port<u16>> = Mutex::new(Port::new(0xC050));
static REG_PORT2:  Mutex<Port<u16>> = Mutex::new(Port::new(0xC052));
static QH_POOL: Once<Mutex<BoxRefMut<MappedPages, [UhciQH;MAX_QH]>>> = Once::new();
static TD_POOL: Once<Mutex<BoxRefMut<MappedPages, [UhciTDRegisters;MAX_TD]>>> = Once::new();
static UHCI_DEVICE_POOL: Once<Mutex<BoxRefMut<MappedPages, [UsbDevice;2]>>> = Once::new();
static UHCI_FRAME_LIST: Once<Mutex<BoxRefMut<MappedPages, [Volatile<u32>;1024]>>> = Once::new();
static DATA_BUFFER: Once<Mutex<MappedPages>> = Once::new();


// ------------------------------------------------------------------------------------------------
// ------------------------------------------------------------------------------------------------
// USB Limits

static USB_STRING_SIZE:u8=                 127;

// ------------------------------------------------------------------------------------------------
// USB Speeds

static USB_FULL_SPEED:u8=                  0x00;
static USB_LOW_SPEED:u8=                   0x01;
static USB_HIGH_SPEED:u8=                  0x02;


/// Initialize the USB 1.1 host controller
pub fn init(active_table: &mut ActivePageTable) -> Result<(), &'static str> {



    run(0);
    short_packet_int(1);

    ioc_int(1);

    port1_reset();

    if if_connect_port1(){
        port1_enable(1);
    }

    port2_reset();

    if if_connect_port2(){
        port2_enable(1);
    }

    let frame_list = box_frame_list(active_table,
                                    UHCI_FRBASEADD_PORT.lock().read() as PhysicalAddress)?;
    UHCI_FRAME_LIST.call_once(|| {
        Mutex::new(frame_list)
    });

    let qh_pool = box_qh_pool(active_table)?;
    QH_POOL.call_once(||{
        Mutex::new(qh_pool)
    });

    let td_pool = box_td_pool(active_table)?;
    TD_POOL.call_once(||{
        Mutex::new(td_pool)
    });
    
    let device_pool = box_device_pool(active_table)?;
    UHCI_DEVICE_POOL.call_once(||{
        Mutex::new(device_pool)
    });

    let buffer = map_pool(active_table)?;
    DATA_BUFFER.call_once(||{
        Mutex::new(buffer)
    });


    clean_framelist();


    run(1);
    info!("\nUHCI USBCMD: {:b}\n", UHCI_CMD_PORT.lock().read());
    info!("\nUHCI USBSTS: {:b}\n", UHCI_STS_PORT.lock().read());
    info!("\nUHCI USBINTR: {:b}\n", UHCI_INT_PORT.lock().read());
    info!("\nUHCI FRNUM: {:b}\n", frame_number());
    info!("\nUHCI FAME BASE: {:b}\n", UHCI_FRBASEADD_PORT.lock().read());
    info!("\nUHCI SOFMOD: {:b}\n", UHCI_SOFMD_PORT.lock().read());
    info!("\nUHCI PORTSC1: {:b}\n", REG_PORT1.lock().read());
    info!("\nUHCI PORTSC2: {:b}\n", REG_PORT2.lock().read());
    Ok(())
}


/// Allocate a available virtual buffer pointer for building TD
pub fn buffer_pointer_alloc(offset:usize)-> Option<usize> {

    DATA_BUFFER.try().and_then(|buffer| {
        if offset >= 4096 {
            buffer.lock().address_at_offset(offset - 4096)
        } else{
            buffer.lock().address_at_offset(offset)
        }

    })
}


/// Read allocated td's link pointer
pub fn qh_pointers(index:usize)->Option<Result<(u32,u32),&'static str>>{

    QH_POOL.try().map(|qh_pool| {

        let qh = &mut qh_pool.lock()[index];
        Ok((qh.vertical_pointer.read(),qh.horizontal_pointer.read()))

    })
}

/// Allocate a available Uhci Queue Head
/// Return the available Queue Head's physical address and index in the pool
pub fn qh_alloc()-> Option<Result<(usize,usize),&'static str>>{

    QH_POOL.try().map(|qh_pool| {


        let mut index:usize = 0;
        for x in qh_pool.lock().iter_mut(){

            if x.active.read() == 0{

                x.active.write(1);

                let add: *mut UhciQH = x;


                return Ok((add as usize, index));


            }else{
                index += 1;
            }
        }

        warn!("No available Queue head for transfer");
        Err("No available Queue head for transfer")

    })
}

/// According to the given index, init the available queue head
pub fn init_qh(index:usize,horizontal_pointer:u32,element_pointer:u32){

    QH_POOL.try().map(|qh_pool| {

        let qh = &mut qh_pool.lock()[index];
        qh.horizontal_pointer.write(horizontal_pointer);
        qh.vertical_pointer.write(element_pointer);

    });
}

/// Register the UHCI's usb device
pub fn device_register(index:usize, device: UsbDevice){

    UHCI_DEVICE_POOL.try().map(|device_pool| {

        let d = &mut device_pool.lock()[index];
        d.port = device.port;
        d.interrupt_endpoint = device.interrupt_endpoint;
        d.iso_endpoint = device.iso_endpoint;
        d.control_endpoint = device.control_endpoint;
        d.device_type = device.device_type;
        d.addr = device.addr;
        d.maxpacketsize = device.maxpacketsize;
        d.speed = device.speed;
        d.controller = device.controller;
    });
}


/// Allocate a available Uhci TD
pub fn td_alloc()-> Option<Result<(usize,usize),&'static str>>{

    TD_POOL.try().map(|td_pool| {

        let mut index:usize = 0;
        for x in td_pool.lock().iter_mut(){

            if x.active.read() == 0{

                x.active.write(1);

                let add: *mut UhciTDRegisters = x;

                return Ok((add as usize,index));

            }else{

                index += 1;
            }
        }

        Err("No available Queue head for transfer")

    })
}

/// According to the given index, init the available TD
pub fn init_td(index:usize,type_select: u8, pointer: u32, speed: u8, add: u32, endp: u32, toggle: u32, pid: u32,
               data_size: u32, data_add: u32){

    TD_POOL.try().map(|td_pool| {

        let td = &mut td_pool.lock()[index];
        td.init(type_select, pointer, speed, add, endp, toggle, pid,
                data_size, data_add)

    });
}

/// Write next data structure's physical address into given TD according to the index
/// Param: index: index of the allocated TD; type_select: 1 -> Queue Head, 0 -> TD;
/// pointer : the physical address to be linked
pub fn td_link(index:usize, type_select: u8, pointer: u32){

    TD_POOL.try().map(|td_pool| {

        let td = &mut td_pool.lock()[index];
        if type_select == 1{
            td.link_pointer.write(pointer|TD_PTR_QH);
        }else{
            td.link_pointer.write(pointer);
        }

    });
}

/// Write next data structure's physical address into given TD according to the index
/// Let the TD indicate vertical first.
pub fn td_link_vf(index:usize, type_select: u8, pointer: u32){

    let vf_pointer = pointer | TD_PTR_DEPTH;
    td_link(index, type_select, vf_pointer)
}

/// Read allocated td's link pointer
pub fn td_link_pointer(index:usize)->Option<Result<u32,&'static str>>{

    TD_POOL.try().map(|td_pool| {

        let td = &mut td_pool.lock()[index];
        Ok(td.link_pointer.read())



    })
}

/// Read allocated td's link pointer
pub fn td_token(index:usize)->Option<Result<u32,&'static str>>{

    TD_POOL.try().map(|td_pool| {

        let td = &mut td_pool.lock()[index];
        Ok(td.token.read())

    })
}

/// Read allocated td's link pointer
pub fn td_status(index:usize)->Option<Result<u32,&'static str>>{

    TD_POOL.try().map(|td_pool| {

        let td = &mut td_pool.lock()[index];
        let status = td.control_status.read();
        Ok(status)

    })
}

/// Clean the UHCI framelist's contents, set it to default
pub fn clean_framelist(){

    UHCI_FRAME_LIST.try().map(|frame_list|{

        let mut index:usize = 0;
        for x in frame_list.lock().iter_mut() {

            x.write(1);

        }
    });
}

///Clean a frame in framelist
pub fn clean_a_frame(index: usize){

    UHCI_FRAME_LIST.try().map(|frame_list|{


        let frame =  &mut frame_list.lock()[index];
        frame.write(0);
    });

}

/// Link the TD and Queue Head to the frame list, and return the index of the frame
pub fn qh_link_to_framelist(pointer: u32) -> Option<Result<usize,&'static str>>{

    UHCI_FRAME_LIST.try().map(|frame_list|{

        let mut index:usize = 0;
        for x in frame_list.lock().iter_mut() {

            if (x.read() == 0) || (x.read() & 0x1 == 0x1) {

                x.write(pointer | TD_PTR_QH);

                return Ok(index);

            }else{
                index += 1;
            }

        }

        Err("No empty frame, need to clean one")
        })
}

///read frame list link pointer
pub fn frame_link_pointer(index: usize) -> Option<Result<u32,&'static str>>{

    UHCI_FRAME_LIST.try().map(|frame_list|{

        let pointer = frame_list.lock()[index].read();
        Ok(pointer)
    })
}


//-------------------------------------------------------------------------------------------------
const MAX_QH:usize=                          16;
const MAX_TD:usize=                          64;


pub fn box_qh_pool(active_table: &mut ActivePageTable)
                   -> Result<BoxRefMut<MappedPages, [UhciQH; MAX_QH]>, &'static str>{


    let qh_pool: BoxRefMut<MappedPages, [UhciQH; MAX_QH]>  = BoxRefMut::new(Box::new(map_pool(active_table)?))
        .try_map_mut(|mp| mp.as_type_mut::<[UhciQH; MAX_QH]>(0))?;



    Ok(qh_pool)
}
pub fn box_td_pool(active_table: &mut ActivePageTable)
                   -> Result<BoxRefMut<MappedPages, [UhciTDRegisters; MAX_TD]>, &'static str>{


    let td_pool: BoxRefMut<MappedPages, [UhciTDRegisters; MAX_TD]>  = BoxRefMut::new(Box::new(map_pool(active_table)?))
        .try_map_mut(|mp| mp.as_type_mut::<[UhciTDRegisters; MAX_TD]>(0))?;

    Ok(td_pool)
}

pub fn box_device_pool(active_table: &mut ActivePageTable)
                       -> Result<BoxRefMut<MappedPages, [UsbDevice; 2]>, &'static str>{


    let device_pool: BoxRefMut<MappedPages, [UsbDevice; 2]>  = BoxRefMut::new(Box::new(map_pool(active_table)?))
        .try_map_mut(|mp| mp.as_type_mut::<[UsbDevice; 2]>(0))?;

    Ok(device_pool)
}

/// Box the the frame list
pub fn box_frame_list(active_table: &mut ActivePageTable, frame_base: PhysicalAddress)
                      -> Result<BoxRefMut<MappedPages, [Volatile<u32>;1024]>, &'static str>{


    let frame_pointer: BoxRefMut<MappedPages, [Volatile<u32>;1024]>  = BoxRefMut::new(Box::new(map(active_table,frame_base)?))
        .try_map_mut(|mp| mp.as_type_mut::<[Volatile<u32>;1024]>(0))?;


    Ok(frame_pointer)
}

///Get a physical memory page for data
pub fn map_pool(active_table: &mut ActivePageTable) -> Result<MappedPages, &'static str> {

    let frame = allocate_frame().unwrap();
    let new_page = try!(allocate_pages(1).ok_or("out of virtual address space for EHCI Capability Registers)!"));
    let frames = Frame::range_inclusive(frame.clone(), frame.clone());
    let mut fa = try!(FRAME_ALLOCATOR.try().ok_or("EHCI::init(): couldn't get FRAME_ALLOCATOR")).lock();
    let mapped_page = try!(active_table.map_allocated_pages_to(
        new_page,
        frames,
        EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE,
        fa.deref_mut())
    );

    Ok(mapped_page)
}

/// Box the device standard request
pub fn box_dev_req(active_table: &mut ActivePageTable,phys_addr: PhysicalAddress,offset: PhysicalAddress)
                   -> Result<BoxRefMut<MappedPages, UsbDevReq>, &'static str> {
    let page = map(active_table,phys_addr)?;
    let dev_req: BoxRefMut<MappedPages, UsbDevReq> = BoxRefMut::new(Box::new(page))
        .try_map_mut(|mp| mp.as_type_mut::<UsbDevReq>(offset))?;

    Ok(dev_req)
}

/// Box the endpoint description
pub fn box_endpoint_desc(active_table: &mut ActivePageTable,phys_addr: PhysicalAddress,offset: PhysicalAddress)
                      -> Result<BoxRefMut<MappedPages, UsbEndpDesc>, &'static str>{

    let page = map(active_table,phys_addr)?;
    let endpoint_desc: BoxRefMut<MappedPages, UsbEndpDesc>  = BoxRefMut::new(Box::new(page))
        .try_map_mut(|mp| mp.as_type_mut::<UsbEndpDesc>(offset))?;

    Ok(endpoint_desc)
}

/// Box the interface description
pub fn box_inter_desc(active_table: &mut ActivePageTable,phys_addr: PhysicalAddress,offset: PhysicalAddress)
                      -> Result<BoxRefMut<MappedPages, UsbIntfDesc>, &'static str>{

    let page = map(active_table,phys_addr)?;
    let inter_desc: BoxRefMut<MappedPages, UsbIntfDesc>  = BoxRefMut::new(Box::new(page))
        .try_map_mut(|mp| mp.as_type_mut::<UsbIntfDesc>(offset))?;

    Ok(inter_desc)
}
/// Box the device config description
pub fn box_config_desc(active_table: &mut ActivePageTable,phys_addr: PhysicalAddress,offset: PhysicalAddress)
                       -> Result<BoxRefMut<MappedPages, UsbConfDesc>, &'static str>{

    let page = map(active_table,phys_addr)?;
    let config_desc: BoxRefMut<MappedPages, UsbConfDesc>  = BoxRefMut::new(Box::new(page))
        .try_map_mut(|mp| mp.as_type_mut::<UsbConfDesc>(offset))?;

    Ok(config_desc)
}

/// Box the device description
pub fn box_device_desc(active_table: &mut ActivePageTable,phys_addr: PhysicalAddress,offset: PhysicalAddress)
                       -> Result<BoxRefMut<MappedPages, UsbDeviceDesc>, &'static str>{

    let page = map(active_table,phys_addr)?;
    let config_desc: BoxRefMut<MappedPages, UsbDeviceDesc>  = BoxRefMut::new(Box::new(page))
        .try_map_mut(|mp| mp.as_type_mut::<UsbDeviceDesc>(offset))?;

    Ok(config_desc)
}



/// return a mapped page of given physical addrsss
pub fn map(active_table: &mut ActivePageTable, phys_addr: PhysicalAddress) -> Result<MappedPages, &'static str> {

    let new_page = try!(allocate_pages(1).ok_or("out of virtual address space for EHCI Capability Registers)!"));
    let frames = Frame::range_inclusive(Frame::containing_address(phys_addr), Frame::containing_address(phys_addr));
    let mut fa = try!(FRAME_ALLOCATOR.try().ok_or("EHCI::init(): couldn't get FRAME_ALLOCATOR")).lock();
    let mapped_page = try!(active_table.map_allocated_pages_to(
        new_page,
        frames,
        EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE,
        fa.deref_mut())
    );
    Ok(mapped_page)
}

/// Read the information of the device on the port 1 and config the device
pub fn port1_device_init() -> Result<UsbDevice,&'static str>{
    if if_connect_port1(){
        if if_enable_port1(){
            let speed:u8;
            if low_speed_attach_port1(){
                speed = USB_LOW_SPEED;
            }else{
                speed = USB_FULL_SPEED;
            }

            return Ok(UsbDevice::new(1,speed,0,0,Controller::UCHI,
                                     HIDType::Unknown,0,0,0));
        }
        info!("Port 1 is not enabled");
        return Err("Port 1 is not enabled");
    }
    info!("No device is connected to the port 1");
    Err("No device is connected to the port 1")


}

/// Read the information of the device on the port 1 and config the device
pub fn port2_device_init() -> Result<UsbDevice,&'static str>{
    if if_connect_port2(){
        if if_enable_port2(){
            let speed:u8;
            if low_speed_attach_port2(){
                speed = USB_LOW_SPEED;
            }else{
                speed = USB_FULL_SPEED;
            }

            return Ok(UsbDevice::new(2,speed,0,0,Controller::UCHI,
                                     HIDType::Unknown,0,0,0, ));
        }
        return Err("Port 2 is not enabled");
    }
    Err("No device is connected to the port 2")

}

/// Read the SOF timing value
/// please read the intel doc for value decode
///  64 stands for 1 ms Frame Period (default value)
pub fn get_sof_timing() -> u16{

    UHCI_SOFMD_PORT.lock().read() & 0xEF

}







// ------------------------------------------------------------------------------------------------
// UHCI Command Register

const CMD_RS: u16 =                     (1 << 0);    // Run/Stop
const CMD_HCRESET: u16 =                (1 << 1);    // Host Controller Reset
const CMD_GRESET: u16 =                 (1 << 2);    // Global Reset
const CMD_EGSM: u16 =                   (1 << 3);    // Enter Global Suspend Resume
const CMD_FGR: u16 =                    (1 << 4);    // Force Global Resume
const CMD_SWDBG: u16 =                  (1 << 5);    // Software Debug
const CMD_CF: u16 =                     (1 << 6);    // Configure Flag
const CMD_MAXP: u16 =                   (1 << 7);    // Max Packet (0 = 32, 1 = 64)

// UHCI Command Wrapper Functions
/// Run or Stop the UHCI
/// Param: 1 -> Run; 0 -> Stop
 pub fn run(value: u8){

    if value == 1{

        let command = UHCI_CMD_PORT.lock().read() | CMD_RS;
        unsafe{UHCI_CMD_PORT.lock().write(command);}

    }else if value == 0{

        let command = UHCI_CMD_PORT.lock().read() & (!CMD_RS);
        unsafe{UHCI_CMD_PORT.lock().write(command);}
    }

}

/// Enter the normal mode or debug mode
/// Param: 1 -> Debug Mode; 0 -> Normal Mode
pub fn mode(value: u8) -> Result<(), &'static str>{

    if if_halted(){

        if value == 1{

            let command = UHCI_CMD_PORT.lock().read() | CMD_SWDBG;
            unsafe{UHCI_CMD_PORT.lock().write(command);}

        } else if value == 0{

            let command = UHCI_CMD_PORT.lock().read() & (!CMD_SWDBG);
            unsafe{UHCI_CMD_PORT.lock().write(command);}
        }

        Ok(())
    }else{
        Err("The controller is not halted. Fail to change mode")
    }

}

/// Set the packet size
/// Param: 1 -> 64 bytes; 0 -> 32 bytes
pub fn packet_size(value:u8) -> Result<(), &'static str>{

    if if_halted(){

        if value == 1{

            let command = UHCI_CMD_PORT.lock().read() | CMD_MAXP;
            unsafe{UHCI_CMD_PORT.lock().write(command);}

        } else if value == 0{

            let command = UHCI_CMD_PORT.lock().read() & (!CMD_MAXP);
            unsafe{UHCI_CMD_PORT.lock().write(command);}
        }
        Ok(())
    }else{

        Err("The controller is not halted. Fail to change the packet size")
    }

}

/// End the global resume signaling
pub fn end_global_resume(){

    let command = UHCI_CMD_PORT.lock().read() & (!CMD_FGR);
    unsafe{UHCI_CMD_PORT.lock().write(command);}
}

/// End the global suspend mode
pub fn end_global_suspend() -> Result<(), &'static str>{

    if if_halted(){

        let command = UHCI_CMD_PORT.lock().read() & (!CMD_EGSM);
        unsafe{UHCI_CMD_PORT.lock().write(command);}
        Ok(())
    }else {

        Err("The controller is not halted. Fail to quit global suspend mode")
    }

}

/// Reset the UHCI
pub fn reset(){

    unsafe{UHCI_CMD_PORT.lock().write(CMD_HCRESET);}

}


// ------------------------------------------------------------------------------------------------
// UHCI Interrupt Enable Register

const INTR_TIMEOUT: u16 =                    (1 << 0);    // Timeout/CRC Interrupt Enable
const INTR_RESUME: u16 =                     (1 << 1);    // Resume Interrupt Enable
const INTR_IOC: u16 =                        (1 << 2);    // Interrupt on Complete Enable
const INTR_SP: u16 =                         (1 << 3);    // Short Packet Interrupt Enable

// UHCI Interrupt Wrapper Function


/// Enable / Disable the short packet interrupt
/// Param: 1 -> enable; 0 -> disable
pub fn short_packet_int(value: u8){

    if value == 1{

        let command = UHCI_INT_PORT.lock().read() | INTR_SP;
        unsafe{UHCI_INT_PORT.lock().write(command);}

    } else if value == 0{

        let command = UHCI_INT_PORT.lock().read() & (!INTR_SP);
        unsafe{UHCI_INT_PORT.lock().write(command);}
    }


}

/// Enable / Disable the Resume Interrupt
/// Param: 1 -> enable; 0 -> disable
pub fn resume_int(value: u8){

    if value == 1{

        let command = UHCI_INT_PORT.lock().read() | INTR_RESUME;
        unsafe{UHCI_INT_PORT.lock().write(command);}

    } else if value == 0{

        let command = UHCI_INT_PORT.lock().read() & (!INTR_RESUME);
        unsafe{UHCI_INT_PORT.lock().write(command);}
    }
}

/// Enable / Disable the Interrupt On Complete
/// Param: 1 -> enable; 0 -> disable
pub fn ioc_int(value: u8){

    if value == 1{

        let command = UHCI_INT_PORT.lock().read() | INTR_IOC;
        unsafe{UHCI_INT_PORT.lock().write(command);}

    } else if value == 0{

        let command = UHCI_INT_PORT.lock().read() & (!INTR_IOC);
        unsafe{UHCI_INT_PORT.lock().write(command);}
    }
}

/// Enable / Disable the Interrupt On Timeout/CRC
/// Param: 1 -> enable; 0 -> disable
pub fn tcrc_int(value: u8){

    if value == 1{

        let command = UHCI_INT_PORT.lock().read() | INTR_TIMEOUT;
        unsafe{UHCI_INT_PORT.lock().write(command);}

    } else if value == 0{

        let command = UHCI_INT_PORT.lock().read() & (!INTR_TIMEOUT);
        unsafe{UHCI_INT_PORT.lock().write(command);}
    }
}

// ------------------------------------------------------------------------------------------------
// UHCI Status Register

const STS_USBINT: u16 =                      (1 << 0);    // USB Interrupt
const STS_ERROR: u16 =                       (1 << 1);    // USB Error Interrupt
const STS_RD: u16 =                          (1 << 2);    // Resume Detect
const STS_HSE: u16 =                         (1 << 3);    // Host System Error
const STS_HCPE: u16 =                        (1 << 4);    // Host Controller Process Error
const STS_HCH: u16 =                         (1 << 5);    // HC Halted

// UHCI Status Rehister wrapper function

/// See whether the UHCI is Halted
/// Return a bool
pub fn if_halted() -> bool{

    let flag = (UHCI_STS_PORT.lock().read() & STS_HCH) == STS_HCH;
    flag
}

/// See whether UHCI has serious error occurs during a host system access
/// Return a bool
pub fn if_process_error() -> bool{

    let flag = (UHCI_STS_PORT.lock().read() & STS_HSE) == STS_HSE;
    flag

}

/// See whether UHCI t receives a “RESUME” signal from a USB device
/// Return a bool
pub fn resume_detect() -> bool{

    let flag = (UHCI_STS_PORT.lock().read() & STS_RD) == STS_RD;
    flag

}

/// See whether completion of a USB transaction results in an error condition
/// Return a bool
pub fn if_error_int() -> bool{

    let flag = (UHCI_STS_PORT.lock().read() & STS_ERROR) == STS_ERROR;
    flag

}

/// See whether an interrupt is a completion of a transaction
/// Return a bool
pub fn if_interrupt() -> bool{

    let flag = (UHCI_STS_PORT.lock().read() & STS_USBINT) == STS_USBINT;
    flag

}
// ------------------------------------------------------------------------------------------------

// ------------------------------------------------------------------------------------------------
// Port Status and Control Registers

const PORT_CONNECTION: u16 =                 (1 << 0);    // Current Connect Status
const PORT_CONNECTION_CHANGE: u16 =          (1 << 1);    // Connect Status Change
const PORT_ENABLE: u16 =                     (1 << 2);    // Port Enabled
const PORT_ENABLE_CHANGE: u16 =              (1 << 3);    // Port Enable Change
const PORT_LS: u16 =                         (3 << 4);    // Line Status
const PORT_RD: u16 =                         (1 << 6);    // Resume Detect
const PORT_LSDA: u16 =                       (1 << 8);    // Low Speed Device Attached
const PORT_RESET: u16 =                      (1 << 9);    // Port Reset
const PORT_SUSP: u16 =                       (1 << 12);   // Suspend
const PORT_RWC: u16 =                        (PORT_CONNECTION_CHANGE | PORT_ENABLE_CHANGE);

// Port Status Wrapper functions



/// See whether the port 1 is in suspend state
/// Return a bool
pub fn if_port1_suspend() -> bool{


    let flag = (REG_PORT1.lock().read() & PORT_SUSP) != 0;
    flag

}

/// Suspend or Activate the port
/// value: 1 -> suspend, 0 -> activate
pub fn port1_suspend(value: u8){

    let bits = REG_PORT1.lock().read();
    if value == 1{
        unsafe{
            REG_PORT1.lock().write(bits | PORT_SUSP);
        }
    } else if value == 0{

        unsafe{
            REG_PORT1.lock().write(bits & (!PORT_SUSP));
        }
    }

}

/// See whether the port 1 is in reset state
/// Param:
/// Return a bool
pub fn if_port1_reset() -> bool{


    let flag = (REG_PORT1.lock().read() & PORT_RESET) != 0;
    flag


}

/// Reset the port 1
pub fn port1_reset() {

    let reset_command = REG_PORT1.lock().read() | PORT_RESET;
    unsafe { REG_PORT1.lock().write(reset_command); }

    //use better way to delay, need 60 ms
    for _x in 0..300{}

    let reset_command = REG_PORT1.lock().read() & (!PORT_RESET);
    unsafe { REG_PORT1.lock().write(reset_command); }

    for _x in 0..20{

        let port_status = REG_PORT1.lock().read();

        if if_connect_port1(){
            port1_enable(1);
            info!("UHCI port 1 reset complete, the port is ready to use for device");
            return;
        }

        if connect_change_port1(){
            connect_change_clear_port1();
            info!("UHCI port 1 connect status changed after port reset");
            continue;
        }

        if enable_change_port1(){
            enable_change_clear_port1();
            info!("UHCI port 1 enable status changed after port reset");
            continue;
        }
    }

    info!("UHCI port 1 reset complete, no device is attached");
}



/// See whether low speed device attached to port 1
/// Return a bool
pub fn low_speed_attach_port1() -> bool{

    let flag = (REG_PORT1.lock().read() & PORT_LSDA) != 0;
    flag


}

/// See whether Port enbale/disable state changes
/// Param:
/// Return a bool
pub fn enable_change_port1() -> bool{


    let flag = (REG_PORT1.lock().read() & PORT_ENABLE_CHANGE) != 0;
    flag


}

/// Clear Enable Change bit of port 1
pub fn enable_change_clear_port1() {

        unsafe { REG_PORT1.lock().write(PORT_ENABLE_CHANGE); }
}


/// See whether the port 1 is in enable state
/// Return a bool
pub fn if_enable_port1() -> bool{


    let flag = (REG_PORT1.lock().read() & PORT_ENABLE) != 0;
    flag


}

/// Enable or Disable the port 1
/// value: 1 -> enable; 0 -> disable
pub fn port1_enable(value: u8) {

    let bits = REG_PORT1.lock().read();
    if value == 1{
        unsafe{
            REG_PORT1.lock().write(bits | PORT_ENABLE);
        }
    } else if value == 0{
        unsafe{
            REG_PORT1.lock().write(bits & (!PORT_ENABLE));
        }
    }

}

/// See whether Port 1 connect state changes
/// Return a bool
pub fn connect_change_port1() -> bool{


    let flag = (REG_PORT1.lock().read() & PORT_CONNECTION_CHANGE) != 0;
    flag


}

/// Clear Connect Change bit in port 1
pub fn connect_change_clear_port1() {


    unsafe { REG_PORT1.lock().write(PORT_CONNECTION_CHANGE); }

}

/// See whether a device is connected to this port
pub fn if_connect_port1() -> bool{

    let flag = (REG_PORT1.lock().read() & PORT_CONNECTION) != 0;
    flag


}

/// See whether the port 2 is in suspend state
/// Return a bool
pub fn if_port2_suspend() -> bool{


    let flag = (REG_PORT2.lock().read() & PORT_SUSP) != 0;
    flag

}

/// Suspend or Activate the port
/// value: 1 -> suspend, 0 -> activate
pub fn port2_suspend(value: u8){

    let bits = REG_PORT2.lock().read();
    if value == 1{
        unsafe{
            REG_PORT2.lock().write(bits | PORT_SUSP);
        }
    } else if value == 0{

        unsafe{
            REG_PORT2.lock().write(bits & (!PORT_SUSP));
        }
    }

}

/// See whether the port 2 is in reset state
/// Param:
/// Return a bool
pub fn if_port2_reset() -> bool{


    let flag = (REG_PORT2.lock().read() & PORT_RESET) != 0;
    flag


}

/// Reset the port 2
pub fn port2_reset() {
    let reset_command = REG_PORT2.lock().read() | PORT_RESET;
    unsafe { REG_PORT2.lock().write(reset_command); }

    //use better way to delay, need 60 ms
    for _x in 0..300 {}

    let reset_command = REG_PORT2.lock().read() & (!PORT_RESET);
    unsafe { REG_PORT2.lock().write(reset_command); }

    for _x in 0..20 {
        let port_status = REG_PORT2.lock().read();

        if if_connect_port2() {
            port2_enable(1);
            info!("UHCI port 2 reset complete, the port is ready to use for device");
            return;
        }

        if connect_change_port2() {
            connect_change_clear_port2();
            info!("UHCI port 2 connect status changed after port reset");
            continue;
        }

        if enable_change_port2() {
            enable_change_clear_port2();
            info!("UHCI port 2 enable status changed after port reset");
            continue;
        }

    }

    info!("UHCI port 2 reset complete, no device is attached");
}

/// See whether low speed device attached to port 2
/// Return a bool
pub fn low_speed_attach_port2() -> bool{

    let flag = (REG_PORT2.lock().read() & PORT_LSDA) != 0;
    flag


}

/// See whether Port enbale/disable state changes
/// Param:
/// Return a bool
pub fn enable_change_port2() -> bool{


    let flag = (REG_PORT2.lock().read() & PORT_ENABLE_CHANGE) != 0;
    flag


}

/// Clear Enable Change bit of port 2
pub fn enable_change_clear_port2() {

    unsafe { REG_PORT2.lock().write(PORT_ENABLE_CHANGE); }
}


/// See whether the port 2 is in enable state
/// Return a bool
pub fn if_enable_port2() -> bool{


    let flag = (REG_PORT2.lock().read() & PORT_ENABLE) != 0;
    flag


}

/// Enable or Disable the port 2
/// value: 1 -> enable; 0 -> disable
pub fn port2_enable(value: u8) {

    let bits = REG_PORT2.lock().read();
    if value == 1{
        unsafe{
            REG_PORT2.lock().write(bits | PORT_ENABLE);
        }
    } else if value == 0{
        unsafe{
            REG_PORT2.lock().write(bits & (!PORT_ENABLE));
        }
    }

}

/// See whether port 2 connect state changes
/// Return a bool
pub fn connect_change_port2() -> bool{


    let flag = (REG_PORT2.lock().read() & PORT_CONNECTION_CHANGE) != 0;
    flag


}

/// Clear Connect Change bit in port 2
pub fn connect_change_clear_port2() {


    unsafe { REG_PORT2.lock().write(PORT_CONNECTION_CHANGE); }

}

/// See whether a device is connected to this port
pub fn if_connect_port2() -> bool{

    let flag = (REG_PORT2.lock().read() & PORT_CONNECTION) != 0;
    flag


}

// ------------------------------------------------------------------------------------------------

// ------------------------------------------------------------------------------------------------
// Frame Base Register

/// Read the frame list base address
pub fn frame_list_base() -> u32{

    UHCI_FRBASEADD_PORT.lock().read() & 0xFFFFF000
}

/// Read the current frame number
pub fn frame_number() -> u16{

    UHCI_FRNUM_PORT.lock().read() & 0x3FF
}

/// Read the Frame List current index
/// The return value corresponds to memory address signals [11:2].
pub fn current_index() -> u16{

    let index = (frame_number() & 0x3FF) << 2;
    index
}


/// Assign Frame list base memory address
/// Param: base [11:0] must be 0s
pub fn assign_frame_list_base(base: u32){

    unsafe{UHCI_FRBASEADD_PORT.lock().write(base);}
}

// ------------------------------------------------------------------------------------------------
// Transfer Descriptor

// TD Link Pointer
pub const TD_PTR_TERMINATE:u32=                 (1 << 0);
const TD_PTR_QH :u32=                       (1 << 1);
const TD_PTR_DEPTH  :u32=                   (1 << 2);


// TD Control and Status
const TD_CS_ACTLEN :u32=                    0x000007ff;
const TD_CS_STATUS :u32=                    (0xff << 16);  // Status
const TD_CS_BITSTUFF :u32=                  (1 << 17);     // Bitstuff Error
const TD_CS_CRC_TIMEOUT :u32=               (1 << 18);     // CRC/Time Out Error
const TD_CS_NAK :u32=                       (1 << 19);     // NAK Received
const TD_CS_BABBLE  :u32=                   (1 << 20);     // Babble Detected
const TD_CS_DATABUFFER :u32=                (1 << 21);     // Data Buffer Error
const TD_CS_STALLED :u32=                   (1 << 22);     // Stalled
pub const TD_CS_ACTIVE  :u32=                   (1 << 23);     // Active
const TD_CS_IOC :u32=                       (1 << 24);     // Interrupt on Complete
const TD_CS_IOS :u32=                       (1 << 25);     // Isochronous Select
const TD_CS_LOW_SPEED :u32=                 (1 << 26);     // Low Speed Device
const TD_CS_ERROR_MASK :u32=                (3 << 27);     // Error counter
const TD_CS_ERROR_SHIFT :u8=                 27;           // Error counter write shift
const TD_CS_SPD :u32=                       (1 << 29);     // Short Packet Detect

// TD Token
const TD_TOK_PID_MASK :u32=                 0xff;    // Packet Identification
const TD_TOK_DEVADDR_MASK :u32=             0x7f00;    // Device Address
const TD_TOK_DEVADDR_SHIFT :u8=             8;
const TD_TOK_ENDP_MASK  :u32=               0x78000;    // Endpoint
const TD_TOK_ENDP_SHIFT :u8=                15;
const TD_TOK_D :u32=                        0x80000;    // Data Toggle
const TD_TOK_D_SHIFT :u8=                   19;
const TD_TOK_MAXLEN_MASK  :u32=             0xffe00000;    // Maximum Length
const TD_TOK_MAXLEN_SHIFT :u8=              21;

#[repr(C,packed)]
pub struct UhciTDRegisters
{
    pub link_pointer: Volatile<u32>,
    pub control_status: Volatile<u32>,
    pub token: Volatile<u32>,
    pub buffer_point: Volatile<u32>,
    pub active:Volatile<u32>,
    _padding_1: [u32;3],
}

impl UhciTDRegisters {

    ///Initialize the Transfer description  according to the usb device(function)'s information
    /// Param:
    /// type_select: 1 -> link pointer links to Queue Head, 0 -> links to TD
    /// pointer: link pointer
    /// speed: device's speed; add: device assigned address by host controller
    /// endp: endpoinet number of this transfer pipe of this device
    /// toggle: Data Toggle. This bit is used to synchronize data transfers between a USB endpoint and the host
    /// pid:  This field contains the Packet ID to be used for this transaction. Only the IN (69h), OUT (E1h),
    /// and SETUP (2Dh) tokens are allowed.  Bits [3:0] are complements of bits [7:4].
    /// len: The Maximum Length field specifies the maximum number of data bytes allowed for the transfer.
    /// The Maximum Length value does not include protocol bytes, such as PID and CRC.
    /// data_add: the pointer to data to be transferred
    pub fn init(&mut self, type_select: u8, pointer: u32, speed: u8, add: u32, endp: u32, toggle: u32, pid: u32,
                data_size: u32, data_add: u32){
        let size:u32;
        if data_size == 0{
            size = data_size;
        }else {
            size = data_size - 1;
        }

        let token = ((size << TD_TOK_MAXLEN_SHIFT) |
            (toggle << TD_TOK_D_SHIFT) |
            (endp << TD_TOK_ENDP_SHIFT) |
            (add << TD_TOK_DEVADDR_SHIFT) |
            pid) as u32;
        let mut cs = ((3 << TD_CS_ERROR_SHIFT) | TD_CS_ACTIVE) as u32;


        if speed == USB_LOW_SPEED {
                cs |= TD_CS_LOW_SPEED;
            }
        if pointer == 0{
            self.link_pointer.write(TD_PTR_TERMINATE);
        }else{
            if type_select == 1{
                self.link_pointer.write(pointer|TD_PTR_QH);
            }else{
                self.link_pointer.write(pointer);
            }
        }


        self.control_status.write(cs);
;

        self.token.write(token);

        self.buffer_point.write(data_add);

    }

    ///
    /// get the pointer to this TD struct itself
    pub fn get_self_pointer(&mut self) -> *mut UhciTDRegisters {
        let add: *mut UhciTDRegisters = self;
        add
    }

    /// get the horizotal pointer to next data struct
    pub fn next_pointer(&self) -> PhysicalAddress {
        let pointer = self.link_pointer.read() & 0xFFFFFFF0;
        pointer as PhysicalAddress
    }

    /// get the pointer to the data buffer
    pub fn read_buffer_pointer(&self) -> PhysicalAddress {

        let pointer = self.buffer_point.read() as PhysicalAddress;
        pointer
    }


    /// get the endpointer number
    pub fn read_endp(& self) -> u8{

        let num = (self.token.read() & TD_TOK_ENDP_MASK) as u8;
        num
    }

    /// get the device address
    pub fn read_address(& self) -> u8{

        let add = (self.token.read() & TD_TOK_DEVADDR_MASK) as u8;
        add
    }

    ///  get the Packet ID
    ///  Return: ( [7:4] bits of pid, [3:0] bits of pid )
    pub fn read_pid(& self) -> (u8,u8){

        let pid = (self.token.read() & TD_TOK_PID_MASK) as u8;
        (pid & 0xF0, pid & 0xF)

    }


}


// ------------------------------------------------------------------------------------------------
// Queue Head
#[repr(C,packed)]
pub struct UhciQH
{
    pub horizontal_pointer: Volatile<u32>,
    pub vertical_pointer: Volatile<u32>,
    pub active: Volatile<u32>,
    _padding_1: u32

}





