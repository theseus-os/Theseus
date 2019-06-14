#![no_std]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate event_types;
extern crate ata_pio;
extern crate e1000;
extern crate memory;
extern crate dfqueue; 
extern crate apic;
extern crate acpi;
extern crate keyboard;
extern crate pci;
extern crate mouse;
extern crate network_manager;
extern crate ethernet_smoltcp_device;
extern crate smoltcp;
extern crate ixgbe;

use dfqueue::DFQueueProducer;
use event_types::Event;
use memory::MemoryManagementInfo;
use pci::{PciDevice, get_pci_device_vd};


/// A randomly chosen IP address that must be outside of the DHCP range.. // TODO FIXME: use DHCP to acquire IP
const DEFAULT_LOCAL_IP: &'static str = "10.0.2.15/24"; // the default QEMU user-slirp network gives IP addresses of "10.0.2.*"
// const DEFAULT_LOCAL_IP: &'static str = "192.168.1.252/24"; // home router reserved IP
// const DEFAULT_LOCAL_IP: &'static str = "10.42.0.91/24"; // rice net IP

/// Standard home router address. // TODO FIXME: use DHCP to acquire gateway IP
const DEFAULT_GATEWAY_IP: [u8; 4] = [10, 0, 2, 2]; // the default QEMU user-slirp networking gateway IP
// const DEFAULT_GATEWAY_IP: [u8; 4] = [192, 168, 1, 1]; // the default gateway for our TAP-based bridge
// const DEFAULT_GATEWAY_IP: [u8; 4] = [10, 42, 0, 1]; // rice net gateway ip

/// This is for early-stage initialization of things like VGA, ACPI, (IO)APIC, etc.
pub fn early_init(kernel_mmi: &mut MemoryManagementInfo) -> Result<(), &'static str> {
    // first, init the local apic info
    apic::init(&mut kernel_mmi.page_table)?;
    
    // then init/parse the ACPI tables to fill in the APIC details, among other things
    // this returns an iterator over the "APIC" (MADT) tables, which we use to boot AP cores
    acpi::init(&mut kernel_mmi.page_table)?;

    Ok(())
}



pub fn init(keyboard_producer: DFQueueProducer<Event>) -> Result<(), &'static str>  {
    keyboard::init(keyboard_producer);
    mouse::init();

    
    /* for dev in pci::pci_device_iter() {
        debug!("Found pci device: {:?}", dev);
    } */

    // intialize the 82599 NIC if present and add it to the list of network interfaces
    match init_pci_dev(ixgbe::registers::INTEL_VEND, ixgbe::registers::INTEL_82599, ixgbe::IxgbeNic::init) {
        Ok(()) => {
            let gateway_ip = [192,168,0,1];
            let nic_ref = ixgbe::get_ixgbe_nic().ok_or("device_manager::init(): ixgbe nic hasn't been initialized")?;
            ethernet_smoltcp_device::EthernetNetworkInterface::add_network_interface(nic_ref, "192.168.0.101/24", &gateway_ip)?;
        },
        Err(_e) => warn!("Ixgbe device not found"),
    };
    
    // intialize the E1000 NIC if present and add it to the list of network interfaces
    match init_pci_dev(e1000::INTEL_VEND, e1000::E1000_DEV, e1000::E1000Nic::init) {
        Ok(()) => {
            let nic_ref = e1000::get_e1000_nic().ok_or("device_manager::init(): e1000 nic hasn't been initialized")?;
            ethernet_smoltcp_device::EthernetNetworkInterface::add_network_interface(nic_ref, DEFAULT_LOCAL_IP, &DEFAULT_GATEWAY_IP)?;
        },
        Err(_e) => warn!("E1000 device not found"),
    };

    // testing ata pio read, write, and IDENTIFY functionality, example of uses, can be deleted 
    /*
    ata_pio::init_ata_devices();
    let test_arr: [u16; 256] = [630;256];
    println!("Value from ATA identification function: {}", ata_pio::ATA_DEVICES.try().expect("ATA_DEVICES used before initialization").primary_master);
    let begin = ata_pio::pio_read(0xE0,0);
    //only use value if Result is ok
    if begin.is_ok(){
        println!("Value from drive at sector 0 before write:  {}", begin.unwrap()[0]);
    }
    ata_pio::pio_write(0xE0,0,test_arr);
    let end = ata_pio::pio_read(0xE0,0);
    if end.is_ok(){
    println!("Value from drive at sector 0 after write: {}", end.unwrap()[0]);
    }
    */

    /*
    let bus_array = pci::PCI_BUSES.try().expect("PCI_BUSES not initialized");
    let ref bus_zero = bus_array[0];
    let slot_zero = bus_zero.connected_devices[0]; 
    println!("pci config data for bus 0, slot 0: dev id - {:#x}, class code - {:#x}", slot_zero.device_id, slot_zero.class_code);
    println!("pci config data {:#x}",pci::pci_config_read(0,0,0,0x0c));
    println!("{:?}", bus_zero);
    */
    Ok(())

}

/// Function type for a pci device initialization procedure
pub type PciDevInitFunc = fn(&PciDevice) -> Result<(), &'static str>;

/// Finds the pci device and initializes it.
/// 
/// # Arguments
/// * `vendor_id`: pci vendor id of device
/// * `device_id`: pci device id of device
/// * `init_func`: initialization function for the pci device
fn init_pci_dev(vendor_id: u16, device_id: u16, init_func: PciDevInitFunc) -> Result<(), &'static str> {
    let pci_dev = get_pci_device_vd(vendor_id, device_id).ok_or("device_manager::init_pci_dev: device not found")?;
    init_func(pci_dev)?;
    Ok(())
}
