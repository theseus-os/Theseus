#![no_std]

#![allow(dead_code)]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate port_io;

use core::fmt;
use alloc::vec::Vec;
use port_io::Port;
use spin::{Once, Mutex};

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

static PCI_CONFIG_ADDRESS_PORT: Mutex<Port<u32>> = Mutex::new( Port::new(CONFIG_ADDRESS));
static PCI_CONFIG_DATA_PORT: Mutex<Port<u32>> = Mutex::new( Port::new(CONFIG_DATA));

// pub static DMA_FINISHED: AtomicBool = AtomicBool::new(true);

// // the ports to the DMA primary and secondary command and status bytes
// pub static DMA_PRIM_COMMAND_BYTE: Once<Mutex<Port<u8>>> = Once::new();
// pub static DMA_PRIM_STATUS_BYTE: Once<Mutex<Port<u8>>> = Once::new();
// pub static DMA_SEC_COMMAND_BYTE: Once<Mutex<Port<u8>>> = Once::new();
// pub static DMA_SEC_STATUS_BYTE: Once<Mutex<Port<u8>>> = Once::new();

// // ports to write prdt address to
// pub static DMA_PRIM_PRDT_ADDR: Once<Mutex<Port<u32>>> = Once::new();
// pub static DMA_SEC_PRDT_ADDR: Once<Mutex<Port<u32>>> = Once::new();


pub static PCI_BUSES: Once<Vec<PciBus>> = Once::new();

fn get_pci_buses() -> &'static Vec<PciBus> {
    PCI_BUSES.call_once( || { scan_pci() } )
}



/// Returns a reference to the `PciDevice` with the given bus, slot, func identifier.
pub fn get_pci_device_bsf(bus: u16, slot: u16, func: u16) -> Option<&'static PciDevice> {
    for b in get_pci_buses() {
        for d in &b.devices {
            if d.bus == bus && d.slot == slot && d.func == func {
                return Some(&d);
            }
        }
    }
    
    None
}

/// Returns a reference to the `PciDevice` with the given vendor id and device id... Ramla
pub fn get_pci_device_vd(vendor_id: u16, device_id: u16) -> Option<&'static PciDevice> {
    for b in get_pci_buses() {
        for d in &b.devices {
            if d.vendor_id == vendor_id && d.device_id == device_id {
                return Some(&d);
            }
        }
    }
    
    None
}

/// Returns an iterator that iterates over all `PciDevice`s,
/// and, if uninitialized, it initializes the PCI bus & scans it to enumerates devices.
pub fn pci_device_iter() -> impl Iterator<Item = &'static PciDevice> {
    get_pci_buses().iter().flat_map(|b| b.devices.iter())
}



/// Compute a PCI address from bus, slot, func, and offset. 
/// The least two significant bits of offset are masked, so it's 4-byte aligned addressing,
/// which makes sense since we read PCI config data in u32 chunks.
#[inline(always)]
fn pci_address(bus: u16, slot: u16, func: u16, offset: u16) -> u32 {
    ((bus as u32) << 16) | 
    ((slot as u32) << 11) | 
    ((func as u32) << 8) | 
    ((offset as u32) & (PCI_CONFIG_ADDRESS_OFFSET_MASK as u32)) | 
    0x8000_0000
}


/// read 32-bit data at the specified `offset` from the PCI device specified by the given `bus`, `slot`, `func` set.  Ramla
pub fn pci_read_32(bus: u16, slot: u16, func: u16, offset: u16) -> u32 {
    unsafe { 
        PCI_CONFIG_ADDRESS_PORT.lock().write(pci_address(bus, slot, func, offset)); 
    }
    PCI_CONFIG_DATA_PORT.lock().read() >> ((offset & (!PCI_CONFIG_ADDRESS_OFFSET_MASK)) * 8)
}

/// read 16-bit data at the specified `offset` from the PCI device specified by the given `bus`, `slot`, `func` set. 
pub fn pci_read_16(bus: u16, slot: u16, func: u16, offset: u16) -> u16 {
    pci_read_32(bus, slot, func, offset) as u16
} 

/// read 8-bit data at the specified `offset` from the PCI device specified by the given `bus`, `slot`, `func` set. 
pub fn pci_read_8(bus: u16, slot: u16, func: u16, offset: u16) -> u8 {
    pci_read_32(bus, slot, func, offset) as u8
}

