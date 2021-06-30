//! Traits and types for expressing I/O transfers of both byte-wise and block-wise granularity.
//! 
//! The important items are summarized below:
//! * [`BlockReader`], [`BlockWriter`]: traits that represent I/O streams which can be read from
//!   or written to at the granularity of a single block.
//! * [`BlockIo`]: a parent trait that specifies the size in bytes of each block 
//!   in a block-based I/O stream.
//! * [`KnownLength`]: a trait that represents an I/O stream with a known length, 
//!   such as a disk drive.
//! * [`ByteReader`], [`ByteWriter`]: traits that represent I/O streams which can be read from
//!   or written to at the granularity of an individual bytes.
//! * Wrapper types that allow byte-granular I/O access atop block-based streams:
//!   [`ByteReaderWrapper`], [`ByteWriterWrapper`], [`ByteReaderWriterWrapper`].
//!    * Notably, the [`blocks_from_bytes()`] function is useful for calculating which
//!      block-based I/O transfers are needed to satisfy an arbitrary byte-wise transfer.
//!
//! Note that the above traits represent "stateless" access into I/O streams or devices,
//! in that successive read/write operations will not advance any kind of "offset".
//!
//! To read or write while tracking the current offset into the I/O stream, 
//! we provide the [`Reader`], [`Writer`], and [`ReaderWriter`] types. 
//! These types act as stateful wrappers around I/O streams that track the current offset 
//! into that stream, i.e., where the next read or write operation will start.
//!
//! For example, a storage device like a hard drive that transfers 512-byte blocks at a time
//! should implement `BlockReader`, `BlockWriter`, `BlockIo`, and `KnownLength` traits.
//! A user can then use those traits directly to transfer whole blocks to/from the device,
//! or wrap the storage device in one of the byte-wise reader/writer types 
//! in order to transfer arbitrary bytes (as little as one byte) at a time to/from the device. 
//!

#![no_std]

// #[macro_use] extern crate log;
#[macro_use] extern crate alloc;
#[macro_use] extern crate derive_more;
extern crate bare_io;

use core::{cmp::min, ops::Range};
use alloc::{
    boxed::Box,
    vec::Vec,
};
use bare_io::{Seek, SeekFrom};


/// Errors that can be returned from I/O operations.
#[derive(Debug)]
pub enum IoError {
    /// An input parameter or argument was incorrect or invalid.
    InvalidInput,
    /// The I/O operation attempted to access data beyond the bounds of this I/O stream.
    OutOfBounds,
    /// The I/O operation timed out and was canceled.
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

impl From<IoError> for &'static str {
    fn from(io_error: IoError) -> Self {
        match io_error {
            IoError::InvalidInput => "IoError: invalid input",
            IoError::OutOfBounds  => "IoError: out of bounds",
            IoError::TimedOut     => "IoError: timed out",
        }
    }
}


/// A parent trait used to specify the block size (in bytes)
/// of I/O transfers (read and write operations). 
/// See its use in [`BlockReader`] and [`BlockWriter`].
pub trait BlockIo {
    /// Returns the size in bytes of a single block (i.e., sector),
    /// the minimum granularity of I/O transfers.
    fn block_size(&self) -> usize;
}

impl<B> BlockIo for Box<B> where B: BlockIo + ?Sized {
    fn block_size(&self) -> usize { (**self).block_size() }
}
impl<B> BlockIo for &B where B: BlockIo + ?Sized {
    fn block_size(&self) -> usize { (**self).block_size() }
}
impl<B> BlockIo for &mut B where B: BlockIo + ?Sized {
    fn block_size(&self) -> usize { (**self).block_size() }
}


/// A trait that represents an I/O stream that has a known length, e.g., a disk drive.
///
/// This trait exists to enable seeking to an offset from the end of the stream.
pub trait KnownLength {
    /// Returns the length (size in bytes) of this I/O stream or device.
    fn len(&self) -> usize;
}

impl<KL> KnownLength for Box<KL> where KL: KnownLength + ?Sized {
    fn len(&self) -> usize { (**self).len() }
}
impl<KL> KnownLength for &KL where KL: KnownLength + ?Sized {
    fn len(&self) -> usize { (**self).len() }
}
impl<KL> KnownLength for &mut KL where KL: KnownLength + ?Sized {
    fn len(&self) -> usize { (**self).len() }
}


