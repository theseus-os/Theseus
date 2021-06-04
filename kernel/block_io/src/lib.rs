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

use core::ops::Range;
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