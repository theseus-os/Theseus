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
extern crate e1000_smoltcp_device;
extern crate smoltcp;


use alloc::sync::Arc;
use spin::Mutex;
use dfqueue::DFQueueProducer;
use event_types::Event;
use memory::MemoryManagementInfo;
use pci::get_pci_device_vd;
use smoltcp::wire::{IpCidr, Ipv4Address};
use core::str::FromStr;


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



pub fn init(key_producer: DFQueueProducer<Event>, mouse_producer: DFQueueProducer<Event>) -> Result<(), &'static str>  {
    keyboard::init(key_producer);
    mouse::init(mouse_producer);

    
    for dev in pci::pci_device_iter() {
        debug!("Found pci device: {:?}", dev);
    }

    if let Some(e1000_pci_dev) = get_pci_device_vd(e1000::INTEL_VEND, e1000::E1000_DEV) {
        debug!("e1000 PCI device found: {:?}", e1000_pci_dev);
        let e1000_nic_ref = e1000::E1000Nic::init(e1000_pci_dev)?;
        let static_ip = IpCidr::from_str(DEFAULT_LOCAL_IP).map_err(|_e| "couldn't parse 'DEFAULT_LOCAL_IP' address")?;
        let gateway_ip = Ipv4Address::from_bytes(&DEFAULT_GATEWAY_IP);
        let e1000_iface = e1000_smoltcp_device::E1000NetworkInterface::new(e1000_nic_ref, Some(static_ip), Some(gateway_ip))?;
        network_manager::NETWORK_INTERFACES.lock().push(Arc::new(Mutex::new(e1000_iface)));
    }
    else {
        warn!("No e1000 device found on this system.");
    }
    

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