/// A trait that represents an I/O stream (e.g., an I/O device) that can be read from in blocks.
/// The block size specifies the minimum granularity of each transfer, 
/// as given by the [`BlockIo::block_size()`] function.
///
/// A `BlockReader` is not aware of the current block offset into the stream;
/// thus, each read operation requires a starting offset: 
/// the number of blocks from the beginning of the I/O stream at which the read should start.
pub trait BlockReader: BlockIo {
    /// Reads blocks of data from this reader into the given `buffer`.
    ///
    /// The number of blocks read is dictated by the length of the given `buffer`.
    ///
    /// ## Arguments
    /// * `buffer`: the buffer into which data will be read. 
    ///    The length of this buffer must be a multiple of the block size.
    /// * `block_offset`: the offset in number of blocks from the beginning of this reader.
    ///
    /// ## Return
    /// If successful, returns the number of blocks read into the given `buffer`. 
    /// Otherwise, returns an error.
    fn read_blocks(&mut self, buffer: &mut [u8], block_offset: usize) -> Result<usize, IoError>;
}

impl<R> BlockReader for Box<R> where R: BlockReader + ?Sized {
    fn read_blocks(&mut self, buffer: &mut [u8], block_offset: usize) -> Result<usize, IoError> {
        (**self).read_blocks(buffer, block_offset)
    }
}
impl<R> BlockReader for &mut R where R: BlockReader + ?Sized {
    fn read_blocks(&mut self, buffer: &mut [u8], block_offset: usize) -> Result<usize, IoError> {
        (**self).read_blocks(buffer, block_offset)
    }
}

/// A trait that represents an I/O stream (e.g., an I/O device) that can be written to in blocks.
/// The block size specifies the minimum granularity of each transfer, 
/// as given by the [`BlockIo::block_size()`] function.
///
/// A `BlockWriter` is not aware of the current block offset into the stream;
/// thus, each write operation requires a starting offset: 
/// the number of blocks from the beginning of the I/O stream at which the write should start.
pub trait BlockWriter: BlockIo {
    /// Writes blocks of data from the given `buffer` to this writer.
    ///
    /// The number of blocks written is dictated by the length of the given `buffer`.
    ///
    /// ## Arguments
    /// * `buffer`: the buffer from which data will be written. 
    ///    The length of this buffer must be a multiple of the block size.
    /// * `block_offset`: the offset in number of blocks from the beginning of this writer.
    ///
    /// ## Return
    /// If successful, returns the number of blocks written to this writer. 
    /// Otherwise, returns an error.
    fn write_blocks(&mut self, buffer: &[u8], block_offset: usize) -> Result<usize, IoError>;

    /// Flushes this entire writer's output stream, 
    /// ensuring all contents in intermediate buffers are fully written out. 
    fn flush(&mut self) -> Result<(), IoError>;
}

impl<R> BlockWriter for Box<R> where R: BlockWriter + ?Sized {
    fn write_blocks(&mut self, buffer: &[u8], block_offset: usize) -> Result<usize, IoError> {
        (**self).write_blocks(buffer, block_offset)
    }
    fn flush(&mut self) -> Result<(), IoError> { (**self).flush() }
}
impl<R> BlockWriter for &mut R where R: BlockWriter + ?Sized {
    fn write_blocks(&mut self, buffer: &[u8], block_offset: usize) -> Result<usize, IoError> {
        (**self).write_blocks(buffer, block_offset)
    }
    fn flush(&mut self) -> Result<(), IoError> { (**self).flush() }
}


