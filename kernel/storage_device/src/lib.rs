//! Trait definitions for storage devices and storage controllers. 
//! 
//! All storage devices should implement the `StorageDevice` trait, 
//! such as hard disk drives, optical discs, SSDs, etc. 
//! All storage controllers should implement the `StorageController` trait,
//! such as AHCI controllers and IDE controllers.
//! 
//! Below is a quick example of how to iterate through available `StorageDevice`s on the system
//! and usage of the `StorageDevice` trait methods, as well as how to downcast it
//! into a specific concrete type, such as an `AtaDrive`.
//! ```rust
//! if let Some(controller) = storage_manager::storage_controllers().next() {
//!     if let Some(sd) = controller.lock().devices().next() {
//!         {
//!             // Call `StorageDevice` trait methods directly
//!             let mut locked_sd = sd.lock();
//!             debug!("Found drive with size {}, {} sectors", locked_sd.size_in_bytes(), locked_sd.size_in_blocks());
//! 
//!             // Here we downcast the `StorageDevice` into an `AtaDrive` so we can call `AtaDrive` methods.
//!             if let Some(ata_drive) = locked_sd.as_any_mut().downcast_mut() {
//!                 debug!("      drive was master? {}", ata_drive.is_master());
//! 
//!                 // Read 10 sectors from the beginning of the drive (at offset 0)
//!                 let mut initial_buf: [u8; 5120] = [0; 5120]; // 10 sectors of bytes
//!                 let sectors_read = ata_drive.read_pio(&mut initial_buf[..], 0).unwrap();
//!                 debug!("[SUCCESS] sectors_read: {:?}", sectors_read);
//!                 debug!("{:?}", core::str::from_utf8(&initial_buf));
//! 
//!                 // Write 3 sectors to the drive
//!                 let mut write_buf = [0u8; 512*3];
//!                 for b in write_buf.chunks_exact_mut(16) {
//!                     b.copy_from_slice(b"QWERTYUIOPASDFJK");
//!                 }
//!                 let bytes_written = ata_drive.write_pio(&write_buf[..], 2).unwrap();
//!                 debug!("WRITE_PIO {:?}", bytes_written);
//!             }
//!         }
//!         // Read 10 sectors from the drive using the `StorageDevice` trait methods.
//!         let mut after_buf: [u8; 5120] = [0; 5120];
//!         let sectors_read = sd.lock().read_blocks(&mut after_buf[..], 0).unwrap();
//!         debug!("{:X?}", &after_buf[..]);
//!         debug!("{:?}", core::str::from_utf8(&after_buf));
//!         trace!("POST-WRITE READ_BLOCKS {} sectors", sectors_read);
//!     }
//! }
//! ```
//! 
//! # Limitations
//! 
//! Note that if other crates are using a storage device through a block cache, 
//! the cache does not own the storage device and cache validity is questionable.
//! Be careful in that case. See the warning in the [block_cache](../block_cache/index.html) module.

#![no_std]

extern crate alloc;
extern crate spin;
#[macro_use] extern crate downcast_rs;
extern crate io;

use alloc::{
    boxed::Box,
    sync::Arc,
};
use spin::Mutex;
use downcast_rs::Downcast;
use io::{BlockIo, KnownLength, BlockReader, BlockWriter};


/// A trait that represents a storage controller,
/// such as an AHCI controller or IDE controller.
pub trait StorageController {
    /// Returns an iterator of references to all `StorageDevice`s attached to this controller.
    /// The lifetime of the iterator and the device references it returns are both bound
    /// by the lifetime of this `StorageController`.
    /// 
    /// # Note
    /// I would prefer to have this Iterator return a `&StorageDeviceRef`,
    /// but Rust does not permit casts from `&Arc<Mutex<Struct>>` to `&Arc<Mutex<Trait>>`,
    /// it only supports casts from `Arc<Mutex<Struct>>` to `Arc<Mutex<Trait>>`.
    fn devices<'c>(&'c self) -> Box<(dyn Iterator<Item = StorageDeviceRef> + 'c)>;
}

/// A trait object wrapped in an Arc and Mutex that allows 
/// arbitrary storage controllers to be shared in a thread-safe manner.
pub type StorageControllerRef = Arc<Mutex<dyn StorageController + Send>>;


/// A trait that represents a storage device,
/// such as hard disks, removable drives, SSDs, etc.
/// 
/// A `StorageDevice` must implement the following traits:
/// * `BlockIo`: to specify its block size.
/// * `BlockReader` and `BlockWriter`: to enable reading and writing data 
///   from/to the device at block granularity. 
/// * `KnownLength`: to specify the size in bytes (length) of the entire device. 
///
/// This trait includes additional functions to query device info, e.g.,
/// the device's total size in number of blocks.
pub trait StorageDevice: BlockIo + BlockReader + BlockWriter + KnownLength + Downcast {
	/// Returns the total size of this device, given in number of blocks (sectors).
    fn size_in_blocks(&self) -> usize;
}
impl_downcast!(StorageDevice);

/// A trait object wrapped in an Arc and Mutex that allows 
/// arbitrary storage devices to be shared in a thread-safe manner.
pub type StorageDeviceRef = Arc<Mutex<dyn StorageDevice + Send>>;