/// write data to the specified `offset` on the PCI device specified by the given `bus`, `slot`, `func` set. 
pub fn pci_write(bus: u16, slot: u16, func: u16, offset: u16, value: u32) {
    unsafe { 
        PCI_CONFIG_ADDRESS_PORT.lock().write(pci_address(bus, slot, func, offset)); 
        PCI_CONFIG_DATA_PORT.lock().write((value) << ((offset & 2) * 8));
    }
}

/// sets the PCI device's bit 3 in the Command portion, which is apparently needed to activate DMA (??)
pub fn pci_set_command_bus_master_bit(pci_dev: &PciDevice) {
    unsafe { 
        let PciDevice { bus, slot, func, ..} = pci_dev;
        PCI_CONFIG_ADDRESS_PORT.lock().write(pci_address(*bus, *slot, *func, PCI_COMMAND));
        let inval = PCI_CONFIG_DATA_PORT.lock().read(); 
        trace!("pci_set_command_bus_master_bit: PciDevice: B:{} S:{} F:{} read value: {:#x}", 
                bus, slot, func, inval);
        PCI_CONFIG_DATA_PORT.lock().write(inval | (1 << 2));
        trace!("pci_set_command_bus_master_bit: PciDevice: B:{} S:{} F:{} read value AFTER WRITE CMD: {:#x}", 
                bus, slot, func, PCI_CONFIG_DATA_PORT.lock().read());
    }
}

/// sets the PCI device's command bit 10 to disable legacy interrupts
pub fn pci_set_interrupt_disable_bit(bus: u16, slot: u16, func: u16) {
    unsafe { 
        PCI_CONFIG_ADDRESS_PORT.lock().write(pci_address(bus, slot, func, PCI_COMMAND));
        let inval = PCI_CONFIG_DATA_PORT.lock().read(); 
        trace!("pci_set_interrupt_disable_bit: PciDevice: B:{} S:{} F:{} read value: {:#x}", 
                bus, slot, func, inval);
        PCI_CONFIG_DATA_PORT.lock().write(inval | (1 << 10));
        trace!("pci_set_interrupt_disable_bit: PciDevice: B:{} S:{} F:{} read value AFTER WRITE CMD: {:#x}", 
                bus, slot, func, PCI_CONFIG_DATA_PORT.lock().read());
    }
}

/// Explore the pci config space and return address of capability
pub fn pci_config_space(dev_pci: &PciDevice, capability_pci: u16) -> Option<u16> {

    debug!("NIC PCI CONFIG SPACE");

    debug!("PCI command: {:#X}", pci_read_16(dev_pci.bus, dev_pci.slot, dev_pci.func, PCI_COMMAND));

    let status = pci_read_16(dev_pci.bus, dev_pci.slot, dev_pci.func, PCI_STATUS);

    debug!("status: {:#X}", status);

    // capabilities only valid if bit 4 of STATUS reg is set to 1
    if (status >> 4 & 1) == 1 {
        // finding capabilities pointer
        let capabilities = pci_read_8(dev_pci.bus, dev_pci.slot, dev_pci.func, PCI_CAPABILITIES);
        debug!("capabilities pointer: {:#X}", capabilities);

        //botttom two bits of capability pointer need to be masked
        let mut cap_addr = capabilities as u16 & 0xFFFC;
        let mut cap_header = pci_read_16(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr);
        let mut cap_id;

        while cap_addr != 0 {
            cap_id = cap_header & 0xFF;

            // if cap_id == MSIX_CAPABILITY {
            //         let msix_control = pci_read_32(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr);

            //         debug!("MSIX control: {:#X}", msix_control);

            //         let msix_table = pci_read_32(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr + 4);

            //         debug!("MSIX table: {:#X}", msix_table);

            //         let msix_pba = pci_read_32(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr + 8);

            //         debug!("MSIX PBA: {:#X}", msix_pba);

            //         // pci_unset_msix_enable_bit(dev_pci.bus, dev_pci.slot, dev_pci.func, node_next);

            //         // pci_write(dev_pci.bus, dev_pci.slot, dev_pci.func, node_next, msix_control&0x7FFF_FFFF);

            //         // let msix_control = pci_read_32(dev_pci.bus, dev_pci.slot, dev_pci.func, node_next);

            //         // debug!("MSIX control_updated: {:#X}", msix_control);// (msi_control>>16) & 0xFFFF);
            // }
            
            if cap_id == capability_pci {
                    debug!("Found capability: {:#X} at {:#X}", capability_pci, cap_addr);// (msi_control>>16) & 0xFFFF);
                    return Some(cap_addr);
            }

            //find address of next capability
            cap_addr = (cap_header >> 8) & 0xFF;            
            cap_header= pci_read_16(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr as u16);

        }
    }

    None
}

