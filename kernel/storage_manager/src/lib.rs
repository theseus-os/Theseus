//! Manages and handles initialization of all storage devices
//! and storage controllers in the system.

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
extern crate spin;
extern crate owning_ref;
extern crate pci;
extern crate ata;
extern crate storage_device;

use alloc::{
    vec::Vec,
    sync::Arc,
};
use spin::Mutex;
use pci::PciDevice;
use storage_device::StorageControllerRef;

pub use storage_device::*;


lazy_static! {
    /// A list of all of the available and initialized storage controllers that exist on this system.
    static ref STORAGE_CONTROLLERS: Mutex<Vec<StorageControllerRef>> = Mutex::new(Vec::new());
}

/// Returns an iterator over all initialized storage controllers on this system.
/// 
/// This function requires allocation, as it currently clones the list of storage controllers,\
/// effectively a `Vec<Arc<StorageController>>`.
pub fn storage_controllers() -> impl Iterator<Item = StorageControllerRef> {
    STORAGE_CONTROLLERS.lock().clone().into_iter()
}

/// Returns an iterator over all storage devices attached to the storage controllers on this system.
///
/// This function requires allocation, as it currently clones the list of storage devices (lazily)
/// within each storage controller, effectively a `Vec<Arc<Vec<Arc<StorageDevice>>>>`.
pub fn storage_devices() -> impl Iterator<Item = StorageDeviceRef> {
    storage_controllers()
        .flat_map(|c| c.lock()
            .devices()
            .collect::<Vec<StorageDeviceRef>>()
            .into_iter()
    )
}


/// Attempts to handle the initialization of the given `PciDevice`,
/// if it is a recognized storage device.
/// 
/// Returns `Ok(true)` if successful, 
/// `Ok(false)` if the given `PciDevice` isn't a supported storage device,
/// and an error if it fails to initialize a supported storage device.
pub fn init_device(pci_device: &PciDevice) -> Result<bool, &'static str> {
    // We currently only support IDE controllers for ATA drives (aka PATA).
    if pci_device.class == 0x01 && pci_device.subclass == 0x01 {
        info!("IDE controller PCI device found at: {:?}", pci_device.location);
        let ide_controller = ata::IdeController::new(pci_device)?;
        STORAGE_CONTROLLERS.lock().push(Arc::new(Mutex::new(ide_controller)));
        return Ok(true);
    }

    // Here: in the future, handle other supported storage devices

    Ok(false)
}
