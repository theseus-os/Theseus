use port_io::Port;
use spin::{Once, Mutex};
use alloc::rc::Rc;
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
