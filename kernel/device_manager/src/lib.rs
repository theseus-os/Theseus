#![no_std]
#![cfg_attr(target_arch = "x86_64", feature(trait_alias))]

extern crate alloc;

use log::info;

#[cfg(target_arch = "x86_64")]
use {
    log::{error, debug, warn},
    mpmc::Queue,
    event_types::Event,
    memory::MemoryManagementInfo,
    alloc::vec::Vec,
    io::{ByteReaderWriterWrapper, LockableIo, ReaderWriter},
    storage_manager::StorageDevice,
    memory::PhysicalAddress,
    serial_port::{SerialPortAddress, init_serial_port, take_serial_port_basic},
};

/// Performs early-stage initialization for simple devices needed during early boot.
///
/// This includes:
/// * local APICs ([`apic`]),
/// * [`acpi`] tables for system configuration info, including the IOAPIC.
#[cfg(target_arch = "x86_64")]
pub fn early_init(
    rsdp_address: Option<PhysicalAddress>,
    kernel_mmi: &mut MemoryManagementInfo
) -> Result<(), &'static str> {
    // First, initialize the local APIC hardware such that we can populate
    // and initialize each LocalAPIC discovered in the ACPI table initialization routine below.
    apic::init();
    
    // Then, parse the ACPI tables to acquire system configuration info.
    acpi::init(rsdp_address, &mut kernel_mmi.page_table)?;

    Ok(())
}


/// Initializes all other devices not initialized during [`early_init()`]. 
///
/// Devices include:
/// * At least one [`serial_port`] (e.g., `COM1`) with full interrupt support,
/// * The fully-featured system [`logger`],
/// * The legacy PS2 controller and any connected devices: [`keyboard`] and [`mouse`],
/// * All other devices discovered on the [`pci`] bus.
pub fn init(
    #[cfg(target_arch = "x86_64")]
    key_producer: Queue<Event>,
    #[cfg(target_arch = "x86_64")]
    mouse_producer: Queue<Event>,
) -> Result<(), &'static str>  {

    let serial_ports = logger::take_early_log_writers();
    let logger_writers = IntoIterator::into_iter(serial_ports)
        .flatten()
        .filter_map(|sp| serial_port::init_serial_port(sp.base_port_address(), sp))
        .cloned();

    logger::init(None, logger_writers);
    info!("Initialized full logger.");

    // COM1 is the only UART on aarch64; it's used for logging as well as for the console.
    #[cfg(target_arch = "x86_64")] {
        // Ensure that both COM1 and COM2 are initialized, for logging and/or headless operation.
        // If a serial port was used for logging (as configured in [`logger::early_init()`]),
        // ignore its inputs for purposes of starting new console instances.
        let init_serial_port = |spa: SerialPortAddress| {
            if let Some(sp) = take_serial_port_basic(spa) {
                init_serial_port(spa, sp);
            } else {
                console::ignore_serial_port_input(spa as u16);
                info!("Ignoring input on {:?} because it is being used for logging.", spa);
            }
        };
        init_serial_port(SerialPortAddress::COM1);
        init_serial_port(SerialPortAddress::COM2);
    }

    // PS/2 is x86_64 only
    #[cfg(target_arch = "x86_64")] {
        let ps2_controller = ps2::init()?;
        keyboard::init(ps2_controller.keyboard_ref(), key_producer)?;
        mouse::init(ps2_controller.mouse_ref(), mouse_producer)?;
    }

    // No PCI support on aarch64 at the moment
    #[cfg(target_arch = "x86_64")] {
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
                let nic = e1000::E1000Nic::init(dev)?;
                let interface = net::register_device(nic);
                nic.lock().init_interrupts(interface)?;

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
                    VIRT_ENABLED, 
                    None, 
                    RSS_ENABLED, 
                    ixgbe::RxBufferSizeKiB::Buffer2KiB,
                    RX_DESCS,
                    TX_DESCS
                )?;

                ixgbe_devs.push(ixgbe_nic);
                continue;
            }
            if dev.vendor_id == mlx5::MLX_VEND && (dev.device_id == mlx5::CONNECTX5_DEV || dev.device_id == mlx5::CONNECTX5_EX_DEV) {
                info!("mlx5 PCI device found at: {:?}", dev.location);
                const RX_DESCS: usize = 512;
                const TX_DESCS: usize = 8192;
                const MAX_MTU:  u16 = 9000;

                mlx5::ConnectX5Nic::init(dev, TX_DESCS, RX_DESCS, MAX_MTU)?;
                continue;
            }

            // here: check for and initialize other ethernet cards
        }

        warn!("Ignoring PCI device with no handler. {:X?}", dev);
    }

    // Once all the NICs have been initialized, we can store them and add them to the list of network interfaces.
    let ixgbe_nics = ixgbe::IXGBE_NICS.call_once(|| ixgbe_devs);
    for ixgbe_nic_ref in ixgbe_nics.iter() {
        net::register_device(ixgbe_nic_ref);
    }

    // Convenience notification for developers to inform them of no networking devices
    if net::get_default_interface().is_none() {
        warn!("Note: no network devices found on this system.");
    }

    // Discover filesystems from each storage device on the storage controllers initialized above
    // and mount each filesystem to the root directory by default.
    if false {
        for storage_device in storage_manager::storage_devices() {
            let disk = fatfs_adapter::FatFsAdapter::new(
                ReaderWriter::new(
                    ByteReaderWriterWrapper::from(
                        LockableIo::<dyn StorageDevice + Send, spin::Mutex<_>, _>::from(storage_device)
                    )
                ),
            );

            if let Ok(filesystem) = fatfs::FileSystem::new(disk, fatfs::FsOptions::new()) {
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

                let root = filesystem.root_dir();
                debug!("Root directory contents:");
                for f in root.iter() {
                    debug!("\t {:X?}", f.map(|entry| (entry.file_name(), entry.attributes(), entry.len())));
                }
            }
        }
    }
    }

    Ok(())
}

