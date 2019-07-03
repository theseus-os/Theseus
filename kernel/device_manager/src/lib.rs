#![no_std]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate event_types;
extern crate ata;
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
            warn!("Initializing ATA Controller...");
            let mut ata_controller = ata::AtaController::new(dev)?;
            let mut initial_buf: [u8; 6100] = [0; 6100];
            let primary_drive = ata_controller.primary_master.as_mut().unwrap();
            let bytes_read = primary_drive.read_pio(&mut initial_buf[..], 0)?;
            debug!("{:X?}", &initial_buf[..]);
            debug!("{:?}", core::str::from_utf8(&initial_buf));
            trace!("READ_PIO {} bytes", bytes_read);

            let mut write_buf = [0u8; 512*3];
            for b in write_buf.chunks_exact_mut(16) {
                b.copy_from_slice(b"QWERTYUIOPASDFJK");
            }
            let bytes_written = primary_drive.write_pio(&write_buf[..512], 1024);
            debug!("WRITE_PIO {:?}", bytes_written);

            let mut after_buf: [u8; 6100] = [0; 6100];
            let bytes_read = primary_drive.read_pio(&mut after_buf[..], 0)?;
            debug!("{:X?}", &after_buf[..]);
            debug!("{:?}", core::str::from_utf8(&after_buf));
            trace!("AFTER WRITE READ_PIO {} bytes", bytes_read);


            for (i, (before, after)) in initial_buf.iter().zip(after_buf.iter()).enumerate() {
                if before != after {
                    trace!("byte {} diff: {:X} -> {:X}", i, before, after);
                }
            }
        }
    }
    
    Ok(())
}