pub fn pci_enable_msi(dev_pci: &PciDevice) -> Result<(), &'static str> {

    let cap_addr = try!(pci_config_space(dev_pci, MSI_CAPABILITY).ok_or("Device not MSI capable"));

    let ctrl = pci_read_32(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr);
    let addr = pci_read_32(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr + 4);
    let data = pci_read_16(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr + 12);

    debug!("MSI HEADER BEFORE ENABLE");
    debug!("MSI CAPABILTIY ID: {:#X}", ctrl & 0xFF);
    debug!("MSI NEXT: {:#X}", ctrl>>8 & 0xFF);
    debug!("MSI CTRL: {:#X}", ctrl>>16 & 0xFFFF);
    debug!("MSI ADDR: {:#X}", addr);
    debug!("MSI DATA: {:#X}", data);

    //write to MSI Addr... Intel Arch SDM, vol3, 10.11, TODO: need to add dest id as an argument
    pci_write(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr+4, 0x0FEE<<20 | 119<<12);
    // pci_write(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr+4, 0x0FEE<<20);

    //write to MSI data, TODO: should add this as a function arg, vector num
    pci_write(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr+12, 0x30);

    //enable MSI in ctrl
    let msi_enable = 1<<16;
    pci_write(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr, ctrl|msi_enable);

    let ctrl = pci_read_32(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr);
    let addr = pci_read_32(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr + 4);
    let data = pci_read_16(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr + 12);


    debug!("MSI HEADER AFTER ENABLE");
    debug!("MSI CAPABILTIY ID: {:#X}", ctrl & 0xFF);
    debug!("MSI NEXT: {:#X}", ctrl>>8 & 0xFF);
    debug!("MSI CTRL: {:#X}", ctrl>>16 & 0xFFFF);
    debug!("MSI ADDR: {:#X}", addr);
    debug!("MSI DATA: {:#X}", data);

    Ok(())  

}

pub fn pci_enable_msix(dev_pci: &PciDevice) -> Result<(), &'static str> {

    let cap_addr = try!(pci_config_space(dev_pci, MSIX_CAPABILITY).ok_or("Device not MSI capable"));

    let message_control_offset = 2;
    let ctrl = pci_read_16(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr + message_control_offset);

    //write to bit 15 of Message Control, (82599 data sheet, section 9.3.8.1)
    let msix_enable = 1 << 15; 
    pci_write(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr + message_control_offset, (ctrl|msix_enable) as u32);

    let ctrl = pci_read_32(dev_pci.bus, dev_pci.slot, dev_pci.func, cap_addr);
    debug!("MSIX HEADER AFTER ENABLE: {:#X}", ctrl);

    Ok(())  

}



// pub fn pci_get_interrupt_status_bit(bus: u16, slot: u16, func: u16) {
//     unsafe { 
//         PCI_CONFIG_ADDRESS_PORT.lock().write(pci_address(bus, slot, func, PCI_COMMAND));
//         let inval = PCI_CONFIG_DATA_PORT.lock().read(); 
//         trace!("pci_set_interrupt_status_bit: PciDevice: B:{} S:{} F:{} read value: {:#x}", 
//                 bus, slot, func, inval);
//         PCI_CONFIG_DATA_PORT.lock().write(inval | (1 << 19));
//         trace!("pci_set_interrupt_status_bit:: PciDevice: B:{} S:{} F:{} read value AFTER WRITE CMD: {:#x}", 
//                 bus, slot, func, PCI_CONFIG_DATA_PORT.lock().read());
//     }
// }

/// struct representing a PCI Bus, containing an array of PCI Devices
#[derive(Debug)]
pub struct PciBus{
    /// the number of this PCI bus
    pub bus_number: u16,
    /// array of devices
    pub devices: Vec<PciDevice>,
}

// the PCI configuration address port drops the least-significant 2 bits
const PCI_CONFIG_ADDRESS_OFFSET_MASK: u16 = 0xFC; 

