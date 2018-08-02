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





use alloc::string::ToString;
use core::ops::DerefMut;
use volatile::{Volatile, ReadOnly, WriteOnly};
use alloc::boxed::Box;
use alloc::arc::Arc;
use alloc::Vec;
use port_io::Port;
use owning_ref::{BoxRef, BoxRefMut};
use spin::{RwLock, Once, Mutex};
use irq_safety::MutexIrqSafe;
use memory::{MemoryManagementInfo,FRAME_ALLOCATOR,Frame,PageTable, ActivePageTable, PhysicalAddress, VirtualAddress, EntryFlags, MappedPages, allocate_pages};
use usb_device::{UsbDevice,Controller};

static UHCI_CMD_PORT:  Mutex<Port<u16>> = Mutex::new(Port::new(0xC040));
static UHCI_STS_PORT:  Mutex<Port<u16>> = Mutex::new(Port::new(0xC042));
static UHCI_INT_PORT:  Mutex<Port<u16>> = Mutex::new(Port::new(0xC044));
static UHCI_FRNUM_PORT:  Mutex<Port<u16>> = Mutex::new(Port::new(0xC046));
static UHCI_FRBASEADD_PORT:  Mutex<Port<u32>> = Mutex::new(Port::new(0xC048));
static UHCI_SOFMD_PORT:  Mutex<Port<u16>> = Mutex::new(Port::new(0xC04C));
static REG_PORT1:  Mutex<Port<u16>> = Mutex::new(Port::new(0xC050));
static REG_PORT2:  Mutex<Port<u16>> = Mutex::new(Port::new(0xC052));


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
pub fn init() -> Result<(), &'static str> {

    reset();
    if let Err(e) = mode(0) {
        error!("{:?}", e);
    }else{
        info!("The USB 1.1 host conreoller is in the normal mode");
    }

    if let Err(e) = packet_size(1) {
        error!("{:?}", e);
    }else{
        info!("The packet maximum size is 64 bytes")
    }

    assign_frame_list_base( 0x1FFDE000);

    short_packet_int(1);

    ioc_int(1);

    if let Some(connect) = if_connect(1){
        if connect{
            port_enable(1,1);
        }
    }

    if let Some(connect) = if_connect(2){
        if connect{
            port_enable(2,1);
        }
    }

    run(1);
    info!("\nUHCI USBCMD: {:b}\n", UHCI_CMD_PORT.lock().read());
    info!("\nUHCI USBSTS: {:b}\n", UHCI_STS_PORT.lock().read());
    info!("\nUHCI USBINTR: {:b}\n", UHCI_INT_PORT.lock().read());
    info!("\nUHCI FRNUM: {:b}\n", UHCI_FRNUM_PORT.lock().read());
    info!("\nUHCI FAME BASE: {:b}\n", UHCI_FRBASEADD_PORT.lock().read());
    info!("\nUHCI SOFMOD: {:b}\n", UHCI_SOFMD_PORT.lock().read());
    info!("\nUHCI PORTSC1: {:b}\n", REG_PORT1.lock().read());
    info!("\nUHCI PORTSC2: {:b}\n", REG_PORT2.lock().read());
    Ok(())
 }


/// Read the information of the device on the port 1 and config the device
pub fn port1_device_init() -> Result<UsbDevice,&'static str>{
    if if_connect_port1(){
        if if_enable_port1(){
            let mut speed:u8;
            if low_speed_attach_port1(){
                speed = USB_LOW_SPEED;
            }else{
                speed = USB_FULL_SPEED;
            }

            Ok(UsbDevice::new(1,speed,0,0,Controller::UCHI))
        }
        Err("Port 1 is not enabled")
    }
    Err("No device is connected to the port 1")

}

/// Read the information of the device on the port 1 and config the device
pub fn port2_device_init() -> Result<UsbDevice,&'static str>{
    if if_connect_port2(){
        if if_enable_port2(){
            let mut speed:u8;
            if low_speed_attach_port2(){
                speed = USB_LOW_SPEED;
            }else{
                speed = USB_FULL_SPEED;
            }

            Ok(UsbDevice::new(2,speed,0,0,Controller::UCHI))
        }
        Err("Port 2 is not enabled")
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
    if port_num == 1 {
        unsafe { REG_PORT1.lock().write(REG_PORT1.lock().read() & (!PORT_RESET)); }
    }
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


    let flag = (REG_PORT1.lock().read() & PORT_RESET) != 0;
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


    let flag = (REG_port2.lock().read() & PORT_SUSP) != 0;
    flag

}

/// Suspend or Activate the port
/// value: 1 -> suspend, 0 -> activate
pub fn port2_suspend(value: u8){

    let bits = REG_port2.lock().read();
    if value == 1{
        unsafe{
            REG_port2.lock().write(bits | PORT_SUSP);
        }
    } else if value == 0{

        unsafe{
            REG_port2.lock().write(bits & (!PORT_SUSP));
        }
    }

}

