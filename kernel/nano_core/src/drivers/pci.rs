use port_io::Port;
use spin::{Once, Mutex};
use alloc::rc::Rc;
use core::sync::atomic::{AtomicBool, Ordering};
use alloc::Vec;
use core::fmt;

//data written here sets information at CONFIG_DATA
const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

/// access to CONFIG_ADDRESS 
static PCI_CONFIG_ADDRESS_PORT: Mutex<Port<u32>> = Mutex::new( Port::new(CONFIG_ADDRESS));
/// acccess to CONFIG_DATA
static PCI_CONFIG_DATA_PORT: Mutex<Port<u32>> = Mutex::new( Port::new(CONFIG_DATA));

pub static PCI_BUSES: Once<Vec<PciBus>> = Once::new();

pub static BAR4_BASE: Once<u16> = Once::new();
pub static DMA_FINISHED: AtomicBool = AtomicBool::new(true);

///the ports to the DMA primary and secondary command and status bytes
// pub static PCI_COMMAND_PORT: Once<Mutex<Port<u16>>> = Once::new(); // I don't think we need this? --Kevin
pub static DMA_PRIM_COMMAND_BYTE: Once<Mutex<Port<u8>>> = Once::new();
pub static DMA_PRIM_STATUS_BYTE: Once<Mutex<Port<u8>>> = Once::new();
pub static DMA_SEC_COMMAND_BYTE: Once<Mutex<Port<u8>>> = Once::new();
pub static DMA_SEC_STATUS_BYTE: Once<Mutex<Port<u8>>> = Once::new();
///ports to write prdt address to
pub static DMA_PRIM_PRDT_ADDR: Once<Mutex<Port<u32>>> = Once::new();
pub static DMA_SEC_PRDT_ADDR: Once<Mutex<Port<u32>>> = Once::new();

//used to read from PCI config, additionally initializes PCI buses to be used
pub fn pci_config_read(bus: u32, slot: u32, func: u32, offset: u32)->u16{
    
    //data to be written to CONFIG_ADDRESS
    let lbus = bus as u32;
    let lslot = slot as u32;
    let lfunc = func as u32;
    let loffset = offset as u32;
    let address: u32 = (lbus << 16) | (lslot << 11) | (lfunc << 8) | (loffset & 0xfc) | 0x8000_0000;

    unsafe { PCI_CONFIG_ADDRESS_PORT.lock().write(address); }
    let inval = PCI_CONFIG_DATA_PORT.lock().read() >> ((offset & 2) * 8);
    (inval & 0xffff) as u16
}

/// struct representing PCI Bus, contains array of PCI Devices
#[derive(Debug)]
pub struct PciBus {
    pub bus_number: u16,
    //total number of devices connected to the bus
    pub total_connected: u8,
    //array of slots
    pub connected_devices: Vec<PciDev>,
}

// impl Default for PciBus{
//     fn default() -> PciBus {
//         let def_connected: [PciDev; 32] = [PciDev{..Default::default()};32];
//         PciBus {
//             bus_number: 0xFFFF, 
//             total_connected: 0xFF, 
//             connected_devices: def_connected }
//     }
// }

// impl Clone for PciBus {
//     fn clone(&self) -> PciBus {
//         *self
//     }
// }

//
#[derive(Debug)]
pub struct PciDev{
    //slot number
    pub slot: u8,
    pub exists: bool,
    pub device_id: u16, 
    pub vendor_id: u16,
    pub func: u16,
    /// class can be used to determine device type http://wiki.osdev.org/PCI#Class_Codes 
    pub class: u8,
    /// subclass can also be used to determine device type http://wiki.osdev.org/PCI#Class_Codes 
    pub subclass: u8,
}

impl fmt::Display for PciDev { 
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "slot: {}, exists: {}, device_id: {:#x}, vendor_id: {:#x}, func: {}, class: {:#x}, subclass: {:#x}", 
            self.slot, self.exists, self.device_id, self.vendor_id, self.func, self.class, self.subclass)
    }
}