// see this: http://wiki.osdev.org/PCI#PCI_Device_Structure
const PCI_VENDOR_ID:             u16 = 0x0;
const PCI_DEVICE_ID:             u16 = 0x2;
pub const PCI_COMMAND:               u16 = 0x4;
pub const PCI_STATUS:                u16 = 0x6;
const PCI_REVISION_ID:           u16 = 0x8;
const PCI_PROG_IF:               u16 = 0x9;
const PCI_SUBCLASS:              u16 = 0xA;
const PCI_CLASS:                 u16 = 0xB;
const PCI_CACHE_LINE_SIZE:       u16 = 0xC;
const PCI_LATENCY_TIMER:         u16 = 0xD;
const PCI_HEADER_TYPE:           u16 = 0xE;
const PCI_BIST:                  u16 = 0xF;
pub const PCI_BAR0:                  u16 = 0x10;
const PCI_BAR1:                  u16 = 0x14;
const PCI_BAR2:                  u16 = 0x18;
const PCI_BAR3:                  u16 = 0x1C;
const PCI_BAR4:                  u16 = 0x20;
const PCI_BAR5:                  u16 = 0x24;
const PCI_CARDBUS_CIS:           u16 = 0x28;
const PCI_SUBSYSTEM_VENDOR_ID:   u16 = 0x2C;
const PCI_SUBSYSTEM_ID:          u16 = 0x2E;
const PCI_EXPANSION_ROM_BASE:    u16 = 0x30;
pub const PCI_CAPABILITIES:          u16 = 0x34;
// 0x35 through 0x3B are reserved
pub const PCI_INTERRUPT_LINE:        u16 = 0x3C;
pub const PCI_INTERRUPT_PIN:         u16 = 0x3D;
const PCI_MIN_GRANT:             u16 = 0x3E;
const PCI_MAX_LATENCY:           u16 = 0x3F;

//PCI Capability IDs
pub const MSI_CAPABILITY:       u16 = 0x05;
pub const MSIX_CAPABILITY:       u16 = 0x11;



/// Contains information common to every type of PCI Device
#[derive(Debug)]
pub struct PciDevice {
    pub bus: u16,
    pub slot: u16,
    pub func: u16,

    pub vendor_id: u16,
    pub device_id: u16, 
    pub command: u16,
    pub status: u16,
    pub revision_id: u8,
    pub prof_if: u8,
    /// subclass can also be used to determine device type http://wiki.osdev.org/PCI#Class_Codes 
    pub subclass: u8,
    /// class can be used to determine device type http://wiki.osdev.org/PCI#Class_Codes 
    pub class: u8,
    pub cache_line_size: u8,
    pub latency_timer: u8,
    pub header_type: u8,
    pub bist: u8,
    pub bars: [u32; 6],
    pub int_pin: u8,
    pub int_line: u8,
}

impl fmt::Display for PciDevice { 
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "B:{} S:{} F:{}, vendor_id: {:#x}, device_id: {:#x}, command: {:#x}, status: {:#x}, class: {:#x}, subclass: {:#x}, header_type: {:#x}, bars: {:?}, int_pin: {:#x}, int_line: {:X}", 
            self.bus, self.slot, self.func, self.vendor_id, self.device_id, self.command, self.status, self.class, self.subclass, self.header_type, self.bars, self.int_pin, self.int_line)
    }
}



