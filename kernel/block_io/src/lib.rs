//! Wrappers for converting block I/O operations from one block size to another.
//! 
//! For example, these wrappers can expose a storage device that transfers 512-byte blocks at a time
//! as a device that can transfer arbitrary bytes at a time (as little as one byte). 
//! 
//! Furthermore, these reads and writes are cached using the `block_cache` crate.
//! 
//! # Limitations
//! Currently, the `BlockIoWrapper` struct is hardcoded to use a `StorageDevice` reference,
//! when in reality it should just use anything that implements traits like `BlockReader + BlockWriter`. 
//! 
//! The read and write functions are implemented such that if the backing storage device
//! needs to be accessed, it is done so by transferring only one block at a time. 
//! This is quite inefficient, and we should instead transfer multiple blocks at once. 

#![no_std]

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
#[macro_use] extern crate derive_more;
extern crate storage_device;
extern crate block_cache;
extern crate bare_io;

use core::{
    cmp::{min, max},
    ops::Range,
};
use alloc::vec::Vec;
use storage_device::StorageDeviceRef;
use block_cache::BlockCache;


/// The set of errors that can be returned from I/O operations.
pub enum IoError {
    /// An input parameter or argument was incorrect or invalid.
    InvalidInput,
    /// The I/O operation attempted to access data beyond the bounds of this I/O stream.
    OutOfBounds,
    /// The I/O operationg timed out and was canceled.
    TimedOut,
}

impl From<IoError> for bare_io::Error {
    fn from(io_error: IoError) -> Self {
        use bare_io::{ErrorKind, Error};
        match io_error {
            IoError::InvalidInput => ErrorKind::InvalidInput.into(),
            IoError::OutOfBounds  => Error::new(ErrorKind::Other, "out of bounds"),
            IoError::TimedOut     => ErrorKind::TimedOut.into(),
        }
    }
}


/// A parent trait that is used to specify the block size (in bytes)
/// of I/O transfers (read and write operations). 
/// See its use in `BlockReader` and `BlockWriter`.
pub trait BlockIo {
    /// The size in bytes of a block transferred during an I/O operation.
    const BLOCK_SIZE: usize;
}


/// A trait that represents an I/O stream (e.g., an I/O device) that can be read from in block-size chunks.
/// The granularity of each transfer is given by the `BLOCK_SIZE` constant.
///
/// A `BlockReader` is not aware of the current block offset into the stream;
/// thus, each read operation requires a starting offset: 
/// the number of blocks from the beginning of the I/O stream at which the read should start.
pub trait BlockReader: BlockIo {
    /// Reads blocks of data from this reader into the given `buffer`.
    ///
    /// The number of blocks read is dictated by the length of the given `buffer`.
    ///
    /// # Arguments
    /// * `buffer`: the buffer into which data will be copied. 
    ///    The length of this buffer must be a multiple of `BLOCK_SIZE`.
    /// * `block_offset`: the offset in number of blocks from the beginning of this reader.
    ///
    /// # Return
    /// If successful, returns the number of blocks read into the given `buffer`. 
    /// Otherwise, returns an error.
    fn read_blocks(&mut self, buffer: &mut [u8], block_offset: usize) -> Result<usize, IoError>;
}


/// A trait that represents an I/O stream (e.g., an I/O device) that can be written to in block-size chunks. 
/// The granularity of each transfer is given by the `BLOCK_SIZE` constant.
///
/// A `BlockWriter` is not aware of the current block offset into the stream;
/// thus, each write operation requires a starting offset: 
/// the number of blocks from the beginning of the I/O stream at which the write should start.
pub trait BlockWriter: BlockIo {
    /// Writes blocks of data from the given `buffer` to this writer.
    ///
    /// The number of blocks written is dictated by the length of the given `buffer`.
    ///
    /// # Arguments
    /// * `buffer`: the buffer from which data will be copied. 
    ///    The length of this buffer must be a multiple of `BLOCK_SIZE`.
    /// * `block_offset`: the offset in number of blocks from the beginning of this writer.
    ///
    /// # Return
    /// If successful, returns the number of blocks written to this writer. 
    /// Otherwise, returns an error.
    fn write_blocks(&mut self, buffer: &[u8], block_offset: usize) -> Result<usize, IoError>;

