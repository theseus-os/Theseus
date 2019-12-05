#![no_std]
#![feature(trait_alias)]

#[macro_use] extern crate log;
extern crate event_types;
extern crate e1000;
extern crate memory;
extern crate apic;
extern crate acpi;
extern crate keyboard;
extern crate pci;
extern crate mouse;
extern crate storage_manager;
extern crate network_manager;
extern crate ethernet_smoltcp_device;
extern crate mpmc;

use mpmc::Queue;
use event_types::Event;
use memory::MemoryManagementInfo;
use ethernet_smoltcp_device::EthernetNetworkInterface;
use network_manager::add_to_network_interfaces;

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
    // First, initialize the local apic info.
    apic::init(&mut kernel_mmi.page_table)?;
    
    // Then, parse the ACPI tables to acquire system configuration info.
    acpi::init(&mut kernel_mmi.page_table)?;

    Ok(())
}


/// Initializes all other devices, such as the keyboard and mouse
/// as well as all devices discovered on the PCI bus.
pub fn init(key_producer: Queue<Event>, mouse_producer: Queue<Event>) -> Result<(), &'static str>  {
    keyboard::init(key_producer);
    mouse::init(mouse_producer);

    // Initialize/scan the PCI bus to discover PCI devices
    for dev in pci::pci_device_iter() {
        debug!("Found pci device: {:?}", dev);
    } 

    // Iterate over all PCI devices and initialize the drivers for the devices we support.
    for dev in pci::pci_device_iter() {
        // Currently we skip Bridge devices, since we have no use for them yet. 
        if dev.class == 0x06 {
            continue;
        }

        // If this is a storage device, initialize it as such.
        match storage_manager::init_device(dev) {
            // finished with this device, proceed to the next one.
            Ok(true)  => continue,
            // fall through, let another handler deal with it.
            Ok(false) => { }
            // error, so skip this device.
            Err(e) => {
                error!("Failed to initialize storage device, it will be unavailable.\n{:?}\nError: {}", dev, e);
                continue;
            }
        }

        // If this is a network device, initialize it as such.
        // Look for networking controllers, specifically ethernet cards
        if dev.class == 0x02 && dev.subclass == 0x00 {
            if dev.vendor_id == e1000::INTEL_VEND && dev.device_id == e1000::E1000_DEV {
                info!("e1000 PCI device found at: {:?}", dev.location);
                let e1000_nic_ref = e1000::E1000Nic::init(dev)?;
                let e1000_interface = EthernetNetworkInterface::new_ipv4_interface(e1000_nic_ref, DEFAULT_LOCAL_IP, &DEFAULT_GATEWAY_IP)?;
                add_to_network_interfaces(e1000_interface);
                continue;
            }
            // here: check for and initialize other ethernet cards
        }

        warn!("Ignoring PCI device with no handler. {:?}", dev);
    }

    // Convenience notification for developers to inform them of no networking devices
    if network_manager::NETWORK_INTERFACES.lock().is_empty() {
        warn!("Note: no network devices found on this system.");
    }

    Ok(())
}