/// A trait that represents an I/O stream that can be read from at the granularity of individual bytes,
/// but which does not track the current offset into the stream.
///
/// ## `ByteReader` implementation atop `BlockReader`
/// The [`ByteReader`] trait ideally _should be_ auto-implemented for any type
/// that implements the [`BlockReader`] trait,
/// to allow easy byte-wise access to a block-based I/O stream.
/// However, Rust does not allow trait specialization yet, so we cannot do this;
/// instead, use the [`ByteReaderWrapper`] type to accomplish this.
pub trait ByteReader {
    /// Reads bytes of data from this reader into the given `buffer`.
    ///
    /// The number of bytes read is dictated by the length of the given `buffer`.
    ///
    /// ## Arguments
    /// * `buffer`: the buffer into which data will be copied.
    /// * `offset`: the offset in bytes from the beginning of this reader 
    ///    where the read operation will begin.
    ///
    /// ## Return
    /// If successful, returns the number of bytes read into the given `buffer`. 
    /// Otherwise, returns an error.
    fn read_at(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, IoError>; 
}

impl<R> ByteReader for Box<R> where R: ByteReader + ?Sized {
    fn read_at(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, IoError> {
        (**self).read_at(buffer, offset)
    }
}
impl<R> ByteReader for &mut R where R: ByteReader + ?Sized {
    fn read_at(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, IoError> {
        (**self).read_at(buffer, offset)
    }
}


/// A trait that represents an I/O stream that can be written to,
/// but which does not track the current offset into the stream.
///
/// ## `ByteWriter` implementation atop `BlockWriter`
/// The [`ByteWriter`] trait ideally _should be_ auto-implemented for any type 
/// that implements both the [`BlockWriter`] **and** [`BlockReader`] traits
/// to allow easy byte-wise access to a block-based I/O stream.
/// However, Rust does not allow trait specialization yet, so we cannot do this;
/// instead, use the [`ByteWriterWrapper`] type to accomplish this.
///
/// It is only possible to implement a byte-wise writer atop a block-wise writer AND reader together,
/// because it is often necessary to read an original block of data from the underlying stream
/// before writing a partial block back to the device.
/// This is required to avoid incorrectly overwriting unrelated byte ranges.
/// 
/// Note that other implementations of `ByteWriter` may not have this restriction,
/// e.g., when the underlying writer supports writing individual bytes.
pub trait ByteWriter {
    /// Writes bytes of data from the given `buffer` to this writer.
    ///
    /// The number of bytes written is dictated by the length of the given `buffer`.
    ///
    /// ## Arguments
    /// * `buffer`: the buffer from which data will be copied. 
    /// * `offset`: the offset in number of bytes from the beginning of this writer
    ///    where the write operation will begin.
    ///
    /// ## Return
    /// If successful, returns the number of bytes written to this writer. 
    /// Otherwise, returns an error.
    fn write_at(&mut self, buffer: &[u8], offset: usize) -> Result<usize, IoError>;