    /// Flushes this entire writer's output stream, 
    /// ensuring all contents in intermediate buffers are fully written out. 
    fn flush(&mut self) -> Result<(), IoError>;
}


/// A trait that represents an I/O stream that can be read from,
/// but which does not track the current offset into the stream.
///
/// This trait is auto-implemented for any type that implements the `BlockReader` trait,
/// allowing it to be easily wrapped around a `BlockReader` for byte-wise access to a block-based I/O stream.
pub trait ByteReader {
    /// Reads bytes of data from this reader into the given `buffer`.
    ///
    /// The number of bytes read is dictated by the length of the given `buffer`.
    ///
    /// # Arguments
    /// * `buffer`: the buffer into which data will be copied.
    /// * `offset`: the offset in bytes from the beginning of this reader where the read operation begins.
    ///
    /// # Return
    /// If successful, returns the number of bytes read into the given `buffer`. 
    /// Otherwise, returns an error.
    fn read(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, IoError>; 
}

impl<R: BlockReader> ByteReader for R {
    fn read(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, IoError> {

        let mut total_bytes_read = 0;

        let mut block_to_read = offset / R::BLOCK_SIZE;
        let offset_into_first_block = offset % R::BLOCK_SIZE;

        let end_offset = offset + buffer.len();
        let last_block = end_offset / R::BLOCK_SIZE;
        let offset_into_last_block = end_offset % R::BLOCK_SIZE;
        

        // There are three possible ranges of bytes that we need to read from the block reader:
        // 1. The leading partial block, before the beginning of the first whole block. in which the byte-wise offset is not aligned on a block boundary.
        // 2. The first whole block through the end of the last whole block.
        // 3. The trailing partial block, after the end of the last whole block in the byte offset  end of the last full block 
        // 
        // We avoid the need to copy between intermediate buffers by issuing up to 3 `read_block()` operations.
        // We issue the reads sequentially in ascending order to hopefully preserve locality of memory access
        // and accessing (seeking) the underlying I/O stream.

        let mut src_offset  = offset_into_first_block;
        let mut dest_offset = 0;

        let mut tmp_block_bytes: Vec<u8> = Vec::new();

        if offset_into_first_block != 0 {
            // The offset is NOT block-aligned, so we need to perform an initial read of the first block separately.
            if tmp_block_bytes.is_empty() {
                tmp_block_bytes = vec![0; R::BLOCK_SIZE];
            }
            let _blocks_read = self.read_blocks(&mut tmp_block_bytes, block_to_read)?;
            let num_bytes_to_copy = R::BLOCK_SIZE - offset_into_first_block;
            let dest_range = dest_offset .. (dest_offset + num_bytes_to_copy);
            let src_range  =  src_offset .. (src_offset  + num_bytes_to_copy);
            buffer[dest_range].copy_from_slice(&tmp_block_bytes[src_range]);
            dest_offset += num_bytes_to_copy;
            total_bytes_read += num_bytes_to_copy;
            block_to_read += 1;
        }

        let last_contiguous_block = if offset_into_last_block == 0 {
            // The end offset IS block-aligned, so we can simply perform one more read that covers all remaining blocks.
            last_block
        } else {
            // The end offset is NOT block-aligned, so we must perform a final read of the last block separately.
            last_block - 1
        };
        // Perform the read of contiguously whole blocks. 
        {
            let num_bytes_to_copy = R::BLOCK_SIZE * (last_contiguous_block - block_to_read);
            let dest_range = dest_offset .. (dest_offset + num_bytes_to_copy);
            let blocks_read = self.read_blocks(&mut buffer[dest_range], block_to_read)?;
            total_bytes_read += blocks_read * R::BLOCK_SIZE;
            dest_offset += num_bytes_to_copy;
            block_to_read = last_contiguous_block;
        }
        

        
        if offset_into_last_block != 0 {
            // The end offset is NOT block-aligned, so we must perform a final read of the last block separately.
            if tmp_block_bytes.is_empty() {
                tmp_block_bytes = vec![0; R::BLOCK_SIZE];
            }
            let _blocks_read = self.read_blocks(&mut tmp_block_bytes, block_to_read)?;
            let num_bytes_to_copy = offset_into_last_block;
            let dest_range = dest_offset .. (dest_offset + num_bytes_to_copy);
            let src_range  =  src_offset .. (src_offset  + num_bytes_to_copy);
            buffer[dest_range].copy_from_slice(&tmp_block_bytes[src_range]);
            dest_offset += num_bytes_to_copy;
            total_bytes_read += num_bytes_to_copy;
        }

        Ok(total_bytes_read)
    }
}

/// A trait that represents an I/O stream that can be written to,
/// but which does not track the current offset into the stream.
///
/// This trait is auto-implemented for any type that implements the `BlockWriter` trait,
/// allowing it to be easily wrapped around a `BlockWriter` for byte-wise access to a block-based I/O stream.
pub trait ByteWriter {
    /// Writes bytes of data from the given `buffer` to this writer.
    ///
    /// The number of bytes written is dictated by the length of the given `buffer`.
    ///
    /// # Arguments
    /// * `buffer`: the buffer from which data will be copied. 
    /// * `offset`: the offset in number of bytes from the beginning of this writer.
    ///
    /// # Return
    /// If successful, returns the number of bytes written to this writer. 
    /// Otherwise, returns an error.
    fn write(&mut self, buffer: &[u8], offset: usize) -> Result<usize, IoError>;

