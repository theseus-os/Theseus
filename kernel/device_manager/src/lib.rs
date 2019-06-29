#![no_std]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate event_types;
extern crate ata;
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



pub fn init(keyboard_producer: DFQueueProducer<Event>) -> Result<(), &'static str>  {
    keyboard::init(keyboard_producer);
    mouse::init();

    
    for dev in pci::pci_device_iter() {
        debug!("Found PCI device (hex values): {:X?}", dev);
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
        warn!("Note: no e1000 device found on this system.");
    }
    

    // look for devices we support
    for dev in pci::pci_device_iter() {
        // look for IDE controllers (include IDE disk)
        if dev.class == 0x01 && dev.subclass == 0x01 {
            ata::AtaController::new(dev)?;
        }
    }


    // testing ata pio read, write, and IDENTIFY functionality, example of uses, can be deleted 
    ata_pio::init_ata_devices();
    let test_arr: [u16; 256] = [630; 256];
    debug!("Value from ATA identification function: {:?}", ata_pio::ATA_DEVICES.try().expect("ATA_DEVICES used before initialization").primary_master);
    let begin = ata_pio::pio_read(0xE0, 0).map_err(|_e| "initial pio_read() failed")?;
    debug!("Value from drive at sector 0 before write: {:?}", &begin[..]);
    ata_pio::pio_write(0xE0, 0, test_arr).map_err(|_e| "pio_write() failed")?;
    let end = ata_pio::pio_read(0xE0, 0).map_err(|_e| "subsequent pio_read() failed")?;
    debug!("Value from drive at sector 0 after write: {:?}", &end[..]);
    
    Ok(())
}
