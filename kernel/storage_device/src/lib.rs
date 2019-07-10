//! Trait definitions for storage devices and storage controllers. 
//! 
//! All storage devices should implement the `StorageDevice` trait, 
//! such as hard disk drives, optical discs, SSDs, etc. 
//! All storage controllers should implement the `StorageController` trait,
//! such as AHCI controllers and IDE controllers.

#![no_std]

extern crate alloc;
extern crate spin;

use alloc::{
    boxed::Box,
    sync::Arc,
};
use spin::Mutex;


/// A trait that represents a storage controller within Theseus,
/// such as an AHCI controller or IDE controller.
/// 
/// TODO FIXME: document these methods
pub trait StorageController {
    /// Returns an iterator of references to all `StorageDevice`s attached to this controller.
    fn devices<'c>(&'c self) -> Box<(dyn Iterator<Item = &'c dyn StorageDevice> + 'c)>;

    // /// Returns an iterator of mutable references to all `StorageDevice`s attached to this controller.
    // fn devices_mut(&mut self) -> &Iterator<Item = &mut StorageDevice>;
}

/// A trait object wrapped in an Arc and Mutex that allows 
/// arbitrary storage controllers to be shared in a thread-safe manner.
pub type StorageControllerRef = Arc<Mutex<dyn StorageController + Send>>;


/// A trait that represents a storage device within Theseus,
/// such as hard disks, removable drives, SSDs, etc.
/// 
/// TODO FIXME: document these methods
pub trait StorageDevice {

    fn read_sector(&mut self, buffer: &mut [u8], offset_in_sectors: usize) -> Result<usize, &'static str>;

    fn write_sector(&mut self, buffer: &[u8], offset_in_sectors: usize) -> Result<usize, &'static str>;

	/// Returns the size of a single sector in bytes, as defined by this drive.
    fn sector_size_in_bytes(&self) -> usize; 

	/// Returns the number of sectors in this drive.
    fn size_in_sectors(&self) -> usize;
    
    /// Returns the size of this drive in bytes,
	/// rounded up to the nearest sector size.
    fn size_in_bytes(&self) -> usize {
        self.sector_size_in_bytes() * self.size_in_sectors()
    }
}

/// A trait object wrapped in an Arc and Mutex that allows 
/// arbitrary storage devices to be shared in a thread-safe manner.
pub type StorageDeviceRef = Arc<Mutex<dyn StorageDevice + Send>>;