    /// Flushes this writer's output stream, 
    /// ensuring all contents in intermediate buffers are fully written out.
    fn flush(&mut self) -> Result<(), IoError>;
}

impl<R> ByteWriter for Box<R> where R: ByteWriter + ?Sized {
    fn write_at(&mut self, buffer: &[u8], offset: usize) -> Result<usize, IoError> {
        (**self).write_at(buffer, offset)
    }
    fn flush(&mut self) -> Result<(), IoError> { (**self).flush() }
}
impl<R> ByteWriter for &mut R where R: ByteWriter + ?Sized {
    fn write_at(&mut self, buffer: &[u8], block_offset: usize) -> Result<usize, IoError> {
        (**self).write_at(buffer, block_offset)
    }
    fn flush(&mut self) -> Result<(), IoError> { (**self).flush() }
}


/// A wrapper struct that implements a byte-wise reader atop a block-based reader.
///
/// This ideally _should_ be realized via automatic trait implementations, 
/// in which all types that implement `BlockReader` also implement `ByteReader`, 
/// but we can't do that because Rust currently does not support specialization.
/// 
/// ## Example
/// Use the `From` implementation around a `BlockReader` instance, such as:
/// ```rust
/// let bytes_read = ByteReaderWrapper::from(storage_dev.lock().deref_mut()).read_at(...);
/// ```
#[derive(Deref, DerefMut)]
pub struct ByteReaderWrapper<R: BlockReader>(R);
impl<R> From<R> for ByteReaderWrapper<R> where R: BlockReader {
    fn from(block_reader: R) -> Self {
        ByteReaderWrapper(block_reader)
    }
} 
impl<R> ByteReader for ByteReaderWrapper<R> where R: BlockReader {
    fn read_at(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, IoError> {
        let mut tmp_block_bytes: Vec<u8> = Vec::new(); // avoid unnecessary allocation
        let offset = offset as usize;

        let transfers = blocks_from_bytes(offset .. offset + buffer.len(), self.block_size());
        for transfer in transfers.iter().flatten() {
            let BlockByteTransfer { byte_range_absolute, block_range, bytes_in_block_range } = transfer;
            let buffer_range = byte_range_absolute.start - offset .. byte_range_absolute.end - offset;

            // If the transfer is block-aligned on both sides, then we can copy it directly into the `buffer`. 
            if bytes_in_block_range.start % self.block_size() == 0 && bytes_in_block_range.end % self.block_size() == 0 {
                let _blocks_read = self.read_blocks(&mut buffer[buffer_range], block_range.start);
            } 
            // Otherwise, we transfer a single block into a temp buffer and copy a sub-range of those bytes into `buffer`.
            else {
                if tmp_block_bytes.is_empty() {
                    tmp_block_bytes = vec![0; self.block_size() * block_range.len()];
                }
                let _blocks_read = self.read_blocks(&mut tmp_block_bytes, block_range.start)?;
                buffer[buffer_range].copy_from_slice(&tmp_block_bytes[bytes_in_block_range.clone()]);
            }
        }

        Ok(buffer.len())
    }
}
impl<RW> KnownLength for ByteReaderWrapper<RW> where RW: KnownLength + BlockReader + BlockWriter {
    fn len(&self) -> usize { self.0.len() }
}


/// A wrapper struct that implements a byte-wise reader and writer
/// atop a block-based reader and writer.
///
/// This ideally _should_ be realized via automatic trait implementations, 
/// in which all types that implement `BlockReader + BlockWriter` 
/// also implement `ByteReader + ByteWriter`, 
/// but we cannot do that because Rust currently does not support specialization.
/// 
/// ## Example
/// Use the `From` implementation around a `BlockReader + BlockWriter` instance, such as:
/// ```rust
/// let mut reader_writer = ByteReaderWriterWrapper::from(storage_dev.lock().deref_mut()); 
/// let bytes_read = reader_writer.read_at(...);
/// let bytes_written = reader_writer.write_at(...);
/// ```
#[derive(Deref, DerefMut)]
pub struct ByteReaderWriterWrapper<RW: BlockReader + BlockWriter>(RW);
impl<RW> From<RW> for ByteReaderWriterWrapper<RW> where RW: BlockReader + BlockWriter {
    fn from(block_reader_writer: RW) -> Self {
        ByteReaderWriterWrapper(block_reader_writer)
    }
}
impl<RW> ByteReader for ByteReaderWriterWrapper<RW> where RW: BlockReader + BlockWriter {
    fn read_at(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, IoError> {
        ByteReaderWrapper::from(&mut self.0).read_at(buffer, offset)
    }
}
impl<RW> ByteWriter for ByteReaderWriterWrapper<RW> where RW: BlockReader + BlockWriter {
    fn write_at(&mut self, buffer: &[u8], offset: usize) -> Result<usize, IoError> {
        let mut tmp_block_bytes: Vec<u8> = Vec::new(); // avoid unnecessary allocation

        let transfers = blocks_from_bytes(offset .. offset + buffer.len(), self.block_size());
        for transfer in transfers.iter().flatten() {
            let BlockByteTransfer { byte_range_absolute, block_range, bytes_in_block_range } = transfer;
            let buffer_range = byte_range_absolute.start - offset .. byte_range_absolute.end - offset;

            // If the transfer is block-aligned on both sides, then we can write it directly 
            // from the `buffer` to the underlying block writer without reading any bytes first.
            if bytes_in_block_range.start % self.block_size() == 0 && bytes_in_block_range.end % self.block_size() == 0 {
                let _blocks_written = self.write_blocks(&buffer[buffer_range], block_range.start);
            } 
            // Otherwise, to transfer only *part* of a block (a sub-range of its bytes), we must:
            // 1. Read that whole block into a temporary buffer,
            // 2. Overwrite (copy) the sub-range of new bytes into that temp buffer,
            // 3. Write that whole block back to the underlying writer.
            else {
                if tmp_block_bytes.is_empty() {
                    tmp_block_bytes = vec![0; self.block_size() * block_range.len()];
                }
                let _blocks_read = self.read_blocks(&mut tmp_block_bytes, block_range.start)?;
                tmp_block_bytes[bytes_in_block_range.clone()].copy_from_slice(&buffer[buffer_range]);
                let _blocks_written = self.write_blocks(&tmp_block_bytes[..], block_range.start)?;
            }
        }

        Ok(buffer.len())
    }

