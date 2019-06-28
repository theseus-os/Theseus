#![no_std]

#![allow(dead_code)]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate port_io;

use core::fmt;
use core::ops::{Deref, DerefMut};
use alloc::vec::Vec;
use port_io::Port;
use spin::{Once, Mutex};




// see this: http://wiki.osdev.org/PCI#PCI_Device_Structure
pub const PCI_VENDOR_ID:             u16 = 0x0;
pub const PCI_DEVICE_ID:             u16 = 0x2;
pub const PCI_COMMAND:               u16 = 0x4;
pub const PCI_STATUS:                u16 = 0x6;
pub const PCI_REVISION_ID:           u16 = 0x8;
pub const PCI_PROG_IF:               u16 = 0x9;
pub const PCI_SUBCLASS:              u16 = 0xA;
pub const PCI_CLASS:                 u16 = 0xB;
pub const PCI_CACHE_LINE_SIZE:       u16 = 0xC;
pub const PCI_LATENCY_TIMER:         u16 = 0xD;
pub const PCI_HEADER_TYPE:           u16 = 0xE;
pub const PCI_BIST:                  u16 = 0xF;
pub const PCI_BAR0:                  u16 = 0x10;
pub const PCI_BAR1:                  u16 = 0x14;
pub const PCI_BAR2:                  u16 = 0x18;
pub const PCI_BAR3:                  u16 = 0x1C;
pub const PCI_BAR4:                  u16 = 0x20;
pub const PCI_BAR5:                  u16 = 0x24;
pub const PCI_CARDBUS_CIS:           u16 = 0x28;
pub const PCI_SUBSYSTEM_VENDOR_ID:   u16 = 0x2C;
pub const PCI_SUBSYSTEM_ID:          u16 = 0x2E;
pub const PCI_EXPANSION_ROM_BASE:    u16 = 0x30;
pub const PCI_CAPABILITIES:          u16 = 0x34;
// 0x35 through 0x3B are reserved
pub const PCI_INTERRUPT_LINE:        u16 = 0x3C;
pub const PCI_INTERRUPT_PIN:         u16 = 0x3D;
pub const PCI_MIN_GRANT:             u16 = 0x3E;
pub const PCI_MAX_LATENCY:           u16 = 0x3F;


/// The PCI configuration space address offset should set the least-significant 2 bits to zero.
const PCI_CONFIG_ADDRESS_OFFSET_MASK: u16 = 0xFC; 

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



pub fn get_pci_buses() -> &'static Vec<PciBus> {
    static PCI_BUSES: Once<Vec<PciBus>> = Once::new();
    PCI_BUSES.call_once( || scan_pci() )
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

/// Returns a reference to the first `PciDevice` with the given vendor id and device id, if one exists.
#[deprecated(note = "This should be removed. It's bad because there can be multiple devices with the same vendor/device ID... which one gets returned?")]
pub fn get_pci_device_vd(vendor_id: u16, device_id: u16) -> Option<&'static PciDevice> {
    pci_device_iter().filter(|dev| dev.vendor_id == vendor_id && dev.device_id == device_id).next()
}


/// Returns an iterator that iterates over all `PciDevice`s, in no particular guaranteed order. 
/// If the PCI bus hasn't been initialized, this initializes the PCI bus & scans it to enumerates devices.
pub fn pci_device_iter() -> impl Iterator<Item = &'static PciDevice> {
    get_pci_buses().iter().flat_map(|b| b.devices.iter())
}


/// A PCI bus, which contains a list of PCI devices on that bus.
#[derive(Debug)]
pub struct PciBus {
    /// the number of this PCI bus
    pub bus_number: u16,
    /// array of devices
    pub devices: Vec<PciDevice>,
}