/// Scans all PCI Buses (brute force iteration) to enumerate PCI Devices on each bus.
/// Initializes structures containing this information. 
fn scan_pci() -> Vec<PciBus> {
	let mut buses: Vec<PciBus> = Vec::new();

    for bus in 0..256 {
        let mut device_list: Vec<PciDevice> = Vec::new();

        for slot in 0..32 {

            for f in 0..8 {
                // get vendor id of device
                let vendor_id = pci_read_16(bus, slot, f, PCI_VENDOR_ID);
                // if vendor ID is 0xFFFF, no device is connected
                if vendor_id == 0xFFFF {
                    continue;
                }
                
                // otherwise, get the subset of common data that describes every PCI Device
                let device = PciDevice {
                    bus: bus,
                    slot: slot,
                    func: f,

                    vendor_id:        vendor_id,
                    device_id:        pci_read_16(bus, slot, f, PCI_DEVICE_ID), 
                    command:          pci_read_16(bus, slot, f, PCI_COMMAND),
                    status:           pci_read_16(bus, slot, f, PCI_STATUS),
                    revision_id:      pci_read_8( bus, slot, f, PCI_REVISION_ID),
                    prof_if:          pci_read_8( bus, slot, f, PCI_PROG_IF),
                    subclass:         pci_read_8( bus, slot, f, PCI_SUBCLASS),
                    class:            pci_read_8( bus, slot, f, PCI_CLASS),
                    cache_line_size:  pci_read_8( bus, slot, f, PCI_CACHE_LINE_SIZE),
                    latency_timer:    pci_read_8( bus, slot, f, PCI_LATENCY_TIMER),
                    header_type:      pci_read_8( bus, slot, f, PCI_HEADER_TYPE),
                    bist:             pci_read_8( bus, slot, f, PCI_BIST),
                    bars:             [ 
                                          pci_read_32(bus, slot, f, PCI_BAR0),
                                          pci_read_32(bus, slot, f, PCI_BAR1), 
                                          pci_read_32(bus, slot, f, PCI_BAR2), 
                                          pci_read_32(bus, slot, f, PCI_BAR3), 
                                          pci_read_32(bus, slot, f, PCI_BAR4), 
                                          pci_read_32(bus, slot, f, PCI_BAR5), 
                                      ],
                    int_pin:          pci_read_8(bus, slot, f, PCI_INTERRUPT_PIN),
                    int_line:         pci_read_8(bus, slot, f, PCI_INTERRUPT_LINE),
                };


                // if device.header_type != 0x00 {
                //     warn!("PCI device has a header_type {:#X}, we do not handle it fully yet! (only support header_type 0x00)", device.header_type);
                // }

                device_list.push(device);
            }
        }

        buses.push( PciBus {
            bus_number: bus as u16, 
            devices: device_list
        });
    }

    buses	
}


