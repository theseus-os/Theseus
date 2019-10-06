//! Wrappers for converting block I/O operations from one block size to another.
//! 
//! For example, these wrappers can expose a storage device that transfers 512-byte blocks at a time
//! as a device that can transfer arbitrary bytes at a time (as little as one byte). 
//! 
//! # Limitations
//! Currently, the `BlockIo` struct is hardcoded to use a `StorageDevice` reference,
//! when in reality it should just use anything that implements traits like `BlockReader + BlockWriter`. 
//! 
//! The read and write functions are implemented such that if the backing storage device
//! needs to be accessed, it is done so by transferring only one block at a time. 
//! This is quite inefficient, and we should instead transfer multiple blocks at once. 
//! 
//! Cached blocks are stored as vectors of bytes on the heap, 
//! we should do something else such as separate mapped regions. 
//! Cached blocks cannot yet be dropped to relieve memory pressure. 

#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate hashbrown;
extern crate storage_device;

use alloc::vec::Vec;
use hashbrown::{
    HashMap,
    hash_map::Entry,
};
use storage_device::{StorageDevice};

/// A wrapper around a `StorageDevice` that supports reads and writes of arbitrary byte lengths
/// (down to a single byte) by issuing commands to the underlying storage device.
/// This is needed because most storage devices only allow reads/writes of larger blocks, 
/// e.g., a 512-byte sector or 4KB cluster.  
/// 
/// It also contains a cache for the blocks in the backing storage device,
/// in order to improve performance by avoiding actual storage device access.
pub struct BlockCache {
    /// The cache of blocks (sectors) read from the storage device,
    /// a map from sector number to data byte array.
    cache: InternalCache,
}

impl BlockCache {
    /// Creates a new `BlockIo` device 
    pub fn new() -> BlockCache {
        BlockCache {
            cache: HashMap::new(),
        }
    }

    /// Flushes the given block to the backing storage device. 
    /// If the `block_to_flush` is None, all blocks in the entire cache
    /// will be written back to the storage device.
    pub fn flush(&mut self, locked_device: &mut dyn StorageDevice, block_num: Option<usize>) -> Result<(), &'static str> {
        if let Some(bn) = block_num {
            // Flush just one block
            if let Some(cached_block) = self.cache.get_mut(&bn) {
                Self::flush_block(&mut *locked_device, bn, cached_block)?;
            }
            // If the block wasn't in the cache, do nothing.
        }
        else {
            // Flush all blocks
            for (bn, cached_block) in self.cache.iter_mut() {
                Self::flush_block(&mut *locked_device, *bn, cached_block)?;
            }
        }
        Ok(())
    }

    /// An internal function that first checks the cache for a specific block
    /// in order to avoid reading from the storage device.
    /// If that block exists in the cache, it is copied into the buffer. 
    /// If not, it is read from the storage device into the cache, and then copied into the buffer.
    pub fn read_block<'c>(cache: &'c mut BlockCache, locked_device: &mut dyn StorageDevice, block: usize) -> Result<&'c [u8], &'static str> {
        match cache.cache.entry(block) {
            Entry::Occupied(occ) => {
                // An existing entry in the cache can be used directly (without going to the backing store)
                // if it's in the `Modified` or `Shared` state.
                // But if it's in the `Invalid` state, we have to re-read the block from the storage device.
                let cached_block = occ.into_mut();
                match cached_block.state {
                    CacheState::Modified | CacheState::Shared => Ok(&cached_block.block),
                    CacheState::Invalid => {
                        locked_device.read_sectors(&mut cached_block.block, block)?;
                        cached_block.state = CacheState::Shared;
                        Ok(&cached_block.block)
                    }
                }
            }
            Entry::Vacant(vacant) => {
                // A vacant entry will be read from the backing storage device,
                // so it will always start out in the `Shared` state.
                let mut v = vec![0; locked_device.sector_size_in_bytes()];
                locked_device.read_sectors(&mut v, block)?;
                let cb = CachedBlock {
                    block: v,
                    state: CacheState::Shared,
                };
                let cached_block = vacant.insert(cb);
                Ok(&cached_block.block)
            }
        }
    }

    pub fn write_block(&mut self, locked_device: &mut dyn StorageDevice, block_num: usize, buffer_to_write: Vec<u8>) 
        -> Result<(), &'static str> {
        
            let mut new_cached_block = CachedBlock {
                block: buffer_to_write,
                state: CacheState::Modified,
            };
            // Currently using a write-through policy right now, so flush the block immediately
            BlockCache::flush_block(&mut *locked_device, block_num, &mut new_cached_block)?;
            self.cache.insert(block_num, new_cached_block);

            Ok(())
    }

    /// An internal function that writes out the given `cached_block`
    /// to the given locked `StorageDevice` if the cached block is in the `Modified` state.
    fn flush_block(locked_device: &mut dyn StorageDevice, block_num: usize, cached_block: &mut CachedBlock) -> Result<(), &'static str> {
        // we only need to actually write blocks in the `Modified` state.
        match cached_block.state {
            CacheState::Shared | CacheState::Invalid => { },
            CacheState::Modified => {
                locked_device.write_sectors(&cached_block.block, block_num)?;
                cached_block.state = CacheState::Shared;
            }
        }
        Ok(())
    }
}



/// A block from a storage device stored in a cache.
/// This currently includes the actual owned cached content as a vector of bytes on the heap,
/// in addition to the `CacheState` of the cached item.
/// 
/// TODO: allow non-dirty blocks to be freed (reclaimed) upon memory pressure. 
#[derive(Debug)]
struct CachedBlock { // Not sure if this should be public, but it seems necessary to fix type leak. TODO make non-public.
    block: Vec<u8>,
    state: CacheState,
}

type InternalCache = HashMap<usize, CachedBlock>;


/// The states of an item in the cache, following the MSI cache coherence protocol.
#[derive(Debug)]
#[allow(dead_code)]
enum CacheState {
    /// Dirty: the cached item has been modified more recently than the backing store,
    /// so it must be flushed at a future time to guarantee data correctness and consistency.
    /// A `Modified` cached item **cannot** be safely dropped from the cache.
    /// A `Modified` cached item can be safely read from or overwritten without going to the backing store.  
    Modified,
    /// Clean: the cached item and the backing store are in sync; they have the same value.
    /// A `Shared` cached item can be safely dropped from the cache.
    Shared,
    /// The cached item is out-of-date and should not be read from,
    /// as the backing storage has a more recent copy than the cache.
    /// Therefore, if a read of an `Invalid` cached item is requested,
    /// it must be re-read from the backing storage.
    /// An `Invalid` item can still be overwritten in the cache without going to the backing store. 
    /// An `Invalid` item can be safely dropped from the cache.
    Invalid,  
}