/// See whether the port 2 is in reset state
/// Param:
/// Return a bool
pub fn if_port2_reset() -> bool{


    let flag = (REG_port2.lock().read() & PORT_RESET) != 0;
    flag


}

/// Reset the port 2
pub fn port2_reset() {
    if port_num == 1 {
        unsafe { REG_port2.lock().write(REG_port2.lock().read() & (!PORT_RESET)); }
    }
}

/// See whether low speed device attached to port 2
/// Return a bool
pub fn low_speed_attach_port2() -> bool{

    let flag = (REG_port2.lock().read() & PORT_LSDA) != 0;
    flag


}

/// See whether Port enbale/disable state changes
/// Param:
/// Return a bool
pub fn enable_change_port2() -> bool{


    let flag = (REG_port2.lock().read() & PORT_ENABLE_CHANGE) != 0;
    flag


}

/// Clear Enable Change bit of port 2
pub fn enable_change_clear_port2() {

    unsafe { REG_port2.lock().write(PORT_ENABLE_CHANGE); }
}


/// See whether the port 2 is in enable state
/// Return a bool
pub fn if_enable_port2() -> bool{


    let flag = (REG_port2.lock().read() & PORT_RESET) != 0;
    flag


}

/// Enable or Disable the port 2
/// value: 1 -> enable; 0 -> disable
pub fn port2_enable(value: u8) {

    let bits = REG_port2.lock().read();
    if value == 1{
        unsafe{
            REG_port2.lock().write(bits | PORT_ENABLE);
        }
    } else if value == 0{
        unsafe{
            REG_port2.lock().write(bits & (!PORT_ENABLE));
        }
    }

}

/// See whether port 2 connect state changes
/// Return a bool
pub fn connect_change_port2() -> bool{


    let flag = (REG_port2.lock().read() & PORT_CONNECTION_CHANGE) != 0;
    flag


}

/// Clear Connect Change bit in port 2
pub fn connect_change_clear_port2() {


    unsafe { REG_port2.lock().write(PORT_CONNECTION_CHANGE); }

}

/// See whether a device is connected to this port
pub fn if_connect_port2() -> bool{

    let flag = (REG_port2.lock().read() & PORT_CONNECTION) != 0;
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

    UHCI_FRNUM_PORT.lock().read() & 0xEFF
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
const TD_PTR_TERMINATE:u32=                 (1 << 0);
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
const TD_CS_ACTIVE  :u32=                   (1 << 23);     // Active
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

const TD_PACKET_IN :u32=                    0x69;
const TD_PACKET_OUT :u32=                   0xe1;
const TD_PACKET_SETUP :u32=                 0x2d;

pub struct UhciTDRegisters
{
    pub link_pointer: Volatile<u32>,
    pub control_status: Volatile<u32>,
    pub token: Volatile<u32>,
    pub buffer_point: Volatile<u32>,
}

impl UhciTDRegisters {


    ///Initialize the Transfer description  according to the usb device(function)'s information
    /// Param:
    /// speed: device's speed; add: device assigned address by host controller
    /// endp: endpoinet number of this transfer pipe of this device
    /// toggle: Data Toggle. This bit is used to synchronize data transfers between a USB endpoint and the host
    /// pid:  This field contains the Packet ID to be used for this transaction. Only the IN (69h), OUT (E1h),
    /// and SETUP (2Dh) tokens are allowed.  Bits [3:0] are complements of bits [7:4].
    /// len: The Maximum Length field specifies the maximum number of data bytes allowed for the transfer.
    /// The Maximum Length value does not include protocol bytes, such as PID and CRC.
    /// data_add: the pointer to data to be transferred
    pub fn init(speed: u32, add: u32, endp: u32, toggle: u32, pid: u32,
                len: u32, data_add: u32) -> UhciTDRegisters {
        let token = ((len << TD_TOK_MAXLEN_SHIFT) |
            (toggle << TD_TOK_D_SHIFT) |
            (endp << TD_TOK_ENDP_SHIFT) |
            (add << TD_TOK_DEVADDR_SHIFT) |
            pid) as u32;

        let cs = ((3 << TD_CS_ERROR_SHIFT) | TD_CS_ACTIVE) as u32;
        UhciTDRegisters {
            link_pointer: Volatile::new(TD_PTR_TERMINATE),
            control_status: Volatile::new(cs),
            token: Volatile::new(token),
            buffer_point: Volatile::new(data_add),

        }
    }

    /// get the pointer to this TD struct itself
    pub fn get_self_pointer(&mut self) -> *mut UhciTDRegisters {
        let add: *mut UhciTDRegisters = self;
        add
    }

    /// get the horizotal pointer to next data struct
    pub fn next_pointer(&self) -> u32 {
        let pointer = self.link_pointer.read() & 0xFFFFFFF0;
        pointer
    }

    /// get the pointer to the data buffer
    pub fn read_buffer_pointer(&self) -> u32 {

        let pointer = self.buffer_point.read();
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

pub struct UhciQH
{
    pub horizontal_pointer: Volatile<u32>,
    pub vertical_pointer: Volatile<u32>,
}