#[cfg(target_arch = "x86_64")]
mod fatfs_adapter {
// TODO: move the following `FatFsAdapter` stuff into a separate crate. 

use derive_more::{From, Into};

/// An adapter (wrapper type) that implements traits required by the [`fatfs`] crate
/// for any I/O device that wants to be usable by [`fatfs`].
///
/// To meet [`fatfs`]'s requirements, the underlying I/O stream must be able to 
/// read, write, and seek while tracking its current offset. 
/// We use traits from the [`core2`] crate to meet these requirements, 
/// thus, the given `IO` parameter must implement those [`core2`] traits.
///
/// For example, this allows one to access a FAT filesystem 
/// by reading from or writing to a storage device.
pub struct FatFsAdapter<IO>(IO);
impl<IO> FatFsAdapter<IO> {
    pub fn new(io: IO) -> FatFsAdapter<IO> { FatFsAdapter(io) }
}
/// This tells the `fatfs` crate that our read/write/seek functions
/// may return errors of the type [`FatFsIoErrorAdapter`],
/// which is a simple wrapper around [`core2::io::Error`].
impl<IO> fatfs::IoBase for FatFsAdapter<IO> {
    type Error = FatFsIoErrorAdapter;
}
impl<IO> fatfs::Read for FatFsAdapter<IO> where IO: core2::io::Read {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buf).map_err(Into::into)
    }
}
impl<IO> fatfs::Write for FatFsAdapter<IO> where IO: core2::io::Write {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.0.write(buf).map_err(Into::into)
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        self.0.flush().map_err(Into::into)
    }
}
impl<IO> fatfs::Seek for FatFsAdapter<IO> where IO: core2::io::Seek {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, Self::Error> {
        let core2_pos = match pos {
            fatfs::SeekFrom::Start(s)   => core2::io::SeekFrom::Start(s),
            fatfs::SeekFrom::Current(c) => core2::io::SeekFrom::Current(c),
            fatfs::SeekFrom::End(e)     => core2::io::SeekFrom::End(e),
        };
        self.0.seek(core2_pos).map_err(Into::into)
    }
}

/// This struct exists to enable us to implement the [`fatfs::IoError`] trait
/// for the [`core2::io::Error`] trait.
/// 
/// This is required because Rust prevents implementing foreign traits for foreign types.
#[derive(Debug, From, Into)]
pub struct FatFsIoErrorAdapter(core2::io::Error);
impl fatfs::IoError for FatFsIoErrorAdapter {
    fn is_interrupted(&self) -> bool {
        self.0.kind() == core2::io::ErrorKind::Interrupted
    }
    fn new_unexpected_eof_error() -> Self {
        FatFsIoErrorAdapter(core2::io::ErrorKind::UnexpectedEof.into())
    }
    fn new_write_zero_error() -> Self {
        FatFsIoErrorAdapter(core2::io::ErrorKind::WriteZero.into())
    }
}
}