    /// Flushes this entire writer's output stream, 
    /// ensuring all contents in intermediate buffers are fully written out.
    fn flush(&mut self) -> Result<(), IoError>;
}
impl<W: BlockWriter> ByteWriter for W {
    fn write(&mut self, buffer: &[u8], offset: usize) -> Result<usize, IoError> {
        todo!()
    }

    fn flush(&mut self) -> Result<(), IoError> {
        self.flush()
    }
}


/// A stateful reader that keeps track of its current offset
/// within the internal stateless `ByteReader` I/O stream.
#[derive(Deref, DerefMut)]
pub struct Reader<R: ByteReader>(IoWithOffset<R>);
impl<R: ByteReader> Reader<R> {
    /// Creates a new Reader with an initial offset of 0.
    pub fn new(reader: R) -> Self {
        Reader(IoWithOffset { io: reader, offset: 0 })
    }
}

/// A stateful writer that keeps track of its current offset
/// within the internal stateless `ByteWriter` I/O stream.
#[derive(Deref, DerefMut)]
pub struct Writer<W: ByteWriter>(IoWithOffset<W>);
impl<W: ByteWriter> Writer<W> {
    /// Creates a new Writer with an initial offset of 0.
    pub fn new(writer: W) -> Self {
        Writer(IoWithOffset { io: writer, offset: 0 })
    }
}

/// A stateful reader and writer that keeps track of its current offset
/// within the internal stateless `ByteReader + ByteWriter` I/O stream.
#[derive(Deref, DerefMut)]
pub struct ReaderWriter<RW: ByteReader + ByteWriter>(IoWithOffset<RW>);
impl<RW: ByteReader + ByteWriter> ReaderWriter<RW> {
    /// Creates a new ReaderWriter with an initial offset of 0.
    pub fn new(reader_writer: RW) -> Self {
        ReaderWriter(IoWithOffset { io: reader_writer, offset: 0 })
    }
}

/// A stateful I/O stream (reader, writer, or both) that keeps track
/// of its current offset within its internal stateless I/O stream.
///
/// Hint: don't use this type directly, use its wrapper types:
///`Reader`, `Writer`, or `ReaderWriter`.
pub struct IoWithOffset<IO> {
    io: IO,
    offset: usize,
}
impl<IO> IoWithOffset<IO> {
    /// Creates a new IO stream with an initial offset of 0.
    pub fn new(io: IO) -> Self {
        IoWithOffset { io, offset: 0 }
    }
}
impl<IO: ByteReader> bare_io::Read for IoWithOffset<IO> {
    fn read(&mut self, buf: &mut [u8]) -> bare_io::Result<usize> {
        let bytes_read = self.io.read(buf, self.offset)
            .map_err(Into::<bare_io::Error>::into)?;
        self.offset += bytes_read;
        Ok(bytes_read)
    }
}
impl<IO: ByteWriter> bare_io::Write for IoWithOffset<IO> {
    fn write(&mut self, buf: &[u8]) -> bare_io::Result<usize> {
        let bytes_written = self.io.write(buf, self.offset)
            .map_err(Into::<bare_io::Error>::into)?;
        self.offset += bytes_written;
        Ok(bytes_written)
    }

