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
extern crate ixgbe;
extern crate alloc;
extern crate fatfs;
extern crate block_io;

use mpmc::Queue;
use event_types::Event;
use memory::MemoryManagementInfo;
use ethernet_smoltcp_device::EthernetNetworkInterface;
use network_manager::add_to_network_interfaces;
use alloc::vec::Vec;
use storage_manager::StorageDeviceRef;
use block_io::BlockIo;

/// A randomly chosen IP address that must be outside of the DHCP range.
/// TODO: use DHCP to acquire an IP address.
const DEFAULT_LOCAL_IP: &'static str = "10.0.2.15/24"; // the default QEMU user-slirp network gives IP addresses of "10.0.2.*"

/// Standard home router address.
/// TODO: use DHCP to acquire gateway IP
const DEFAULT_GATEWAY_IP: [u8; 4] = [10, 0, 2, 2]; // the default QEMU user-slirp networking gateway IP

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
        debug!("Found pci device: {:X?}", dev);
    } 

    // store all the initialized ixgbe NICs here to be added to the network interface list
    let mut ixgbe_devs = Vec::new();

    // Iterate over all PCI devices and initialize the drivers for the devices we support.

    for dev in pci::pci_device_iter() {
        // Currently we skip Bridge devices, since we have no use for them yet. 
        if dev.class == 0x06 {
            continue;
        }

        // If this is a storage device, initialize it as such.
        match storage_manager::init_device(dev) {
            // Successfully initialized this storage device.
            Ok(Some(_storage_controller)) => continue,

            // Not a storage device, so fall through and let another handler deal with it.
            Ok(None) => { }
            
            // Error initializing this device, so skip it.
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
            if dev.vendor_id == ixgbe::INTEL_VEND && dev.device_id == ixgbe::INTEL_82599 {
                info!("ixgbe PCI device found at: {:?}", dev.location);
                
                // Initialization parameters of the NIC.
                // These can be changed according to the requirements specified in the ixgbe init function.
                const VIRT_ENABLED: bool = true;
                const RSS_ENABLED: bool = false;
                const RX_DESCS: u16 = 8;
                const TX_DESCS: u16 = 8;
                
                let ixgbe_nic = ixgbe::IxgbeNic::init(
                    dev, 
                    dev.location,
                    ixgbe::LinkSpeedMbps::LS10000, 
                    VIRT_ENABLED, 
                    None, 
                    RSS_ENABLED, 
                    ixgbe::RxBufferSizeKiB::Buffer8KiB,
                    RX_DESCS,
                    TX_DESCS
                )?;

                ixgbe_devs.push(ixgbe_nic);
                continue;
            }

            // here: check for and initialize other ethernet cards
        }

        warn!("Ignoring PCI device with no handler. {:X?}", dev);
    }

    // Once all the NICs have been initialized, we can store them and add them to the list of network interfaces.
    let ixgbe_nics = ixgbe::IXGBE_NICS.call_once(|| ixgbe_devs);
    for ixgbe_nic_ref in ixgbe_nics.iter() {
        let ixgbe_interface = EthernetNetworkInterface::new_ipv4_interface(
            ixgbe_nic_ref, 
            DEFAULT_LOCAL_IP, 
            &DEFAULT_GATEWAY_IP
        )?;
        add_to_network_interfaces(ixgbe_interface);
    }

    // Convenience notification for developers to inform them of no networking devices
    if network_manager::NETWORK_INTERFACES.lock().is_empty() {
        warn!("Note: no network devices found on this system.");
    }

    // Discover filesystems from each storage device on the storage controllers initialized above
    // and mount each filesystem to the root directory by default.
    for storage_controller in storage_manager::STORAGE_CONTROLLERS.lock().iter() {
        for storage_device in storage_controller.lock().devices() {
            let disk = FatFsStorageDisk {
                block_io: BlockIo::new(storage_device),
                offset: 0,
            };
            fatfs::FileSystem::new(disk, fatfs::FsOptions::new())
        }
    }

    Ok(())
}



/// An adapter that implements traits required by the `fatfs` crate
/// for any `StorageDevice` that wants to be usable by `fatfs`.
struct FatFsStorageDisk {
    block_io: BlockIo,
    offset: usize,
}

impl fatfs::Read for FatFsStorageDisk {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let bytes_read = self.block_io.read(buf, self.offset)?;
        self.offset += bytes_read;
    }
}

impl fatfs::Write for FatFsStorageDisk {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        let bytes_written = self.block_io.write(buf, self.offset)?;
        self.offset += bytes_written;
    }
}

use fatfs::SeekFrom;
impl fatfs::Seek for FatFsStorageDisk {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        match pos {
            SeekFrom::Start(new_pos) => self.offset = new_pos,
            SeekFrom::Current(addend) => self.offset += addend,
            SeekFrom::End(addend) => self.offset = self.block_io.block_size().size_in_bytes + addend,
        }
    }
}


// TODO: when `StorageDevice` returns better error types (e.g., an enum),
//       we'll need to implement this `IoError` trait for those error types.
impl fatfs::IoError for &str {
    fn is_interrupted(&self) -> bool {
        false // no concept of interrupted storage device requests in Theseus yet
    }

    fn new_unexpected_eof_error() -> Self {
        "Read operation hit an unexpected EOF (end of file)"
    }

    fn new_write_zero_error() -> Self {
        "Write operation failed to write more than zero bytes"
    }
}
impl fatfs::IoBase for FatFsStorageDisk {
    type Error = &'static str;
}
