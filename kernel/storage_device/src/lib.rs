//! Trait definitions for storage devices and storage controllers. 
//! 
//! All storage devices should implement the `StorageDevice` trait, 
//! such as hard disk drives, optical discs, SSDs, etc. 
//! All storage controllers should implement the `StorageController` trait,
//! such as AHCI controllers and IDE controllers.

#![no_std]

extern crate alloc;
extern crate spin;
#[macro_use] extern crate downcast_rs;

use alloc::{
    boxed::Box,
    sync::Arc,
};
use spin::Mutex;
use downcast_rs::Downcast;


/// A trait that represents a storage controller,
/// such as an AHCI controller or IDE controller.
pub trait StorageController {
    /// Returns an iterator of references to all `StorageDevice`s attached to this controller.
    /// The lifetime of the iterator and the device references it returns are both bound
    /// by the lifetime of this `StorageController`.
    fn devices<'c>(&'c self) -> Box<(dyn Iterator<Item = &'c dyn StorageDevice> + 'c)>;
}

/// A trait object wrapped in an Arc and Mutex that allows 
/// arbitrary storage controllers to be shared in a thread-safe manner.
pub type StorageControllerRef = Arc<Mutex<dyn StorageController + Send>>;


/// A trait that represents a storage device,
/// such as hard disks, removable drives, SSDs, etc.
/// 
/// It offers functions to read and write at sector granularity,
/// as well as basic functions to query device info.
pub trait StorageDevice: Downcast {
    /// Reads content from the storage device into the given `buffer`.
    /// 
    /// # Arguments 
    /// * `buffer`: the destination buffer. The length of the `buffer` governs how many sectors are read, 
    ///   and must be an even multiple of the storage device's [`sector size`](#method.sector_size_in_bytes). 
    /// 
    /// * `offset_in_sectors`: an absolute offset from the beginning of the storage device, given in number of sectors. 
    ///   This is sometimes referred to as the starting logical block address (LBA).
    /// 
    /// Returns the number of sectors (*not bytes*) read into the `buffer`.
    fn read_sectors(&mut self, buffer: &mut [u8], offset_in_sectors: usize) -> Result<usize, &'static str>;

    /// Writes content from the given `buffer` to the storage device starting at the given `offset_in_sectors`.
    /// 
    /// # Arguments 
    /// * `buffer`: the source buffer. The length of the `buffer` governs how many sectors are written, 
    ///   and must be an even multiple of the storage device's [`sector size`](#method.sector_size_in_bytes). 
    /// 
    /// * `offset_in_sectors`: an absolute offset from the beginning of the storage device, given in number of sectors. 
    ///   This is sometimes referred to as the starting logical block address (LBA).
    /// 
    /// Returns the number of sectors (*not bytes*) written to the storage device.
    fn write_sectors(&mut self, buffer: &[u8], offset_in_sectors: usize) -> Result<usize, &'static str>;

	/// Returns the size of a single sector in bytes, as defined by this drive.
    fn sector_size_in_bytes(&self) -> usize; 

	/// Returns the number of sectors in this drive.
    fn size_in_sectors(&self) -> usize;
    
    /// Returns the size of this drive in bytes, rounded up to the nearest sector size.
    /// This is nothing more than [`sector_size_in_bytes()`](#tymethod.sector_size_in_bytes)` * `[`size_in_sectors()`](#tymethod.size_in_sectors).
    fn size_in_bytes(&self) -> usize {
        self.sector_size_in_bytes() * self.size_in_sectors()
    }
}
impl_downcast!(StorageDevice);

/// A trait object wrapped in an Arc and Mutex that allows 
/// arbitrary storage devices to be shared in a thread-safe manner.
pub type StorageDeviceRef = Arc<Mutex<dyn StorageDevice + Send>>;