    fn flush(&mut self) -> bare_io::Result<()> {
        self.io.flush().map_err(Into::into)
    }    
}





/// A wrapper around a `StorageDevice` that supports reads and writes of arbitrary byte lengths
/// (down to a single byte) by issuing commands to the underlying storage device.
/// This is needed because most storage devices only allow reads/writes of larger blocks, 
/// e.g., a 512-byte sector or 4KB cluster.  
/// 
/// It also contains a cache for the blocks in the backing storage device,
/// in order to improve performance by avoiding actual storage device access.
pub struct BlockIoWrapper {
    /// The cache of blocks (sectors) read from the storage device,
    /// a map from sector number to data byte array.
    cache: BlockCache, 
    block_size: BlockSize,
}
impl BlockIoWrapper {
    /// Creates a new `BlockIoWrapper` device 
    pub fn new(storage_device: StorageDeviceRef) -> BlockIoWrapper {
        let device_ref = storage_device.clone();
        let locked_device = device_ref.lock();
        BlockIoWrapper {
            cache: BlockCache::new(storage_device),
            block_size: BlockSize {
                size_in_bytes: locked_device.size_in_bytes(),
                size_in_blocks: locked_device.size_in_sectors(),
                block_size_in_bytes: locked_device.sector_size_in_bytes()
            },
        }
    }

