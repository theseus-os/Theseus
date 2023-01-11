//! A caching layer for block based storage devices.
//! 
//! For many storage devices, calls to the backing medium are quite expensive. This layer intends to reduce those calls,
//! improving efficiency in exchange for additional memory usage. Note that this crate is intended to be used as a part of
//! `block_io`, but should work on its own.
//! 
//! # Limitations
//! Currently, the `BlockCache` struct is hardcoded to use a `StorageDevice` reference,
//! when in reality it should just use anything that implements traits like `BlockReader + BlockWriter`. 
//! 
//! The read and write functions currently only support reading/writing individual blocks from disk even
//! when the caller might prefer reading a larger number of contiguous blocks. This is inneficient and an
//! optimized implementation should read multiple blocks at once if possible.
//! 
//! Cached blocks are stored as vectors of bytes on the heap, 
//! we should do something else such as separate mapped regions. 
//! Cached blocks cannot yet be dropped to relieve memory pressure. 
//! 
//! Note that this cache only holds a reference to the underlying block device.
//! As such if any other system crates perform writes to the underlying device,
//! in that case the cache will give incorrect and potentially inconsistent results.
//! In the long run, the only way around this would be to only expose a `BlockCache`
//! instead of exposing a `StorageDevice`. I suppose the least disruptive way to implement this
//! might be with a layer of indirection to an implementor of a BlockReader like trait,
//! so that the underlying block reader can be switched out when a cache is enabled.

#![no_std]

#[macro_use] extern crate alloc;
extern crate hashbrown;
extern crate storage_device;

use alloc::vec::Vec;
use hashbrown::{
    HashMap,
    hash_map::Entry,
};
use storage_device::{StorageDevice, StorageDeviceRef};
use alloc::borrow::{Cow, ToOwned};

/// A cache to store read and written blocks from a storage device.
pub struct BlockCache {
    /// The cache of blocks (sectors) read from the storage device,
    /// a map from block number to data byte array.
    cache: InternalCache,
    /// The underlying storage device from where the blocks are read/written.
    storage_device: StorageDeviceRef,
}

impl BlockCache {
    /// Creates a new `BlockCache` device 
    pub fn new(storage_device: StorageDeviceRef) -> BlockCache {
        BlockCache {
            cache: HashMap::new(),
            storage_device,
        }
    }

    /// Flushes the given block to the backing storage device. 
    /// If the `block_to_flush` is None, all blocks in the entire cache
    /// will be written back to the storage device.
    pub fn flush(&mut self, block_num: Option<usize>) -> Result<(), &'static str> {
        let mut locked_device = self.storage_device.lock();
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
    pub fn read_block(cache: &mut BlockCache, block: usize) -> Result<&[u8], &'static str> {
        let mut locked_device = cache.storage_device.lock();
        match cache.cache.entry(block) {
            Entry::Occupied(occ) => {
                // An existing entry in the cache can be used directly (without going to the backing store)
                // if it's in the `Modified` or `Shared` state.
                // But if it's in the `Invalid` state, we have to re-read the block from the storage device.
                let cached_block = occ.into_mut();
                match cached_block.state {
                    CacheState::Modified | CacheState::Shared => Ok(&cached_block.block),
                    CacheState::Invalid => {
                        locked_device.read_blocks(&mut cached_block.block, block)?;
                        cached_block.state = CacheState::Shared;
                        Ok(&cached_block.block)
                    }
                }
            }
            Entry::Vacant(vacant) => {
                // A vacant entry will be read from the backing storage device,
                // so it will always start out in the `Shared` state.
                let mut v = vec![0; locked_device.block_size()];
                locked_device.read_blocks(&mut v, block)?;
                let cb = CachedBlock {
                    block: v,
                    state: CacheState::Shared,
                };
                let cached_block = vacant.insert(cb);
                Ok(&cached_block.block)
            }
        }
    }

    pub fn write_block(&mut self, block_num: usize, buffer_to_write: Cow<[u8]>)
    //pub fn write_block(&mut self, block_num: usize, buffer_to_write: Cow<[u8]>)
        -> Result<(), &'static str> 
        {
            let mut locked_device = self.storage_device.lock();

            let owned_buffer: Vec<u8> = match buffer_to_write {
                Cow::Borrowed(slice_ref) => slice_ref.to_owned(),
                Cow::Owned(vec_owned) => vec_owned, 
            };

            let mut new_cached_block = CachedBlock {
                block: owned_buffer,
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
                locked_device.write_blocks(&cached_block.block, block_num)?;
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