// impl Default for PciDev{
//     fn default()-> PciDev{
//         PciDev {
//             slot: 0xFF, 
//             exists: false, 
//             device_id: 0xFFFF, 
//             vendor_id: 0xFFFF, 
//             func: 0xFFFF,
//             class_code: 0xFFFF
//         }
//     }
// }

// impl Copy for PciDev{}

// impl Clone for PciDev {
//     fn clone(&self) -> PciDev {
//         *self
//     }
// }



const GET_VENDOR_ID: u16 = 0;
const GET_DEVICE_ID: u16 = 2;
const GET_CLASS_ID:  u16 = 0xA;


//initializes structure containing PCI buses and their attached devices
pub fn init_pci_buses(){
    
    //array which holds all PciBuses
	let mut buses: Vec<PciBus> = Vec::new();

	PCI_BUSES.call_once( || {
        
        for bus in 0..256 {

            let mut device_list: Vec<PciDev> = Vec::new();
            let mut num_connected: u8 = 0;

            for slot in 0..32 {

                for f in 0..8 {

                    //sets vendor id of device
                    let vendor_id = pci_config_read(bus, slot, f, GET_VENDOR_ID);
                    let mut device: PciDev;
                    
                    //if vendor ID is 0xFFFF, no device is connected
                    if vendor_id == 0xFFFF {
                        continue;
                        // device = PciDev{..Default::default()};
                    }

                    //otherwise, gets class code and device id and adds a count to how many devices connected to bus
                    else {
                        num_connected += 1;
                        let device_id = pci_config_read(bus, slot, f, GET_DEVICE_ID);
                        let class_word = pci_config_read(bus, slot, f, GET_CLASS_ID);
                        let class: u8 = (class_word >> 8) as u8;
                        let subclass: u8 = (class_word & 0xFF) as u8;

                        device = PciDev {
                            slot: slot as u8, 
                            exists: true, 
                            device_id: device_id, 
                            vendor_id: vendor_id,
                            func: f,
                            class: class,
                            subclass: subclass, 
                        };

                        debug!("found pci device: {}", device);
                    }        
                    device_list.push(device);
                }
            }
            buses.push( PciBus {
                bus_number: bus as u16, 
                total_connected: num_connected, 
                connected_devices: device_list
            });
        }

	    buses
    });
	
}


/*
///PCI DMA, currently set for primary bus

///sets the ports for PCI DMA configuration access using BAR4 information
pub fn set_dma_ports(){
    pci_set_command_bus_master_bit(0, 1, 1);

    let bar4_raw_top: u16 = pci_read(0, 1, 1, GET_BAR4 + 0x2);
    let bar4_raw_bot: u16 = pci_read(0, 1, 1, GET_BAR4);
    let bar4_raw: u32 = ((bar4_raw_top as u32) << 16) | (bar4_raw_bot as u32);
    debug!("BAR4_RAW 32-bit: {:#x}", bar4_raw);
    // the LSB should be set for Port I/O, see http://wiki.osdev.org/PCI#Base_Address_Registers
    if bar4_raw & 0x1 != 0x1 {
        panic!("set_dma_ports: BAR4 indicated MMIO not Port IO, currently unsupported!");
    }
    // we need to mask the lowest 2 bits of the bar4 address to use it as a port
    let bar4 = BAR4_BASE.call_once(|| (bar4_raw & 0xFFFF_FFFC) as u16 );
    debug!("set_dma_ports: BAR4_BASE: {:#x}", bar4);

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
        let tasklist = task::get_tasklist().read();
        let curr_task = tasklist.get_current().unwrap().write();
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
    //pci_write(0, 1, 1, GET_COMMAND, 2);
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
    pci_write(0, 1, 1, GET_COMMAND, 4);
    
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
            let tasklist = task::get_tasklist().read();
            let curr_task = tasklist.get_current().unwrap().write();
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