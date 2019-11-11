//! Wrappers for converting block I/O operations from one block size to another.
//! 
//! For example, these wrappers can expose a storage device that transfers 512-byte blocks at a time
//! as a device that can transfer arbitrary bytes at a time (as little as one byte). 
//! 
//! Furthermore, these reads and writes are cached using the `block_cache` crate.
//! 
//! # Limitations
//! Currently, the `BlockIo` struct is hardcoded to use a `StorageDevice` reference,
//! when in reality it should just use anything that implements traits like `BlockReader + BlockWriter`. 
//! 
//! The read and write functions are implemented such that if the backing storage device
//! needs to be accessed, it is done so by transferring only one block at a time. 
//! This is quite inefficient, and we should instead transfer multiple blocks at once. 

#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate storage_device;
extern crate block_cache;

use alloc::vec::Vec;
use storage_device::{StorageDeviceRef, BlockBounds};
use block_cache::{BlockCache};

/// A wrapper around a `StorageDevice` that supports reads and writes of arbitrary byte lengths
/// (down to a single byte) by issuing commands to the underlying storage device.
/// This is needed because most storage devices only allow reads/writes of larger blocks, 
/// e.g., a 512-byte sector or 4KB cluster.  
/// 
/// It also contains a cache for the blocks in the backing storage device,
/// in order to improve performance by avoiding actual storage device access.
pub struct BlockIo {
    /// The cache of blocks (sectors) read from the storage device,
    /// a map from sector number to data byte array.
    cache: BlockCache, 
    /// The underlying storage device from where the blocks are read/written.
    device: StorageDeviceRef,
}
impl BlockIo {
    /// Creates a new `BlockIo` device 
    pub fn new(storage_device: StorageDeviceRef) -> BlockIo {
        BlockIo {
            cache: BlockCache::new(),
            device: storage_device,
        }
    }

    /// Reads data from this block storage device and places it into the provided `buffer`.
    /// The length of the given `buffer` determines the maximum number of bytes to be read.
	/// 
	/// Returns the number of bytes that were successfully read from the drive
	/// and copied into the given `buffer`.
    /// 
    /// The read blocks will be cached in this `BlockIo` struct to accelerate future storage device access.
    pub fn read(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, &'static str> {
        let mut locked_device = self.device.lock();
        let BlockBounds { range, first_block_offset, .. } = locked_device.block_bounds(offset, buffer.len())?;
        let block_size_in_bytes = locked_device.sector_size_in_bytes();

        // Read the actual data, one block at a time.
		let mut src_offset = first_block_offset; 
		let mut dest_offset = 0;
		for block_num in range {
			// don't copy past the end of `buffer`
			let num_bytes_to_copy = core::cmp::min(block_size_in_bytes - src_offset, buffer.len() - dest_offset);
            let block_bytes = BlockCache::read_block(&mut self.cache, &mut *locked_device, block_num)?;
			buffer[dest_offset .. (dest_offset + num_bytes_to_copy)].copy_from_slice(&block_bytes[src_offset .. (src_offset + num_bytes_to_copy)]);
			trace!("BlockIo::read(): for block {}, copied bytes into buffer[{}..{}] from block[{}..{}]",
				block_num, dest_offset, dest_offset + num_bytes_to_copy, src_offset, src_offset + num_bytes_to_copy,
			);
			dest_offset += num_bytes_to_copy;
			src_offset = 0;
		}

        Ok(dest_offset)
    }

    /// Write data from the given `buffer` into this block storage device starting at the given `offset` in bytes.
    /// The length of the given `buffer` determines the maximum number of bytes to be written.
	/// 
	/// Returns the number of bytes that were successfully written to the storage device.
    /// 
    /// The written blocks will be cached in this `BlockIo` struct to accelerate future storage device access.
    /// Currently, we use a *write-through* cache policy,
    /// in which the blocks are written directly to the cache and the backing storage device immediately.
    pub fn write(&mut self, buffer: &[u8], offset: usize) -> Result<usize, &'static str> {
        let mut locked_device = self.device.lock();
        let block_bounds = locked_device.block_bounds(offset, buffer.len())?;
        let block_size_in_bytes = locked_device.sector_size_in_bytes();

        // // A write transfer (and a read too) can be broken down into three parts: 
        // // (1) Beginning: the first block, which may only be partially included in the transfer.
        // // (2) Middle: the second block to the second-to-last block, which will be full blocks.
        // // (3) End: the last block, which might be partially covered by the byte buffer.
        // // Only the middle blocks can be blindly written to without first reading the existing blocks' contents.
        // // If the first block or last block is aligned to a block boundary, it can be handled as a middle block.
		let mut src_offset = 0;
		let mut dest_offset = block_bounds.first_block_offset;

		for block_num in block_bounds.range {
			let num_bytes_to_copy = core::cmp::min(block_size_in_bytes - dest_offset, buffer.len() - src_offset);

            let buffer_to_write = if num_bytes_to_copy == block_size_in_bytes {
                // We're overwriting the entire block, so no need to read it first
                (&buffer[src_offset .. (src_offset + num_bytes_to_copy)]).to_vec()
            } else {
                // We're only partially writing to this block, so we need to read the old block first.
                let old_block = BlockCache::read_block(&mut self.cache, &mut *locked_device, block_num)?;
                let mut new_block_contents = old_block.to_vec();
                let overwrite_offset = dest_offset % block_size_in_bytes;
                new_block_contents[overwrite_offset .. (overwrite_offset + num_bytes_to_copy)]
                    .copy_from_slice(&buffer[src_offset .. (src_offset + num_bytes_to_copy)]);
                new_block_contents
            };

            self.cache.write_block(&mut *locked_device, block_num, buffer_to_write)?;
            trace!("BlockIo::write(): for block {}, copied bytes from buffer[{}..{}] to block[{}..{}]",
				block_num, src_offset, src_offset + num_bytes_to_copy, dest_offset, dest_offset + num_bytes_to_copy,
			);

			src_offset += num_bytes_to_copy;
			dest_offset = 0;
		}

        Ok(src_offset)
    }

    /// Flushes the given block to the backing storage device. 
    /// If the `block_to_flush` is None, all blocks in the entire cache
    /// will be written back to the storage device.
    pub fn flush(&mut self, block_num: Option<usize>) -> Result<(), &'static str> {
        let mut locked_device = self.device.lock();
        self.cache.flush(&mut *locked_device, block_num)
    }
}