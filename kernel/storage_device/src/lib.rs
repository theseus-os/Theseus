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
//! if let Some(controller) = storage_manager::STORAGE_CONTROLLERS.lock().iter().next() {
//!     if let Some(sd) = controller.lock().devices().next() {
//!         {
//!             // Call `StorageDevice` trait methods directly
//!             let mut locked_sd = sd.lock();
//!             debug!("Found drive with size {}, {} sectors", locked_sd.size_in_bytes(), locked_sd.size_in_sectors());
//! 
//!             // Here we downcast the `StorageDevice` into an `AtaDrive` so we can call `AtaDrive` methods.
//!             if let Some(ata_drive) = locked_sd.as_any_mut().downcast_mut() {
//!                 debug!("      drive was master? {}", ata_drive.is_master());
//! 
//!                 // Read 10 sectors from the drive
//!                 let mut initial_buf: [u8; 5120] = [0; 5120]; // 10 sectors of bytes
//!                 let sectors_read = ata_drive.read_pio(&mut initial_buf[..], 0);
//!                 debug!("[SUCCESS] sectors_read: {:?}", sectors_read);
//!                 debug!("{:?}", core::str::from_utf8(&initial_buf));
//! 
//!                 // Write 3 sectors to the drive
//!                 let mut write_buf = [0u8; 512*3];
//!                 for b in write_buf.chunks_exact_mut(16) {
//!                     b.copy_from_slice(b"QWERTYUIOPASDFJK");
//!                 }
//!                 let bytes_written = ata_drive.write_pio(&write_buf[..], 2);
//!                 debug!("WRITE_PIO {:?}", bytes_written);
//!             }
//!         }
//!         // Read 10 sectors from the drive using the `StorageDevice` trait methods.
//!         let mut after_buf: [u8; 5120] = [0; 5120];
//!         let sectors_read = sd.lock().read_sectors(&mut after_buf[..], 0)?;
//!         debug!("{:X?}", &after_buf[..]);
//!         debug!("{:?}", core::str::from_utf8(&after_buf));
//!         trace!("POST-WRITE READ_SECTORS {} sectors", sectors_read);
//!     }
//! }
//! ```

#![no_std]

extern crate alloc;
extern crate spin;
#[macro_use] extern crate downcast_rs;

use core::ops::Range;
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
/// It offers functions to read and write at sector granularity,
/// as well as basic functions to query device info.
pub trait StorageDevice: Downcast {
    /// Reads content from the storage device into the given `buffer`.
    /// 
    /// # Arguments 
    /// * `buffer`: the destination buffer. The length of the `buffer` governs how many sectors are read, 
    ///   and must be an even multiple of the storage device's [`sector size`](#tymethod.sector_size_in_bytes). 
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
    ///   and must be an even multiple of the storage device's [`sector size`](#tymethod.sector_size_in_bytes). 
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
    
    /// Calculates block-wise bounds based on a byte-wise offset and buffer length. 
    /// 
    /// # Arguments
    /// * `offset`: the absolute byte offset from the beginning of the block storage device
    ///    at which the read/write starts.
    /// * `length`: the number of bytes to be read/written.
    /// 
    /// # Return
    /// Returns a `BlockBounds` object.
    /// 
    /// If `offset + length` extends past the bounds, the `Range` will be truncated
    /// to the last block of the storage device (all the way to the end), 
    /// and the `last_block_offset` will be `0`.
    /// 
    /// Returns an error if the `offset` extends past the bounds of this storage device.
    fn block_bounds(&self, offset: usize, length: usize) -> Result<BlockBounds, &'static str> {
        let block_size_in_bytes = self.sector_size_in_bytes();
        if offset > self.size_in_bytes() {
            return Err("offset was out of bounds");
        }
        let first_block = offset / block_size_in_bytes;
        let first_block_offset = offset % block_size_in_bytes;
        let last_block = core::cmp::min(
            self.size_in_sectors(),
            (offset + length + block_size_in_bytes - 1) / block_size_in_bytes, // round up to next sector
        );
        let last_block_offset = (first_block_offset + length) % block_size_in_bytes;
        // trace!("block_bounds: offset: {}, length: {}, first_block: {}, last_block: {}",
        //     offset, length, first_block, last_block
        // );
        Ok(BlockBounds {
            range: first_block..last_block,
            first_block_offset,
            last_block_offset,
        })
    }
}
impl_downcast!(StorageDevice);

/// A trait object wrapped in an Arc and Mutex that allows 
/// arbitrary storage devices to be shared in a thread-safe manner.
pub type StorageDeviceRef = Arc<Mutex<dyn StorageDevice + Send>>;


/// Block-wise bounds information for a data transfer (read or write)
/// that has been calculated from a byte-wise offset and buffer length. 
/// 
/// See the [`block_bounds()`](trait.StorageDevice.html#method.block_bounds) method.
pub struct BlockBounds {
    /// A `Range` from the first block to the last block of the transfer.
    pub range: Range<usize>,
    /// The offset into the first block (beginning bound) where the transfer starts.
    pub first_block_offset: usize,
    /// The offset into the last block (ending bound) where the transfer ends.
    pub last_block_offset: usize,
}
impl BlockBounds {
    /// The total number of blocks to be transferred, i.e., `last block - first block`.
    pub fn block_count(&self) -> usize {
        self.range.end - self.range.start
    }

    /// Returns true if the first block of the transfer is aligned to a block boundary.
    pub fn is_first_block_aligned(&self) -> bool {
        self.first_block_offset == 0
    }

    /// Returns true if the last block of the transfer is aligned to a block boundary.
    pub fn is_last_block_aligned(&self) -> bool {
        self.last_block_offset == 0
    }
}