/// Scans all PCI Buses (brute force iteration) to enumerate PCI Devices on each bus.
/// Initializes structures containing this information. 
fn scan_pci() -> Vec<PciBus> {
	let mut buses: Vec<PciBus> = Vec::new();

    for bus in 0..256 {
        let mut device_list: Vec<PciDevice> = Vec::new();

        for slot in 0..32 {
            let loc_zero = PciLocation { bus, slot, func: 0 };
            // skip the whole slot if the vendor ID is 0xFFFF
            if 0xFFFF == loc_zero.pci_read_16(PCI_VENDOR_ID) {
                continue;
            }

            // If the header's MSB is set, then there are multiple functions for this device,
            // and we should check all 8 of them to be sure.
            // Otherwise, only need to 
            let header_type = loc_zero.pci_read_8(PCI_HEADER_TYPE);
            let functions_to_check = if header_type & 0x80 == 0x80 {
                0..8
            } else {
                0..1
            };

            for f in functions_to_check {
                let loc = PciLocation { bus, slot, func: f };
                let vendor_id = loc.pci_read_16(PCI_VENDOR_ID);
                if vendor_id == 0xFFFF {
                    continue;
                }

                let device = PciDevice {
                    vendor_id:        vendor_id,
                    device_id:        loc.pci_read_16(PCI_DEVICE_ID), 
                    command:          loc.pci_read_16(PCI_COMMAND),
                    status:           loc.pci_read_16(PCI_STATUS),
                    revision_id:      loc.pci_read_8( PCI_REVISION_ID),
                    prog_if:          loc.pci_read_8( PCI_PROG_IF),
                    subclass:         loc.pci_read_8( PCI_SUBCLASS),
                    class:            loc.pci_read_8( PCI_CLASS),
                    cache_line_size:  loc.pci_read_8( PCI_CACHE_LINE_SIZE),
                    latency_timer:    loc.pci_read_8( PCI_LATENCY_TIMER),
                    header_type:      loc.pci_read_8( PCI_HEADER_TYPE),
                    bist:             loc.pci_read_8( PCI_BIST),
                    bars:             [
                                          loc.pci_read_32(PCI_BAR0),
                                          loc.pci_read_32(PCI_BAR1), 
                                          loc.pci_read_32(PCI_BAR2), 
                                          loc.pci_read_32(PCI_BAR3), 
                                          loc.pci_read_32(PCI_BAR4), 
                                          loc.pci_read_32(PCI_BAR5), 
                                      ],
                    int_pin:          loc.pci_read_8(PCI_INTERRUPT_PIN),
                    int_line:         loc.pci_read_8(PCI_INTERRUPT_LINE),
                    loc:              loc,
                };

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


/// The bus, slot, and function number of a given PCI device.
/// This offers methods for reading and writing the PCI config space. 
#[derive(Copy, Clone)]
pub struct PciLocation {
    bus:  u16,
    slot: u16,
    func: u16,
}

impl PciLocation {
    pub fn bus(&self) -> u16 { self.bus }
    pub fn slot(&self) -> u16 { self.slot }
    pub fn function(&self) -> u16 { self.func }


    /// Computes a PCI address from bus, slot, func, and offset. 
    /// The least two significant bits of offset are masked, so it's 4-byte aligned addressing,
    /// which makes sense since we read PCI data (from the configuration space) in 32-bit chunks.
    fn pci_address(self, offset: u16) -> u32 {
        ((self.bus  as u32) << 16) | 
        ((self.slot as u32) << 11) | 
        ((self.func as u32) <<  8) | 
        ((offset as u32) & (PCI_CONFIG_ADDRESS_OFFSET_MASK as u32)) | 
        0x8000_0000
    }

    /// read 32-bit data at the specified `offset` from the PCI device specified by the given `bus`, `slot`, `func` set.
    pub fn pci_read_32(&self, offset: u16) -> u32 {
        unsafe { 
            PCI_CONFIG_ADDRESS_PORT.lock().write(self.pci_address(offset)); 
        }
        PCI_CONFIG_DATA_PORT.lock().read() >> ((offset & (!PCI_CONFIG_ADDRESS_OFFSET_MASK)) * 8)
    }

    /// Read 16-bit data at the specified `offset` from this PCI device.
    pub fn pci_read_16(&self, offset: u16) -> u16 {
        self.pci_read_32(offset) as u16
    } 

    /// Read 8-bit data at the specified `offset` from the PCI device.
    pub fn pci_read_8(&self, offset: u16) -> u8 {
        self.pci_read_32(offset) as u8
    }

    /// Write 32-bit data to the specified `offset` for the PCI device.
    pub fn pci_write(&self, offset: u16, value: u32) {
        unsafe { 
            PCI_CONFIG_ADDRESS_PORT.lock().write(self.pci_address(offset)); 
            PCI_CONFIG_DATA_PORT.lock().write((value) << ((offset & 2) * 8));
        }
    }

    /// Sets the PCI device's bit 3 in the command portion, which is apparently needed to activate DMA (??)
    pub fn pci_set_command_bus_master_bit(&self) {
        unsafe { 
            PCI_CONFIG_ADDRESS_PORT.lock().write(self.pci_address(PCI_COMMAND));
        }
        let inval = PCI_CONFIG_DATA_PORT.lock().read(); 
        trace!("pci_set_command_bus_master_bit: PciDevice: {}, read value: {:#x}", self, inval);
        unsafe {
            PCI_CONFIG_DATA_PORT.lock().write(inval | (1 << 2));
        }
        trace!("pci_set_command_bus_master_bit: PciDevice: {}, read value AFTER WRITE CMD: {:#x}", 
            self,
            PCI_CONFIG_DATA_PORT.lock().read()
        );
    }
}

impl fmt::Display for PciLocation {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "B: {}, S: {}, F: {}", self.bus, self.slot, self.func)
    }
}

impl fmt::Debug for PciLocation {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{}", self)
    }
}


/// Contains information common to every type of PCI Device,
/// and offers functions for reading/writing to the PCI configuration space.
#[derive(Debug)]
pub struct PciDevice {
    /// the bus, slot, and function number that locates this PCI device in the bus tree.
    loc: PciLocation,

    /// The class code, used to determine device type: http://wiki.osdev.org/PCI#Class_Codes 
    pub class: u8,
    /// The subclass code, used to determine device type: http://wiki.osdev.org/PCI#Class_Codes 
    pub subclass: u8,
    /// The programming interface of this PCI device
    pub prog_if: u8,
    /// The six Base Address Registers (BARs)
    pub bars: [u32; 6],
    pub vendor_id: u16,
    pub device_id: u16, 
    pub command: u16,
    pub status: u16,
    pub revision_id: u8,
    pub cache_line_size: u8,
    pub latency_timer: u8,
    pub header_type: u8,
    pub bist: u8,
    pub int_pin: u8,
    pub int_line: u8,
}

impl Deref for PciDevice {
    type Target = PciLocation;
    fn deref(&self) -> &PciLocation {
        &self.loc
    }
}
impl DerefMut for PciDevice {
    fn deref_mut(&mut self) -> &mut PciLocation {
        &mut self.loc
    }
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