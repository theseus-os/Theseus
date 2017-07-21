use port_io::Port;
use spin::{Once, Mutex};
use alloc::rc::Rc;

//data written here sets information at CONFIG_DATA
const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

//access to CONFIG_ADDRESS 
static PCI_CONFIG_ADDRESS_PORT: Mutex<Port<u32>> = Mutex::new( Port::new(CONFIG_ADDRESS));
//acccess to CONFIG_DATA
static PCI_CONFIG_DATA_PORT: Mutex<Port<u32>> = Mutex::new( Port::new(CONFIG_DATA));

pub static PCI_BUSES: Once<[PciBus;16]> = Once::new();

//used to read from PCI config, additionally initializes PCI buses to be used
pub fn pci_config_read(bus: u32, slot: u32, func: u32, offset: u32)->u16{
    
    //data to be written to CONFIG_ADDRESS
    let address:u32 = ((bus<<16) | (slot<<11) |  (func << 8) | (offset&0xfc) | 0x80000000);

    unsafe{PCI_CONFIG_ADDRESS_PORT.lock().write(address);}

    ((PCI_CONFIG_DATA_PORT.lock().read() >> (offset&2) * 8) & 0xffff) as u16
	//PCI_CONFIG_DATA_PORT.lock().read()
}

//struct representing PCI Bus, contains array of PCI Devices
pub struct PciBus{
    pub bus_number: u16,
    //total number of devices connected to the bus
    pub total_connected: u8,
    //array of slots
    pub connected_devices: [PciDev; 32],
    
}
impl Default for PciBus{
    fn default() -> PciBus{
        let def_connected: [PciDev; 32] = [PciDev{..Default::default()};32];
        PciBus{bus_number: 0xFFFF, total_connected:0xFF, connected_devices: def_connected }
    }

}

impl Copy for PciBus{}

impl Clone for PciBus {
    fn clone(&self) -> PciBus {
        *self
    }
}

//
pub struct PciDev{
    //slot number
    pub slot: u8,
    pub exists: bool,
    pub device_id: u16, 
    pub vendor_id: u16,
    //class_code can be used to determine device type http://wiki.osdev.org/PCI#Class_Codes 
    pub class_code: u16,

}

impl Default for PciDev{
    fn default()-> PciDev{
        PciDev{slot: 0xFF, exists: false, device_id: 0xFFFF, vendor_id: 0xFFFF, class_code: 0xFFFF}
    }
}

impl Copy for PciDev{}

impl Clone for PciDev {
    fn clone(&self) -> PciDev {
        *self
    }
}

//initializes structure containing PCI buses and their attached devices
pub fn init_pci_buses(){
    
    //array which holds all PciBuses
	let mut buses: [PciBus; 16] = [PciBus{..Default::default()};16]; 
	

	PCI_BUSES.call_once( || {
        
        for bus in 0..16{
            let mut device_list: [PciDev; 32] = [PciDev{..Default::default()};32];
            let mut num_connected: u8 = 0;

            for slot in 0..32{
                //sets vendor id of device
                let vendor_id = pci_config_read(bus,slot,0,0);
                let mut device: PciDev;
                
                //if vendor ID is 0xFFFF, no device is connected
                if vendor_id == 0xFFFF{
                    device = PciDev{..Default::default()};
                }

                //otherwise, gets class code and device id and adds a count to how many devices connected to bus
                else{
                    num_connected += 1;
                    let device_id = pci_config_read(bus,slot,0,0x2);
                    let class_code = pci_config_read(bus, slot, 0, 0xa);
                    device = PciDev{slot: slot as u8, exists: true, device_id: device_id, vendor_id: vendor_id,class_code: class_code};
                }        
                device_list[slot as usize] = device;
            }
            buses[bus as usize] = PciBus{bus_number: bus as u16, total_connected: num_connected, connected_devices:device_list};
        }


	buses});
	
}
