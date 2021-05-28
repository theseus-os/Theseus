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
    pub static ref STORAGE_CONTROLLERS: Mutex<Vec<StorageControllerRef>> = Mutex::new(Vec::new());
}


/// Attempts to handle the initialization of the given `PciDevice`,
/// if it is a recognized storage device.
/// 
/// # Return
/// * `Ok(Some(StorageControllerRef))` if successful, containing the newly-initialized storage controller.
/// * `Ok(None)` if the given `PciDevice` isn't a supported storage device,
/// * An error if it fails to initialize a supported storage device.
pub fn init_device(pci_device: &PciDevice) -> Result<Option<StorageControllerRef>, &'static str> {
    // We currently only support IDE controllers for ATA drives (aka PATA).
    let storage_controller = if pci_device.class == 0x01 && pci_device.subclass == 0x01 {
        info!("IDE controller PCI device found at: {:?}", pci_device.location);
        let ide_controller = ata::IdeController::new(pci_device)?;
        let storage_controller_ref: StorageControllerRef = Arc::new(Mutex::new(ide_controller));
        STORAGE_CONTROLLERS.lock().push(Arc::clone(&storage_controller_ref));
        Some(storage_controller_ref)
    } 
    // Here: in the future, handle other supported storage devices
    else {
        None
    };
    
    Ok(storage_controller)
}