    /// Reads data from this block storage device and places it into the provided `buffer`.
    /// The length of the given `buffer` determines the maximum number of bytes to be read.
	/// 
	/// Returns the number of bytes that were successfully read from the drive
	/// and copied into the given `buffer`.
    /// 
    /// The read blocks will be cached in this `BlockIoWrapper` struct to accelerate future storage device access.
    pub fn read(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, &'static str> {
        let BlockBounds { range, first_block_offset, .. } = BlockBounds::block_bounds(offset, buffer.len(), &self.block_size)?;
        let block_size_in_bytes = self.block_size.block_size_in_bytes;

        // Read the actual data, one block at a time.
		let mut src_offset = first_block_offset; 
		let mut dest_offset = 0;
		for block_num in range {
			// don't copy past the end of `buffer`
			let num_bytes_to_copy = core::cmp::min(block_size_in_bytes - src_offset, buffer.len() - dest_offset);
            let block_bytes = BlockCache::read_block(&mut self.cache, block_num)?;
			buffer[dest_offset .. (dest_offset + num_bytes_to_copy)].copy_from_slice(&block_bytes[src_offset .. (src_offset + num_bytes_to_copy)]);
			trace!("BlockIoWrapper::read(): for block {}, copied bytes into buffer[{}..{}] from block[{}..{}]",
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
    /// The written blocks will be cached in this `BlockIoWrapper` struct to accelerate future storage device access.
    /// Currently, we use a *write-through* cache policy,
    /// in which the blocks are written directly to the cache and the backing storage device immediately.
    pub fn write(&mut self, buffer: &[u8], offset: usize) -> Result<usize, &'static str> {

        let block_bounds = BlockBounds::block_bounds(offset, buffer.len(), &self.block_size)?;
        let block_size_in_bytes = self.block_size.block_size_in_bytes;

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
                let old_block = BlockCache::read_block(&mut self.cache, block_num)?;
                let mut new_block_contents = old_block.to_vec();
                let overwrite_offset = dest_offset % block_size_in_bytes;
                new_block_contents[overwrite_offset .. (overwrite_offset + num_bytes_to_copy)]
                    .copy_from_slice(&buffer[src_offset .. (src_offset + num_bytes_to_copy)]);
                new_block_contents
            };

            self.cache.write_block(block_num, buffer_to_write.into())?;
            trace!("BlockIoWrapper::write(): for block {}, copied bytes from buffer[{}..{}] to block[{}..{}]",
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
        self.cache.flush(block_num)
    }
}

/// Information used to track the size of a block device. Primarily used to construct `BlockBounds` objects.
pub struct BlockSize {
    /// Size of the block device in bytes (may not be a multiple of `block_size_in_bytes`)
    pub size_in_bytes: usize,
    /// Number of blocks in the device. Rounds up to include partial blocks.
    pub size_in_blocks: usize,
    /// Number of bytes per block.
    pub block_size_in_bytes: usize,
}

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

    /// Calculates block-wise bounds based on a byte-wise offset and buffer length for some block based device.
    /// 
    /// # Arguments
    /// * `offset`: the absolute byte offset from the beginning of the block storage device
    ///    at which the read/write starts.
    /// * `length`: the number of bytes to be read/written.
    /// * `block_size`: the relevant size information for the underlying block device.
    /// 
    /// # Return
    /// Returns a `BlockBounds` object.
    /// 
    /// If `offset + length` extends past the bounds, the `Range` will be truncated
    /// to the last block of the storage device (all the way to the end), 
    /// and the `last_block_offset` will be `0`.
    /// 
    /// Returns an error if the `offset` extends past the bounds of this block device.
    pub fn block_bounds(offset: usize, length: usize, block_size: &BlockSize) 
            -> Result<BlockBounds, &'static str> {
        
        let block_size_in_bytes = block_size.block_size_in_bytes;
        if offset > block_size.size_in_bytes {
            return Err("offset was out of bounds");
        }
        let first_block = offset / block_size_in_bytes;
        let first_block_offset = offset % block_size_in_bytes;
        let last_block = core::cmp::min(
            block_size.size_in_blocks,
            (offset + length + block_size_in_bytes - 1) / block_size_in_bytes, // round up to next sector
        );
        let last_block_offset = (first_block_offset + length) % block_size_in_bytes;
        trace!("block_bounds: offset: {}, length: {}, first_block: {}, last_block: {}",
            offset, length, first_block, last_block
        );
        Ok(BlockBounds {
            range: first_block..last_block,
            first_block_offset,
            last_block_offset,
        })
    }
}


#[cfg(test)] extern crate std;

/// Calculates block-wise bounds for an I/O transfer based on a byte-wise range into a block-wise stream.
/// 
/// There are up to three transfer operations that can possibly occur, depending on the alignment of the byte-wise range:
/// 1. A partial single-block transfer of some bytes in the first block, 
///    only if the start of `byte_range` is not aligned to `block_size`.
/// 2. A multi-block transfer of contiguous whole blocks, 
///    only if `byte_range` spans more than 2 blocks.  
/// 3. A partial single-block transfer of some bytes in the last block,
///    only if the end of `byte_range` is not aligned to `block_size`.
/// 
/// ## Example
/// Given a read request for a `byte_range` of `1500..3950` and a `block_size` of `512` bytes, we calculate:
/// 1. Read 1 block (block 2) and transfer the last 36 bytes of that block (`476..512`) into the byte range `1500..1536`.
/// 2. Read 4 blocks (blocks `3..7`) and transfer all of those 2048 bytes into the byte range `1536..3584`.
/// 3. Read 1 block (block 7) and transfer the first 366 bytes of that block (`0..366`) into the byte range `3584..3950`.
///
/// # Arguments
/// * `byte_range`: the absolute byte-wise range (from the beginning of the block-wise stream)
///    at which the I/O transfer starts and ends.
/// * `block_size`: the size in bytes of each block in the block-wise I/O stream.
/// 
/// # Return
/// Returns a list of the three above transfer operations, 
/// enclosed in `Option`s to convey that some may not be necessary.
/// 
pub fn blockwise_from_bytewise(
    byte_range: Range<usize>,
    block_size: usize
) -> [Option<BlockByteTransfer>; 3] {
    
    let mut transfers = [None, None, None];
    let mut transfer_idx = 0;

    let last_block = byte_range.end / block_size;
    let offset_into_last_block  = byte_range.end % block_size; 

    let mut curr_byte = byte_range.start;

    while curr_byte < byte_range.end {
        #[cfg(test)] ::std::println!("TRANSFERS: {:?}", transfers);

        let curr_block = curr_byte / block_size;
        let offset_into_curr_block = curr_byte % block_size;

        // If the curr_byte is block-aligned, then we can do a multi-block transfer.
        if offset_into_curr_block == 0 {
            // Determine what the last block of this transfer should be.
            // Special case: if the last byte is block-aligned, this transfer can cover all remaining bytes. 
            if offset_into_last_block == 0 {
                transfers[transfer_idx] = Some(BlockByteTransfer {
                    byte_range_absolute: curr_byte .. byte_range.end,
                    block_range: curr_block .. last_block,
                    bytes_in_block_range: 0 .. (last_block - curr_block) * block_size,
                });
                break; // this is the final transfer
            }
            // Otherwise, if the last byte is NOT block-aligned, this transfer can only extend up until the beginning of the last block 
            // (through the end of the second-to-last block).
            // Unless, that is, it's the final transfer because the end of the byte range is within the current block.  
            else {
                let end_byte = if byte_range.end - curr_byte > block_size {
                    round_down(byte_range.end, block_size) 
                } else {
                    byte_range.end
                };
                transfers[transfer_idx] = Some(BlockByteTransfer {
                    byte_range_absolute: curr_byte .. end_byte,
                    block_range: curr_block .. (round_up(end_byte, block_size) / block_size),
                    bytes_in_block_range: 0 .. (end_byte - curr_byte),
                });
                transfer_idx += 1;
                curr_byte = end_byte;
            }
        }
        // Otherwise, if the curr_byte is NOT block-aligned, then we can only do a single-block transfer.
        else {
            let end_byte = min(byte_range.end, round_up(curr_byte, block_size));
            transfers[transfer_idx] = Some(BlockByteTransfer {
                byte_range_absolute: curr_byte .. end_byte,
                block_range: curr_block .. curr_block + 1, // just one block
                bytes_in_block_range: offset_into_curr_block .. (offset_into_curr_block + (end_byte - curr_byte)),
            });
            transfer_idx += 1;
            curr_byte = end_byte;
        }

    }

    transfers
}

/// A test vector for `blockwise_from_bytewise()` where both the starting byte and ending byte
/// are not block-aligned.
#[test]
fn test_blockwise_bytewise_both_unaligned() {
    let transfers = blockwise_from_bytewise(1500..3950, 512);
    assert_eq!(transfers, [
        Some(BlockByteTransfer {
            byte_range_absolute: 1500..1536,
            block_range: 2..3,
            bytes_in_block_range: 476..512,
        }),
        Some(BlockByteTransfer {
            byte_range_absolute: 1536..3584,
            block_range: 3..7,
            bytes_in_block_range: 0..2048,
        }),
        Some(BlockByteTransfer {
            byte_range_absolute: 3584..3950,
            block_range: 7..8,
            bytes_in_block_range: 0..366,
        }),
    ]);
}



