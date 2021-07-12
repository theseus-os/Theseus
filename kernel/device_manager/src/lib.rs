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
extern crate bare_io;
#[macro_use] extern crate derive_more;

use mpmc::Queue;
use event_types::Event;
use memory::MemoryManagementInfo;
use ethernet_smoltcp_device::EthernetNetworkInterface;
use network_manager::add_to_network_interfaces;
use alloc::vec::Vec;
use block_io::{ByteReaderWriterWrapper, LockedIo, ReaderWriter, Reader, Writer};

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
    for storage_device in storage_manager::storage_devices() {
        let disk = FatFsAdapter(
            ReaderWriter::new(
                ByteReaderWriterWrapper::from(
                    LockedIo::from(storage_device)
                )
            ),
        );

        let filesystem = fatfs::FileSystem::new(disk, fatfs::FsOptions::new()).unwrap();
        debug!("FATFS data:
            fat_type: {:?},
            volume_id: {:X?},
            volume_label: {:?},
            cluster_size: {:?},
            status_flags: {:?},
            stats: {:?}",
            filesystem.fat_type(),
            filesystem.volume_id(),
            filesystem.volume_label(),
            filesystem.cluster_size(),
            filesystem.read_status_flags(),
            filesystem.stats(),
        );

        let indent = "    ";
        let root = filesystem.root_dir();
        debug!("Root directory contents:");
        for f in root.iter() {
            debug!("\t {:#X?}", f.map(|entry| (entry.file_name(), entry.attributes(), entry.len())));
        }

    }

    Ok(())
}



/// An adapter (wrapper type) that implements traits required by the [`fatfs`] crate
/// for any I/O device that wants to be usable by [`fatfs`].
///
/// To meet [`fatfs`]'s requirements, the underlying I/O stream must be able to 
/// read, write, and seek while tracking its current offset. 
/// We use traits from the [`bare_io`] crate to meet these requirements, 
/// thus, the given `IO` parameter must implement those [`bare_io`] traits.
///
/// For example, this allows one to access a FAT filesystem 
/// by reading from or writing to a storage device.
pub struct FatFsAdapter<IO>(IO);
impl<IO> FatFsAdapter<IO> {
    pub fn new(io: IO) -> FatFsAdapter<IO> { FatFsAdapter(io) }
}
/// This tells the `fatfs` crate that our read/write/seek functions
/// may return errors of the type [`FatFsIoErrorAdapter`],
/// which is a simple wrapper around [`bare_io::Error`].
impl<IO> fatfs::IoBase for FatFsAdapter<IO> {
    type Error = FatFsIoErrorAdapter;
}
impl<IO> fatfs::Read for FatFsAdapter<IO> where IO: bare_io::Read {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buf).map_err(Into::into)
    }
}
impl<IO> fatfs::Write for FatFsAdapter<IO> where IO: bare_io::Write {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.0.write(buf).map_err(Into::into)
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        self.0.flush().map_err(Into::into)
    }
}
impl<IO> fatfs::Seek for FatFsAdapter<IO> where IO: bare_io::Seek {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, Self::Error> {
        let bare_io_pos = match pos {
            fatfs::SeekFrom::Start(s)   => bare_io::SeekFrom::Start(s),
            fatfs::SeekFrom::Current(c) => bare_io::SeekFrom::Current(c),
            fatfs::SeekFrom::End(e)     => bare_io::SeekFrom::End(e),
        };
        self.0.seek(bare_io_pos).map_err(Into::into)
    }
}

/// This struct exists so we can implement the [`fatfs::IoError`] trait
/// for the [`bare_io::Error`] trait (albeit indirectly).
/// 
/// This is required because Rust prevents implementing foreign traits for foreign types.
#[derive(Debug, From, Into)]
pub struct FatFsIoErrorAdapter(bare_io::Error);
impl fatfs::IoError for FatFsIoErrorAdapter {
    fn is_interrupted(&self) -> bool {
        self.0.kind() == bare_io::ErrorKind::Interrupted
    }

    fn new_unexpected_eof_error() -> Self {
        FatFsIoErrorAdapter(bare_io::ErrorKind::UnexpectedEof.into())
    }

    fn new_write_zero_error() -> Self {
        FatFsIoErrorAdapter(bare_io::ErrorKind::WriteZero.into())
    }
}