    fn flush(&mut self) -> Result<(), IoError> {
        (**self).flush()
    }
}
impl<RW> KnownLength for ByteReaderWriterWrapper<RW> where RW: KnownLength + BlockReader + BlockWriter {
    fn len(&self) -> usize { self.0.len() }
}


/// A wrapper struct that implements a byte-wise writer
/// atop a block-based reader and writer.
///
/// This is effectively a thin wrapper around [`ByteReaderWriterWrapper`]
/// that allows only byte-wise writing to the underlying I/O stream.
/// 
/// See the [`ByteWriter`] trait docs for an explanation of why both 
/// `BlockReader + BlockWriter` are required.
///
/// ## Example
/// Use the `From` implementation around a `BlockReader + BlockWriter` instance, such as:
/// ```rust
/// /* Assume `storage_dev` implements `BlockReader + BlockWriter` */
/// let mut reader_writer = ByteReaderWriterWrapper::from(storage_dev); 
/// let bytes_written = reader_writer.write_at(...);
/// ```
#[derive(Deref, DerefMut)]
pub struct ByteWriterWrapper<RW: BlockReader + BlockWriter>(RW);
impl<RW> From<RW> for ByteWriterWrapper<RW> where RW: BlockReader + BlockWriter {
    fn from(block_reader_writer: RW) -> Self {
        ByteWriterWrapper(block_reader_writer)
    }
}
impl<RW> ByteWriter for ByteWriterWrapper<RW> where RW: BlockReader + BlockWriter {
    fn write_at(&mut self, buffer: &[u8], offset: usize) -> Result<usize, IoError> {
        ByteReaderWriterWrapper::from(&mut self.0).write_at(buffer, offset)
    }
    fn flush(&mut self) -> Result<(), IoError> {
        ByteReaderWriterWrapper::from(&mut self.0).flush()
    }
}
impl<RW> KnownLength for ByteWriterWrapper<RW> where RW: KnownLength + BlockReader + BlockWriter {
    fn len(&self) -> usize { self.0.len() }
}


/// A stateful reader that keeps track of its current offset
/// within the internal stateless [`ByteReader`] I/O stream.
///
/// This implements the [`bare_io::Read`] trait,
/// as well as the [`bare_io::Seek`] trait if the underlying I/O stream implements [`KnownLength`].
#[derive(Deref, DerefMut)]
pub struct Reader<R: ByteReader>(IoWithOffset<R>);
impl<R> Reader<R> where R: ByteReader {
    /// Creates a new `Reader` with an initial offset of 0.
    pub fn new(reader: R) -> Self {
        Reader(IoWithOffset::new(reader))
    }
}

/// A stateful writer that keeps track of its current offset
/// within the internal stateless [`ByteWriter`] I/O stream.
///
/// This implements the [`bare_io::Write`] trait,
/// as well as the [`bare_io::Seek`] trait if the underlying I/O stream implements [`KnownLength`].
#[derive(Deref, DerefMut)]
pub struct Writer<W: ByteWriter>(IoWithOffset<W>);
impl<W: ByteWriter> Writer<W> {
    /// Creates a new `Writer` with an initial offset of 0.
    pub fn new(writer: W) -> Self {
        Writer(IoWithOffset::new(writer))
    }
}

/// A stateful reader and writer that keeps track of its current offset
/// within the internal stateless [`ByteReader`] + [`ByteWriter`] I/O stream.
///
/// This implements both the [`bare_io::Read`] and [`bare_io::Write`] traits,
/// as well as the [`bare_io::Seek`] trait if the underlying I/O stream implements [`KnownLength`].
#[derive(Deref, DerefMut)]
pub struct ReaderWriter<RW: ByteReader + ByteWriter>(IoWithOffset<RW>);
impl<RW: ByteReader + ByteWriter> ReaderWriter<RW> {
    /// Creates a new `ReaderWriter` with an initial offset of 0.
    pub fn new(reader_writer: RW) -> Self {
        ReaderWriter(IoWithOffset::new(reader_writer))
    }
}

/// A stateful I/O stream (reader, writer, or both) that keeps track
/// of its current offset within its internal stateless I/O stream.
///
/// Don't use this type directly (you cannot construct one).
/// Instead, use its wrapper types: [`Reader`], [`Writer`], or [`ReaderWriter`].
///
/// This type permits seeking through the I/O stream if it has a known length,
/// i.e., if it implements the [`KnownLength`] trait.
#[doc(hidden)]
pub struct IoWithOffset<IO> {
    io: IO,
    offset: u64,
}
impl<IO> IoWithOffset<IO> {
    /// Creates a new IO stream with an initial offset of 0.
    fn new(io: IO) -> Self {
        IoWithOffset { io, offset: 0 }
    }
}
impl<IO> bare_io::Read for IoWithOffset<IO> where IO: ByteReader {
    fn read(&mut self, buf: &mut [u8]) -> bare_io::Result<usize> {
        let bytes_read = self.io.read_at(buf, self.offset as usize)
            .map_err(Into::<bare_io::Error>::into)?;
        self.offset += bytes_read as u64;
        Ok(bytes_read)
    }
}
impl<IO> bare_io::Write for IoWithOffset<IO> where IO: ByteWriter {
    fn write(&mut self, buf: &[u8]) -> bare_io::Result<usize> {
        let bytes_written = self.io.write_at(buf, self.offset as usize)
            .map_err(Into::<bare_io::Error>::into)?;
        self.offset += bytes_written as u64;
        Ok(bytes_written)
    }

