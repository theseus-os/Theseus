#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
extern crate spin;
extern crate owning_ref;
extern crate pci;
extern crate ata;

use alloc::vec::Vec;
use alloc::sync::Arc;
use spin::Mutex;
use pci::PciDevice;


lazy_static! {
    /// A list of all of the available and initialized storage controllers that exist on this system.
    pub static ref STORAGE_CONTROLLERS: Mutex<Vec<StorageControllerRef>> = Mutex::new(Vec::new());
}

/// A trait that represents a storage controller within Theseus,
/// such as an AHCI controller or IDE controller.
/// 
/// TODO FIXME: document these methods
pub trait StorageController {
    /// Returns an iterator of references to all `StorageDevice`s attached to this controller.
    fn devices(&self) -> &Iterator<Item = &StorageDevice>;
    /// Returns an iterator of mutable references to all `StorageDevice`s attached to this controller.
    fn devices_mut(&mut self) -> &Iterator<Item = &mut StorageDevice>;
}

/// A trait object wrapped in an Arc and Mutex that allows 
/// arbitrary storage controllers to be shared in a thread-safe manner.
pub type StorageControllerRef = Arc<Mutex<StorageController + Send>>;


/// A trait that represents a storage device within Theseus,
/// such as hard disks, removable drives, SSDs, etc.
/// 
/// TODO FIXME: document these methods
pub trait StorageDevice {

    fn read_sector(&mut self, buffer: &mut [u8], offset_in_sectors: usize) -> Result<usize, &'static str>;

    fn write_sector(&mut self, buffer: &[u8], offset_in_sectors: usize) -> Result<usize, &'static str>;

    fn sector_size_in_bytes(&self) -> usize; 

    fn size_in_sectors(&self) -> usize;
    
    fn size_in_bytes(&self) -> usize {
        self.sector_size_in_bytes() * self.size_in_sectors()
    }
}


/// A trait object wrapped in an Arc and Mutex that allows 
/// arbitrary storage devices to be shared in a thread-safe manner.
pub type StorageDeviceRef = Arc<Mutex<StorageDevice + Send>>;


/// Attempts to handle the initialization of the given `PciDevice`,
/// if it is a recognized storage device.
/// 
/// Returns `Ok(true)` if successful, 
/// `Ok(false)` if the given `PciDevice` isn't a supported storage device,
/// and an error upon failure.
pub fn init_device(pci_device: &PciDevice) -> Result<bool, &'static str> {
    // We currently only support IDE controllers for ATA drives (aka PATA).
    if pci_device.class == 0x01 && pci_device.subclass == 0x01 {
        info!("IDE controller PCI device found at: {:?}", pci_device.location);
        let _ide_controller = ata::IdeController::new(pci_device)?;
        // TODO: do something with the discovered ATA drives / IDE controller
        // debug!("{:?}", ide_controller.primary_master);
        return Ok(true);
    }

    // Here: in the future, handle other supported storage devices

    Ok(false)
}