    /*
    
/// Calculates block-wise bounds for an I/O transfer based on a byte-wise range into a block-wise stream.
/// 
/// There are up to three transfer operations that can possibly occur, depending on the alignment of the byte-wise range:
/// 1. A partial single-block transfer of some bytes in the first block, 
///    only if the start of `byte_range` is not aligned to `block_size`.
/// 2. A multi-block transfer of contiguous whole blocks, 
///    only if `byte_range` spans more than 2 blocks.  
/// 3. A partial single-block transfer of some bytes in the last block,
///    only if the end of `byte_range` is not aligned to `block_size`.
/// 
/// ## Example
/// Given a read request for a `byte_range` of `1500..3950` and a `block_size` of `512` bytes, we calculate:
/// 1. Read 1 block (block 2) and transfer the last 36 bytes of that block (`476..512`) into the byte range `1500..1536`.
/// 2. Read 4 blocks (blocks `3..7`) and transfer all of those 2048 bytes into the byte range `1536..3584`.
/// 3. Read 1 block (block 7) and transfer the first 366 bytes of that block (`0..366`) into the byte range `3584..3950`.
///
/// # Arguments
/// * `byte_range`: the absolute byte-wise range (from the beginning of the block-wise stream)
///    at which the I/O transfer starts and ends.
/// * `block_size`: the size of each block in the block-wise I/O stream.
/// 
/// # Return
/// Returns a list of the three above transfer operations, 
/// enclosed in `Option`s to convey that some may not be necessary.
/// 
pub fn blockwise_from_bytewise(
    byte_range: Range<usize>,
    block_size: usize
) -> [Option<BlockByteTransfer>; 3] {

    let mut curr_byte = byte_range.start;
    let mut curr_block = curr_byte / block_size;

    let offset_into_first_block = byte_range.start % block_size;
    let offset_into_last_block  = byte_range.end % block_size; 

    let first_transfer = {

        let (end_byte, end_block) = if offset_into_first_block == 0 {
            if offset_into_last_block == 0 {
                // Both the start and end of the byte_range are block-aligned,
                // a special case in which we can cover the whole range with just one transfer.
                (byte_range.end, byte_range.end / block_size)
            } else {
                // The start is block-aligned, but the end is not. 
                // So we end this first transfer at the second-to-last block.
                let end_block = byte_range.end / block_size; // block range is exclusive
                (end_block * block_size, end_block)
            }
        } else {
            // The start is NOT block-aligned, so we do a single-block transfer to the end of this block. 
            (round_up(curr_block, block_size), curr_block + 1)
        };
        
        let ft = Some(BlockByteTransfer {
            byte_range_absolute: byte_range.start .. end_byte,
            block_range: curr_block .. end_block,
            bytes_in_block_range: offset_into_first_block .. block_size,
        });
        curr_byte = end_byte; 
        curr_block = end_block;
        ft
    };


    let second_transfer = 


    //////////////////////////


    let first_block = byte_range.start / block_size;
    let start_offset_into_first_block = byte_range.start % block_size;

    let first_block_map = if start_offset_into_first_block == 0 {
        None
    } else {
        Some(BlockByteTransfer {
            byte_range_absolute: byte_range.start .. round_up(byte_range.start, block_size),
            block_range: first_block .. first_block + 1,
            bytes_in_block_range: start_offset_into_first_block .. block_size,
        })
    };

    let last_block = byte_range.end / block_size;
    let end_offset_into_last_block = byte_range.end % block_size;
    let last_block_map = if end_offset_into_last_block == 0 {
        None
    } else {
        Some(BlockByteTransfer {
            byte_range_absolute: (last_block - 1) * block_size .. byte_range.end,
            block_range: last_block - 1 .. last_block,
            bytes_in_block_range: 0 .. end_offset_into_last_block,
        })
    };


    let middle_blocks_map = match (first_block_map, last_block_map) {
        (None, None) => {
            Some(BlockByteTransfer {
                byte_range_absolute: round_up(byte_range.start, block_size) .. 
                block_range: first_block + 1 .. last_block - 1,
                bytes_in_block_range: 0 .. end_offset_into_last_block,
            })
        }

    };
    
    else {
        Some(BlockByteTransfer {
            byte_range_absolute: round_up(byte_range.start, block_size) .. 
            block_range: first_block + 1 .. last_block - 1,
            bytes_in_block_range: 0 .. end_offset_into_last_block,
        })
    }




    [first_transfer, second_transfer, third_transfer]
}
*/


#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct BlockByteTransfer { // <const BLOCK_SIZE: usize> {
    /// The byte-wise range specified in absolute bytes from the beginning of an I/O stream.
    /// The size of this range should equal the size of `bytes_in_block_range`.
    pub byte_range_absolute: Range<usize>,
    /// The range of blocks to transfer.
    pub block_range: Range<usize>,
    /// The range of bytes relative to the blocks specified by `block_range`.
    /// The size of this range should equal the size of `byte_range_absolute`.
    ///
    /// For example, a range of `0..10` specifies that the first 10 bytes of the `block_range`
    /// are what should be transferred to/from the `byte_range_absolute`.
    pub bytes_in_block_range: Range<usize>,
}


/// Rounds the given `value` up to the nearest `multiple`.
#[inline]
pub fn round_up(value: usize, multiple: usize) -> usize {
    ((value + multiple - 1) / multiple) * multiple
}

/// Rounds the given `value` down to the nearest `multiple`.
#[inline]
pub fn round_down(value: usize, multiple: usize) -> usize {
    (value / multiple) * multiple
}