    fn flush(&mut self) -> bare_io::Result<()> {
        self.io.flush().map_err(Into::into)
    }    
}

impl<IO> Seek for IoWithOffset<IO> where IO: KnownLength {
    fn seek(&mut self, position: SeekFrom) -> bare_io::Result<u64> {
        let (base_pos, offset) = match position {
            SeekFrom::Start(n) => {
                self.offset = n;
                return Ok(n);
            }
            SeekFrom::Current(n) => (self.offset, n),
            SeekFrom::End(n) => (self.io.len() as u64, n),
        };
        let new_pos = if offset >= 0 {
            base_pos.checked_add(offset as u64)
        } else {
            base_pos.checked_sub((offset.wrapping_neg()) as u64)
        };
        if let Some(n) = new_pos {
            self.offset = n;
            Ok(self.offset)
        } else {
            Err(bare_io::Error::new(
                bare_io::ErrorKind::InvalidInput,
                "invalid seek to a negative or overflowing position",
            ))
        }
    }
}


/// Calculates block-wise bounds for an I/O transfer 
/// based on a byte-wise range into a block-wise stream.
///
/// This function returns transfer operations that prioritize using
/// fewer temporary buffers and fewer data copy operations between those buffers
/// instead of prioritizing issuing fewer I/O transfer operations.
/// If you prefer to issue a single I/O transfer to cover the whole range of byte
/// (which may be faster depending on the underlying I/O device),
/// then you should not use this function.
/// 
/// There are up to three transfer operations that can possibly occur,
/// depending on the alignment of the byte-wise range:
/// 1. A partial single-block transfer of some bytes in the first block, 
///    only if the start of `byte_range` is not aligned to `block_size`.
/// 2. A multi-block transfer of contiguous whole blocks, 
///    only if `byte_range` spans more than 2 blocks.  
/// 3. A partial single-block transfer of some bytes in the last block,
///    only if the end of `byte_range` is not aligned to `block_size`.
/// 
/// ## Example
/// Given a read request for a `byte_range` of `1500..3950` and a `block_size` of `512` bytes,
/// this function will return the following three transfer operations:
/// 1. Read 1 block (block `2`) and transfer the last 36 bytes of that block (`476..512`)
///    into the byte range `1500..1536`.
/// 2. Read 4 blocks (blocks `3..7`) and transfer all of those 2048 bytes
///    into the byte range `1536..3584`.
/// 3. Read 1 block (block `7`) and transfer the first 366 bytes of that block (`0..366`)
///    into the byte range `3584..3950`.
///
/// ## Arguments
/// * `byte_range`: the absolute range of bytes where the I/O transfer starts and ends,
///    specified as absolute offsets from the beginning of the block-wise I/O stream.
/// * `block_size`: the size in bytes of each block in the block-wise I/O stream.
/// 
/// ## Return
/// Returns a list of the three above transfer operations, 
/// enclosed in `Option`s to convey that not all operations may be necessary.
/// 
pub fn blocks_from_bytes(
    byte_range: Range<usize>,
    block_size: usize
) -> [Option<BlockByteTransfer>; 3] {
    
    let mut transfers = [None, None, None];
    let mut transfer_idx = 0;

    let last_block = byte_range.end / block_size;
    let offset_into_last_block  = byte_range.end % block_size; 

    let mut curr_byte = byte_range.start;

    while curr_byte < byte_range.end {
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


/// Describes an operation for performing byte-wise I/O on a block-based I/O stream.
/// 
/// See [`blocks_from_bytes()`] for more details.
#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct BlockByteTransfer {
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


#[cfg(test)] 
mod test {
    extern crate std;
    use super::*;
    
    /// A test vector for `blocks_from_bytes()` where both the starting byte and ending byte
    /// are not block-aligned.
    #[test]
    fn test_blockwise_bytewise_multiple_both_unaligned() {
        let transfers = blocks_from_bytes(1500..3950, 512);
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

    /// A test vector for `blocks_from_bytes()` where 
    /// multiple blocks are transferred, with an unaligned start and an aligned end. 
    #[test]
    fn test_blockwise_bytewise_multiple_unaligned_to_aligned() {
        let transfers = blocks_from_bytes(1693..6144, 512);
        assert_eq!(transfers, [
            Some(BlockByteTransfer {
                byte_range_absolute: 1693..2048,
                block_range: 3..4,
                bytes_in_block_range: 157..512,
            }),
            Some(BlockByteTransfer {
                byte_range_absolute: 2048..6144,
                block_range: 4..12,
                bytes_in_block_range: 0..4096,
            }),
            None,
        ]);
    }

    /// A test vector for `blocks_from_bytes()` where 
    /// multiple blocks are transferred, with an aligned start and an unaligned end. 
    #[test]
    fn test_blockwise_bytewise_multiple_aligned_to_unaligned() {
        let transfers = blocks_from_bytes(1536..6100, 512);
        assert_eq!(transfers, [
            Some(BlockByteTransfer {
                byte_range_absolute: 1536..5632,
                block_range: 3..11,
                bytes_in_block_range: 0..4096,
            }),
            Some(BlockByteTransfer {
                byte_range_absolute: 5632..6100,
                block_range: 11..12,
                bytes_in_block_range: 0..468,
            }),
            None,
        ]);
    }

    /// A test vector for `blocks_from_bytes()` where the byte range is within one block.
    /// This tests all four combinations of byte alignment within one block:
    /// 1. unalighed start, unaligned end
    /// 2. aligned start, unaligned end
    /// 3. unaligned start, aligned end
    /// 4. aligned start, aligned end
    #[test]
    fn test_blockwise_bytewise_one_block() {
        // 1. unalighed start, unaligned end
        let transfers = blocks_from_bytes(555..900, 512);
        assert_eq!(transfers, [
            Some(BlockByteTransfer {
                byte_range_absolute: 555..900,
                block_range: 1..2,
                bytes_in_block_range: 43..388,
            }),
            None,
            None,
        ]);

        // 2. aligned start, unaligned end
        let transfers = blocks_from_bytes(512..890, 512);
        assert_eq!(transfers, [
            Some(BlockByteTransfer {
                byte_range_absolute: 512..890,
                block_range: 1..2,
                bytes_in_block_range: 0..378,
            }),
            None,
            None,
        ]);

        // 3. unaligned start, aligned end
        let transfers = blocks_from_bytes(671..1024, 512);
        assert_eq!(transfers, [
            Some(BlockByteTransfer {
                byte_range_absolute: 671..1024,
                block_range: 1..2,
                bytes_in_block_range: 159..512,
            }),
            None,
            None,
        ]);

        // 4. aligned start, aligned end
        let transfers = blocks_from_bytes(1024..1536, 512);
        assert_eq!(transfers, [
            Some(BlockByteTransfer {
                byte_range_absolute: 1024..1536,
                block_range: 2..3,
                bytes_in_block_range: 0..512,
            }),
            None,
            None,
        ]);
    }

    /// A test vector for `blocks_from_bytes()` where
    /// the byte range is several blocks, perfectly aligned on both sides. 
    #[test]
    fn test_blockwise_bytewise_multiple_both_aligned() {
        let transfers = blocks_from_bytes(1024..3072, 512);
        assert_eq!(transfers, [
            Some(BlockByteTransfer {
                byte_range_absolute: 1024..3072,
                block_range: 2..6,
                bytes_in_block_range: 0..2048,
            }),
            None,
            None,
        ]);
    }
}