/*
///PCI DMA, currently set for primary bus

///sets the ports for PCI DMA configuration access using BAR4 information
pub fn set_dma_ports(){
    pci_set_command_bus_master_bit(0, 1, 1);

    let bar4_raw: u16 = pci_read_32(0, 1, 1, PCI_BAR4);
    debug!("BAR4_RAW 32-bit: {:#x}", bar4_raw);
    // the LSB should be set for Port I/O, see http://wiki.osdev.org/PCI#Base_Address_Registers
    if bar4_raw & 0x1 != 0x1 {
        panic!("set_dma_ports: BAR4 indicated MMIO not Port IO, currently unsupported!");
    }
    // we need to mask the lowest 2 bits of the bar4 address to use it as a port
    let bar4 = (bar4_raw & 0xFFFF_FFFC) as u16 ;
    debug!("set_dma_ports: dma_bar4: {:#x}", bar4);

    //offsets for DMA configuration ports found in http://wiki.osdev.org/ATA/ATAPI_using_DMA under "The Bus Master Register"
    // PCI_COMMAND_PORT.call_once(||Mutex::new(Port::new(bar4 - 0x16))); // TODO FIXME: what is this??

    DMA_PRIM_COMMAND_BYTE.call_once(|| Mutex::new( Port::new(bar4 + 0)));
    DMA_PRIM_STATUS_BYTE.call_once(|| Mutex::new( Port::new(bar4 + 0x2)));
    DMA_PRIM_PRDT_ADDR.call_once(|| Mutex::new( Port::new(bar4 + 0x4)));
    
    DMA_SEC_COMMAND_BYTE.call_once(|| Mutex::new( Port::new(bar4 + 0x8)));
    DMA_SEC_STATUS_BYTE.call_once(|| Mutex::new( Port::new(bar4 + 0xA))); 
    DMA_SEC_PRDT_ADDR.call_once(|| Mutex::new( Port::new(bar4 + 0xC)));

}

/// Creates a prdt table (with just one entry right now) 
/// and sends its physical address to the DMA PRDT address port
pub fn set_prdt(start_add: u32) -> Result<u32, ()>{

    
    let prdt: [u64;1] = [start_add as u64 | 512 << 32 | 1 << 63];
    let prdt_ref = &prdt[0] as *const u64;
    // TODO: first, translate prdt_ref to physicaladdress
    let prdt_paddr = {
        let curr_task = get_my_current_task().unwrap().write();
        let curr_mmi = curr_task.mmi.as_ref().unwrap();
        let mut curr_mmi_locked = curr_mmi.lock();
        curr_mmi_locked.translate(prdt_ref as usize)
    };

    // TODO: then, check that the pdrt phys_addr is Some and that it fits within u32
    if let Some(paddr) = prdt_paddr {
        if paddr < (u32::max_value() as usize) {
            unsafe{
                DMA_PRIM_PRDT_ADDR.try().expect("DMA_PRDT_ADD_LOW not configured").lock().write(paddr as u32 );
                DMA_PRIM_STATUS_BYTE.try().expect("DMA_PRIM_STATUS_BYTE not configured").lock().write(0x04); // to reset the interrupt 0x14 status
            }
            return Ok(paddr as u32);    
        }
    }

    Err(())
 

}


/// Start the actual DMA transfer
pub fn start_transfer(is_write: bool){
    let command_byte: u8 = if is_write { 0x00 } else { 0x08 };
    unsafe {
        // first set bit 3, the read/write direction
        DMA_PRIM_COMMAND_BYTE.try().expect("DMA_PRIM_COMMAND_BYTE not configured").lock().write(command_byte);
        // then we can start the DMA transfer with bit 0, the start/stop bit
        DMA_PRIM_COMMAND_BYTE.try().expect("DMA_PRIM_COMMAND_BYTE not configured").lock().write(command_byte | 0x01);
    }
    // unsafe{PCI_COMMAND_PORT.try().unwrap().lock().write(0b01)};
    
    // temp: reading this bit for debug info
    //sets bit 0 in the status byte to clear error and interrupt bits and set dma mode bit
    trace!("start_transfer() 1: DMA status byte = {:#x}", 
            DMA_PRIM_STATUS_BYTE.try().expect("DMA_PRIM_STATUS_BYTE not configured").lock().read()
    );

    return; 

    //sets bit 0 in the status byte to clear error and interrupt bits and set dma mode bit
    unsafe{DMA_PRIM_STATUS_BYTE.try().expect("DMA_PRIM_STATUS_BYTE not configured").lock().write(0)};

    // temp: reading this bit for debug info
    //sets bit 0 in the status byte to clear error and interrupt bits and set dma mode bit
    unsafe{
        trace!("start_read() 2: DMA status byte = {:#x}", 
                DMA_PRIM_STATUS_BYTE.try().expect("DMA_PRIM_STATUS_BYTE not configured").lock().read()
        );
    }
}

///immediately ends the transfer, dma controller clears any set prdt address 
pub fn cancel_transfer(){
    unsafe{DMA_PRIM_COMMAND_BYTE.try().expect("DMA_PRIM_COMMAND_BYTE not configured").lock().write(0)};

}

///the status byte must be read after each IRQ (I believe IRQ 14 which is handled in ata_pio)
///IRQ number still needs to be confirmed, was 14 according to http://www.pchell.com/hardware/irqs.shtml
pub fn acknowledge_disk_irq(){
    let status = DMA_PRIM_STATUS_BYTE.try().expect("DMA_PRIM_STATUS_BYTE not configured").lock().read();
    unsafe{DMA_PRIM_COMMAND_BYTE.try().expect("DMA_PRIM_COMMMAND_BYTE not set in acknowledge_disk_irq").lock().write(0x04 | 0x01)}
    trace!("acknowledge_disk_irq: status: {:#x}", status);
}

use memory::{PhysicalAddress, allocate_frame};
///allocates memory, sets the DMA controller to read mode, sends transfer commands to the ATA drive, and then ends the transfer
///returns start address of prdt if successful or Err(0) if unsuccessful
pub fn read_from_disk(drive: u8, lba: u32) -> Result<PhysicalAddress, ()>{
    //pci_write(0, 1, 1, PCI_COMMAND, 2);
    unsafe{
    DMA_PRIM_COMMAND_BYTE.try().expect("DMA_PRIM_COMMAND_BYTE not configured in read_from_disk function").lock().write(0);
    DMA_PRIM_STATUS_BYTE.try().expect("DMA_PRIM_COMMAND_BYTE not configured in read_from_disk function").lock().write(4);
    }

    if let Some(frame) = allocate_frame() {
        let addr = frame.start_address();
        if addr >= (u32::max_value() as usize) {
            error!("read_from_disk: alloc'd frame start addr={:#x} is too high (above 32-bit range)!", addr);
            return Err(());
        }
        let start_addr: u32 = addr as u32;

        // first, clear the command byte
        unsafe {
            DMA_PRIM_COMMAND_BYTE.try().expect("DMA_PRIM_COMMAND_BYTE not configured").lock().write(0);
        }   

        let prdt_paddr = set_prdt(start_addr);
        let ata_result = ata_pio::dma_read(drive,lba);
        // trying this: clear the status register
        unsafe {
            DMA_PRIM_STATUS_BYTE.try().expect("DMA_PRIM_STATUS_BYTE not configured").lock().write(0);
        }   
        trace!("read_from_disk: set up dma_read stuff, calling start_read().");
        start_transfer(false);
        // cancel_transfer(); // TODO: only do when needing to switch from Read/Write mode
        
        if ata_result.is_ok(){
            return Ok(start_addr as PhysicalAddress);
        }
    }
    else {
        error!("read_from_disk(): failed to allocate frame!");
    }

    Err(())
}


//////////////////////////////////////////////////////////////////////////////////
//New function for reading ATA DMA with more of the code in one function starts here
//////////////////////////////////////////////////////////////////////////////////


///New version of read_from_disk written from scratch and including as much as possible in the same function
pub fn read_from_disk_v2(drive: u8, lba: u32, sector_count: u32) -> Result<PhysicalAddress, ()> {

    //Resetting bus master command register, 
    unsafe{
    DMA_PRIM_COMMAND_BYTE.try().expect("DMA_PRIM_COMMAND_BYTE not initialized in read_from_diskv2").lock().write(0);
    DMA_PRIM_STATUS_BYTE.try().expect("DMA_PRIM_STATUS_BYTE not initialized in read_from_diskv2").lock().write(4);
    }

    ///TODO: Check setting bit 2 in pci command register
    let cmd = pci_read(0, 1, 1, PCI_COMMAND);
    pci_write(0, 1, 1, PCI_COMMAND, cmd | 4);
    
    //Frame allocation occurs here
     if let Some(frame) = allocate_frame() {
        let addr = frame.start_address();
        if addr >= (u32::max_value() as usize) {
            error!("read_from_disk: alloc'd frame start addr={:#x} is too high (above 32-bit range)!", addr);
            return Err(());
        }
        let start_addr: u32 = addr as u32;
        
        //prdt table defined: currently one PRD in PRDT for testing 
        //sector count is multiplied by 512 because 512 bytes in a sector
        let prdt: [u64;1] = [start_addr as u64 | (sector_count as u64) *512 << 32 | 1 << 63];
        let prdt_ref = &prdt[0] as *const u64;

        //gets the physical address of the prdt and sends that to the DMA prdt register
        let prdt_paddr = {
            let curr_task = get_my_current_task().unwrap().write();
            let curr_mmi = curr_task.mmi.as_ref().unwrap();
            let mut curr_mmi_locked = curr_mmi.lock();
            curr_mmi_locked.translate(prdt_ref as usize)
        };


        if prdt_paddr.unwrap() < (u32::max_value() as usize) {
            unsafe{
                DMA_PRIM_PRDT_ADDR.try().expect("DMA_PRDT_ADD_LOW not configured").lock().write(prdt_paddr.unwrap() as u32 );
                DMA_PRIM_STATUS_BYTE.try().expect("DMA_PRIM_STATUS_BYTE not configured").lock().write(0x04); // to reset the interrupt 0x14 status
            }  
        } 
    

        //set bit 3 to set direction of controller for "read"
        //http://wiki.osdev.org/ATA/ATAPI_using_DMA#The_Command_Byte states bit 3 value = 8, need to check if that's a typo
        unsafe{DMA_PRIM_COMMAND_BYTE.try().expect("DMA_PRIM_COMMAND_BYTE not initialized in read_from_disk_v2").lock().write(0x08);}
        let original_status = DMA_PRIM_STATUS_BYTE.try().expect("DMA_PRIM_STATUS_BYTE not configured").lock().read();
        
        //0xFA is value to clear bits 0 and 2 (0b11111010 in binary)
        unsafe{DMA_PRIM_STATUS_BYTE.try().expect("DMA_PRIM_STATUS_BYTE not configured").lock().write(original_status & 0xFA);}
        
        //selects the drive and sends LBA and sector count(1 in this case) to appropriate registers
        ata_pio::dma_read(drive,lba);
        
        //setting bit 1 of the command byte starts the transfer
        unsafe{DMA_PRIM_COMMAND_BYTE.try().expect("DMA_PRIM_COMMAND_BYTE not initialized in read_from_disk_v2").lock().write(0x08 | 0x01);}
        return Ok(start_addr as PhysicalAddress);
     }
    Err(())
}
*/