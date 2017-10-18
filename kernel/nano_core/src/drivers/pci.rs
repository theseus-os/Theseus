use port_io::Port;
use spin::{Once, Mutex};
use alloc::rc::Rc;
use collections::Vec;
use core::fmt;
use memory;
use drivers::ata_pio;
use core::sync::atomic::{AtomicBool, Ordering};

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
pub static DMA_PRIM_COMMAND_BYTE: Once<Mutex<Port<u8>>> = Once::new();
pub static DMA_PRIM_STATUS_BYTE: Once<Mutex<Port<u8>>> = Once::new();
pub static DMA_SEC_COMMAND_BYTE: Once<Mutex<Port<u8>>> = Once::new();
pub static DMA_SEC_STATUS_BYTE: Once<Mutex<Port<u8>>> = Once::new();
///ports to write prdt address to
pub static DMA_PRIM_PRDT_ADD: Once<Mutex<Port<u32>>> = Once::new();
pub static DMA_SEC_PRDT_ADD: Once<Mutex<Port<u32>>> = Once::new();

/// used to read from PCI config, additionally initializes PCI buses to be used
pub fn pci_config_read(bus: u16, slot: u16, func: u16, offset: u16) -> u16 {
    
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
    pub bar4: u16,
}

impl fmt::Display for PciDev { 
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "slot: {}, exists: {}, device_id: {:#x}, vendor_id: {:#x}, func: {}, class: {:#x}, subclass: {:#x}, bar4_address: {:#x}", 
            self.slot, self.exists, self.device_id, self.vendor_id, self.func, self.class, self.subclass, self.bar4)
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
const GET_BAR4: u16 = 0x20;

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
                        let bar4 = pci_config_read(bus, slot, f, GET_BAR4);
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
                            bar4: bar4,
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

///PCI DMA, currently set for primary bus

///sets the ports for PCI DMA configuration access using BAR4 information
pub fn set_dma_ports(){

    BAR4_BASE.call_once(||pci_config_read(0, 1, 1, GET_BAR4));
    //offsets for DMA configuration ports found in http://wiki.osdev.org/ATA/ATAPI_using_DMA under "The Bus Master Register"
    DMA_PRIM_COMMAND_BYTE.call_once(||Mutex::new( Port::new(BAR4_BASE.try().expect("BAR4 address not configured")+0)));
    DMA_PRIM_STATUS_BYTE.call_once(||Mutex::new( Port::new(BAR4_BASE.try().expect("BAR4 address not configured")+0x2)));
    DMA_SEC_COMMAND_BYTE.call_once(||Mutex::new( Port::new(BAR4_BASE.try().expect("BAR4 address not configured")+0x8)));
    DMA_SEC_STATUS_BYTE.call_once(||Mutex::new( Port::new(BAR4_BASE.try().expect("BAR4 address not configured")+0xA))); 

    DMA_PRIM_PRDT_ADD.call_once(||Mutex::new( Port::new(BAR4_BASE.try().expect("BAR4 address not configured")+0x4)));
    DMA_SEC_PRDT_ADD.call_once(||Mutex::new( Port::new(BAR4_BASE.try().expect("BAR4 address not configured")+0xC)));
    
    

}

///uses the memory allocator to allocate a frame and writes the address to the DMA address port
pub fn allocate_mem() -> u32{

    let frame = memory::allocate_frame().expect("pci::allocate_mem() - out of memory trying to allocate frame");
    frame.start_address() as u32

}

///creates a prdt table and send its pointer to the DMA PRDT address port
pub fn set_prdt(start_add: u32) -> u32{

    
    let prdt: [u64;1] = [start_add as u64 | 2 <<32 | 1 << 63];
    let prdt_ref = &prdt as *const u64;
    let prdt_pointer: u32 = unsafe{*prdt_ref as u32};
    unsafe{DMA_PRIM_PRDT_ADD.try().expect("DMA_PRDT_ADD_LOW not configured").lock().write(prdt_pointer);}
    prdt_pointer

}


///functions which configure the DMA controller for read and write mode
pub fn start_read(){
    //sets bit 0 in the command byte to put the dma controller in start mode, clears bit 3 to put in read mode
    unsafe{DMA_PRIM_COMMAND_BYTE.try().expect("DMA_PRIM_COMMAND_BYTE not configured").lock().write(1)}; 
    //sets bit 0 in the status byte to clear error and interrupt bits and set dma mode bit
    unsafe{DMA_PRIM_STATUS_BYTE.try().expect("DMA_PRIM_COMMAND_BYTE not configured").lock().write(1)};
}

pub fn start_write(){
    //sets bit 0 in the command byte to put the dma controller in start mode, set bit 3 to put in write mode
    unsafe{DMA_PRIM_COMMAND_BYTE.try().expect("DMA_PRIM_COMMAND_BYTE not configured").lock().write(0b101)};
    //sets bit 0 in the status byte to clear error and interrupt bits and set dma mode bit
    unsafe{DMA_PRIM_STATUS_BYTE.try().expect("DMA_PRIM_COMMAND_BYTE not configured").lock().write(1)};
}

///immediately ends the transfer, dma controller clears any set prdt address 
pub fn end_transfer(){
    unsafe{DMA_PRIM_COMMAND_BYTE.try().expect("DMA_PRIM_COMMAND_BYTE not configured").lock().write(0)};

}

///the status byte must be read after each IRQ (I believe IRQ 14 which is handled in ata_pio)
///IRQ number still needs to be confirmed, was 14 according to http://www.pchell.com/hardware/irqs.shtml
pub fn acknowledge_disk_irq(){
    DMA_PRIM_STATUS_BYTE.try().expect("DMA_PRIM_STATUS_BYTE not configured").lock().read();
}

///allocates memory, sets the DMA controller to read mode, sends transfer commands to the ATA drive, and then ends the transfer
///returns start address of prdt if successful or Err(0) if unsuccessful
pub fn read_from_disk(drive: u8, lba: u32) -> Result<u32, ()>{
    let start_add: u32 = allocate_mem();
    set_prdt(start_add);
    start_read();
    let ata_result = ata_pio::dma_read(drive,lba);
    end_transfer();
    if ata_result.is_ok(){
        return Ok(start_add);
    }
    return Err(());
}
