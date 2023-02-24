//! Traits and types for expressing I/O transfers of both byte-wise and block-wise granularity.
//! 
//! The important items are summarized below:
//! * [`BlockReader`], [`BlockWriter`]: traits that represent I/O streams which can be read from
//!   or written to at the granularity of a single block (as the smallest transferable chunk).
//! * [`BlockIo`]: a parent trait that specifies the size in bytes of each block 
//!   in a block-based I/O stream.
//! * [`KnownLength`]: a trait that represents an I/O stream with a known length, 
//!   such as a disk drive.
//! * [`ByteReader`], [`ByteWriter`]: traits that represent I/O streams which can be read from
//!   or written to at the granularity of an individual byte.
//! * Wrapper types that allow byte-wise access atop block-based I/O streams:
//!   [`ByteReaderWrapper`], [`ByteWriterWrapper`], [`ByteReaderWriterWrapper`].
//!    * Notably, the [`blocks_from_bytes()`] function is useful for calculating the set of
//!      block-based I/O transfers that are needed to satisfy an arbitrary byte-wise transfer.
//!
//! For example, a storage device like a hard drive that supports transfers of 512-byte blocks
//! should implement `BlockIo`, `BlockReader`, `BlockWriter`, and `KnownLength` traits.
//! A user can then use those traits directly to transfer whole blocks to/from the device,
//! or wrap the storage device in one of the byte-wise reader/writer types 
//! in order to transfer arbitrary bytes (as little as one byte) at a time to/from the device.
//!
//! We also provide the [`LockableIo`] type for convenient use with I/O streams or devices
//! that exist behind a shared lock, e.g., `Arc<Mutex<IO>>`, `Mutex<IO>`, etc.
//! This allows you to access the I/O stream transparently through the lock by using traits
//! that the interior `IO` object implements, such as the block-wise I/O traits listed above.
//!
//! ## Stateless vs. Stateful I/O
//! Note that the above traits represent "stateless" access into I/O streams or devices,
//! in that successive read/write operations will not advance any kind of "offset" or cursor.
//!
//! To read or write while tracking the current offset into the I/O stream, 
//! we provide the [`ReaderWriter`], [`Reader`], and [`Writer`] structs,
//! which act as "stateful" wrappers around an underlying "stateless" I/O stream
//! (such as a stateless `ByteReader` or `ByteWriter`).
//! This offers a more convenient interface with more traditional I/O behavior,
//! in which the next read or write operation will start where the prior one ended.
//!

#![allow(clippy::range_plus_one)]
#![no_std]
#![feature(int_roundings)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate delegate;
extern crate spin;
extern crate core2;
extern crate lockable;

#[cfg(test)]
mod test;

use core::{borrow::Borrow, cmp::min, marker::PhantomData, ops::{Deref, DerefMut, Range}};
use alloc::{boxed::Box, string::String, vec::Vec};
use lockable::Lockable;
use core2::io::{Seek, SeekFrom};


/// Errors that can be returned from I/O operations.
#[derive(Debug)]
pub enum IoError {
    /// An input parameter or argument was incorrect or invalid.
    InvalidInput,
    /// The I/O operation timed out and was canceled.
    TimedOut,
    /// A miscellaneous error occurred.
    Other(&'static str),
}

impl From<IoError> for core2::io::Error {
    fn from(io_error: IoError) -> Self {
        use core2::io::ErrorKind;
        match io_error {
            IoError::InvalidInput => ErrorKind::InvalidInput.into(),
            IoError::TimedOut     => ErrorKind::TimedOut.into(),
            IoError::Other(_)     => ErrorKind::Other.into(),
        }
    }
}

impl From<&'static str> for IoError {
    fn from(s: &'static str) -> IoError {
        IoError::Other(s)
    }
}

impl From<IoError> for String {
    fn from(e: IoError) -> String {
        let s: &'static str = e.into();
        String::from(s)
    }
}

impl From<IoError> for &'static str {
    fn from(io_error: IoError) -> Self {
        match io_error {
            IoError::InvalidInput => "invalid input",
            IoError::TimedOut     => "timed out",
            IoError::Other(s)     => s,
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
#[allow(clippy::len_without_is_empty)]
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
    /// # Arguments
    /// * `buffer`: the buffer into which data will be read. 
    ///    The length of this buffer must be a multiple of the block size.
    /// * `block_offset`: the offset in number of blocks from the beginning of this reader.
    ///
    /// # Return
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
    /// # Arguments
    /// * `buffer`: the buffer from which data will be written. 
    ///    The length of this buffer must be a multiple of the block size.
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

impl<W> BlockWriter for Box<W> where W: BlockWriter + ?Sized {
    fn write_blocks(&mut self, buffer: &[u8], block_offset: usize) -> Result<usize, IoError> {
        (**self).write_blocks(buffer, block_offset)
    }
    fn flush(&mut self) -> Result<(), IoError> { (**self).flush() }
}
impl<W> BlockWriter for &mut W where W: BlockWriter + ?Sized {
    fn write_blocks(&mut self, buffer: &[u8], block_offset: usize) -> Result<usize, IoError> {
        (**self).write_blocks(buffer, block_offset)
    }
    fn flush(&mut self) -> Result<(), IoError> { (**self).flush() }
}


/// A trait that represents an I/O stream that can be read from at the granularity of individual bytes,
/// but which does not track the current offset into the stream.
///
/// # `ByteReader` implementation atop `BlockReader`
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
    /// # Arguments
    /// * `buffer`: the buffer into which data will be copied.
    /// * `offset`: the offset in bytes from the beginning of this reader 
    ///    where the read operation will begin.
    ///
    /// # Return
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
/// # `ByteWriter` implementation atop `BlockWriter`
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
    /// # Arguments
    /// * `buffer`: the buffer from which data will be copied. 
    /// * `offset`: the offset in number of bytes from the beginning of this writer
    ///    where the write operation will begin.
    ///
    /// # Return
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
/// # Example
/// Use the `From` implementation around a `BlockReader` instance, such as:
/// ```ignore
/// // Assume `storage_dev` implements `BlockReader`
/// let bytes_read = ByteReaderWrapper::from(storage_dev).read_at(...);
/// ```
pub struct ByteReaderWrapper<R: BlockReader>(R);
impl<R> From<R> for ByteReaderWrapper<R> where R: BlockReader {
    fn from(block_reader: R) -> Self {
        ByteReaderWrapper(block_reader)
    }
} 
impl<R> ByteReader for ByteReaderWrapper<R> where R: BlockReader {
    fn read_at(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, IoError> {
        let mut tmp_block_bytes: Vec<u8> = Vec::new(); // avoid unnecessary allocation

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
impl<R> BlockIo for ByteReaderWrapper<R> where R: BlockReader {
    delegate!{ to self.0 { fn block_size(&self) -> usize; } }
}
impl<R> KnownLength for ByteReaderWrapper<R> where R: KnownLength + BlockReader {
    delegate!{ to self.0 { fn len(&self) -> usize; } }
}
impl<R> BlockReader for ByteReaderWrapper<R> where R: BlockReader {
    delegate!{ to self.0 { fn read_blocks(&mut self, buffer: &mut [u8], block_offset: usize) -> Result<usize, IoError>; } }
}


/// A wrapper struct that implements a byte-wise reader and writer
/// atop a block-based reader and writer.
///
/// This ideally _should_ be realized via automatic trait implementations, 
/// in which all types that implement `BlockReader + BlockWriter` 
/// also implement `ByteReader + ByteWriter`, 
/// but we cannot do that because Rust currently does not support specialization.
/// 
/// # Example
/// Use the `From` implementation around a `BlockReader + BlockWriter` instance, such as:
/// ```ignore
/// // Assume `storage_dev` implements `BlockReader + BlockWriter`
/// let mut reader_writer = ByteReaderWriterWrapper::from(storage_dev); 
/// let bytes_read = reader_writer.read_at(...);
/// let bytes_written = reader_writer.write_at(...);
/// ```
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
        BlockWriter::flush(self)
    }
}
impl<RW> BlockIo for ByteReaderWriterWrapper<RW> where RW: BlockReader + BlockWriter {
    delegate!{ to self.0 { fn block_size(&self) -> usize; } }
}
impl<RW> KnownLength for ByteReaderWriterWrapper<RW> where RW: KnownLength + BlockReader + BlockWriter {
    delegate!{ to self.0 { fn len(&self) -> usize; } }
}
impl<RW> BlockReader for ByteReaderWriterWrapper<RW> where RW: BlockReader + BlockWriter {
    delegate!{ to self.0 { fn read_blocks(&mut self, buffer: &mut [u8], block_offset: usize) -> Result<usize, IoError>; } }
}
impl<RW> BlockWriter for ByteReaderWriterWrapper<RW> where RW: BlockReader + BlockWriter {
    delegate!{ to self.0 {
        fn write_blocks(&mut self, buffer: &[u8], block_offset: usize) -> Result<usize, IoError>;
        fn flush(&mut self) -> Result<(), IoError>;
    } }
}


/// A wrapper struct that implements a byte-wise writer
/// atop a block-based reader and writer.
///
/// This is effectively the same struct as [`ByteReaderWriterWrapper`],
/// but it allows *only* writing to the underlying I/O stream, not reading.
/// 
/// See the [`ByteWriter`] trait docs for an explanation of why both 
/// `BlockReader + BlockWriter` are required.
///
/// # Example
/// Use the `From` implementation around a `BlockReader + BlockWriter` instance, such as:
/// ```ignore
/// // Assume `storage_dev` implements `BlockReader + BlockWriter`
/// ByteReaderWriterWrapper::from(storage_dev).write_at(...);
/// ```
pub struct ByteWriterWrapper<RW: BlockReader + BlockWriter>(ByteReaderWriterWrapper<RW>);
impl<RW> From<RW> for ByteWriterWrapper<RW> where RW: BlockReader + BlockWriter {
    fn from(block_reader_writer: RW) -> Self {
        ByteWriterWrapper(ByteReaderWriterWrapper(block_reader_writer))
    }
}
impl<RW> ByteWriter for ByteWriterWrapper<RW> where RW: BlockReader + BlockWriter {
    delegate!{ to self.0 { fn write_at(&mut self, buffer: &[u8], offset: usize) -> Result<usize, IoError>; } }
    fn flush(&mut self) -> Result<(), IoError> { ByteWriter::flush(&mut self.0) }
}
impl<RW> BlockIo for ByteWriterWrapper<RW> where RW: BlockReader + BlockWriter {
    delegate!{ to self.0 { fn block_size(&self) -> usize; } }
}
impl<RW> KnownLength for ByteWriterWrapper<RW> where RW: KnownLength + BlockReader + BlockWriter {
    delegate!{ to self.0 { fn len(&self) -> usize; } }
}
impl<RW> BlockWriter for ByteWriterWrapper<RW> where RW: BlockReader + BlockWriter {
    delegate!{ to self.0 { fn write_blocks(&mut self, buffer: &[u8], block_offset: usize) -> Result<usize, IoError>; } }
    fn flush(&mut self) -> Result<(), IoError> { BlockWriter::flush(&mut self.0) }
}


/// A readable and writable "stateful" I/O stream that keeps track 
/// of its current offset within its internal stateless I/O stream.
///
/// ## Trait implementations
/// * This implements the [`core2::io::Read`] and [`core2::io::Write`] traits for read and write access.
/// * This implements the [`core2::io::Seek`] trait if the underlying I/O stream implements [`KnownLength`].
/// * This also forwards all other I/O-related traits implemented by the underlying I/O stream.
/// * This derefs into the inner `IO` type, via both [`Deref`] and [`DerefMut`].
pub struct ReaderWriter<IO> {
    io: IO,
    offset: u64,
}
impl<IO> ReaderWriter<IO> where IO: ByteReader + ByteWriter {
    /// Creates a new `ReaderWriter` with an initial offset of 0.
    pub fn new(io: IO) -> ReaderWriter<IO> {
        ReaderWriter { io, offset: 0 }
    }
}
impl<IO> Deref for ReaderWriter<IO> {
    type Target = IO;
    fn deref(&self) -> &Self::Target {
        &self.io
    }
}
impl<IO> DerefMut for ReaderWriter<IO> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.io
    }
}
impl<IO> core2::io::Read for ReaderWriter<IO> where IO: ByteReader {
    fn read(&mut self, buf: &mut [u8]) -> core2::io::Result<usize> {
        let bytes_read = self.io.read_at(buf, self.offset as usize)
            .map_err(Into::<core2::io::Error>::into)?;
        self.offset += bytes_read as u64;
        Ok(bytes_read)
    }
}
impl<IO> core2::io::Write for ReaderWriter<IO> where IO: ByteWriter {
    fn write(&mut self, buf: &[u8]) -> core2::io::Result<usize> {
        let bytes_written = self.io.write_at(buf, self.offset as usize)
            .map_err(Into::<core2::io::Error>::into)?;
        self.offset += bytes_written as u64;
        Ok(bytes_written)
    }

    fn flush(&mut self) -> core2::io::Result<()> {
        self.io.flush().map_err(Into::into)
    }    
}
impl<IO> Seek for ReaderWriter<IO> where IO: KnownLength {
    fn seek(&mut self, position: SeekFrom) -> core2::io::Result<u64> {
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
            Err(core2::io::Error::new(
                core2::io::ErrorKind::InvalidInput,
                "invalid seek to a negative or overflowing position",
            ))
        }
    }
}
// Implement (by delegation) various I/O traits for `ReaderWriter`.
impl<IO> BlockIo for ReaderWriter<IO> where IO: BlockIo {
    delegate!{ to self.io { fn block_size(&self) -> usize; } }
}
impl<IO> KnownLength for ReaderWriter<IO> where IO: KnownLength {
    delegate!{ to self.io { fn len(&self) -> usize; } }
}
impl<IO> BlockReader for ReaderWriter<IO> where IO: BlockReader {
    delegate!{ to self.io { fn read_blocks(&mut self, buffer: &mut [u8], block_offset: usize) -> Result<usize, IoError>; } }
}
impl<IO> BlockWriter for ReaderWriter<IO> where IO: BlockWriter {
    delegate!{ to self.io {
        fn write_blocks(&mut self, buffer: &[u8], block_offset: usize) -> Result<usize, IoError>; 
        fn flush(&mut self) -> Result<(), IoError>;
    } }
}
impl<IO> ByteReader for ReaderWriter<IO> where IO: ByteReader {
    delegate!{ to self.io { fn read_at(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, IoError>; } }
}
impl<IO> ByteWriter for ReaderWriter<IO> where IO: ByteWriter {
    delegate!{ to self.io {
        fn write_at(&mut self, buffer: &[u8], offset: usize) -> Result<usize, IoError>;
        fn flush(&mut self) -> Result<(), IoError>;
    } }
}


/// A stateful reader that keeps track of its current offset
/// within the internal stateless [`ByteReader`] I/O stream.
///
/// This implements the [`core2::io::Read`] trait for read-only access,
/// as well as the [`core2::io::Seek`] trait if the underlying I/O stream implements [`KnownLength`].
/// It also forwards all other read-only I/O-related traits implemented by the underlying I/O stream.
///
/// Note: this is implemented as a thin wrapper around [`ReaderWriter`].
pub struct Reader<R>(ReaderWriter<R>);
impl<R> Reader<R> where R: ByteReader {
    /// Creates a new `Reader` with an initial offset of 0.
    pub fn new(reader: R) -> Reader<R> {
        Reader(ReaderWriter { io: reader, offset: 0 } )
    }
}
// Implement (by delegation) various I/O traits for `Reader`
impl<IO> BlockIo for Reader<IO> where IO: BlockIo {
    delegate!{ to self.0 { fn block_size(&self) -> usize; } }
}
impl<IO> KnownLength for Reader<IO> where IO: KnownLength {
    delegate!{ to self.0 { fn len(&self) -> usize; } }
}
impl<IO> BlockReader for Reader<IO> where IO: BlockReader {
    delegate!{ to self.0 { fn read_blocks(&mut self, buffer: &mut [u8], block_offset: usize) -> Result<usize, IoError>; } }
}
impl<IO> ByteReader for Reader<IO> where IO: ByteReader {
    delegate!{ to self.0 { fn read_at(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, IoError>; } }
}
impl<IO> core2::io::Read for Reader<IO> where IO: ByteReader {
    delegate!{ to self.0 { fn read(&mut self, buf: &mut [u8]) -> core2::io::Result<usize>; } }
}
impl<IO> Seek for Reader<IO> where IO: KnownLength {
    delegate!{ to self.0 { fn seek(&mut self, position: SeekFrom) -> core2::io::Result<u64>; } }
}


/// A stateful writer that keeps track of its current offset
/// within the internal stateless [`ByteWriter`] I/O stream.
///
/// This implements the [`core2::io::Write`] trait for write-only access,
/// as well as the [`core2::io::Seek`] trait if the underlying I/O stream implements [`KnownLength`].
/// It also forwards all other write-only I/O-related traits implemented by the underlying I/O stream.
///
/// Note: this is implemented as a thin wrapper around [`ReaderWriter`].
pub struct Writer<W>(ReaderWriter<W>);
impl<W: ByteWriter> Writer<W> {
    /// Creates a new `Writer` with an initial offset of 0.
    pub fn new(writer: W) -> Self {
        Writer(ReaderWriter { io: writer, offset: 0 } )
    }
}
// Implement (by delegation) various I/O traits for `Writer`.
impl<IO> BlockIo for Writer<IO> where IO: BlockIo {
    delegate!{ to self.0 { fn block_size(&self) -> usize; } }
}
impl<IO> KnownLength for Writer<IO> where IO: KnownLength {
    delegate!{ to self.0 { fn len(&self) -> usize; } }
}
impl<IO> BlockWriter for Writer<IO> where IO: BlockWriter {
    delegate!{ to self.0 {
        fn write_blocks(&mut self, buffer: &[u8], block_offset: usize) -> Result<usize, IoError>; 
        fn flush(&mut self) -> Result<(), IoError>;
    } }
}
impl<IO> ByteWriter for Writer<IO> where IO: ByteWriter {
    delegate!{ to self.0 {
        fn write_at(&mut self, buffer: &[u8], offset: usize) -> Result<usize, IoError>;
        fn flush(&mut self) -> Result<(), IoError>;
    } }
}
impl<IO> core2::io::Write for Writer<IO> where IO: ByteWriter {
    delegate!{ to self.0 { fn write(&mut self, buf: &[u8]) -> core2::io::Result<usize>; } }
    fn flush(&mut self) -> core2::io::Result<()> { core2::io::Write::flush(&mut self.0) }
}
impl<IO> Seek for Writer<IO> where IO: KnownLength {
    delegate!{ to self.0 { fn seek(&mut self, position: SeekFrom) -> core2::io::Result<u64>; } }
}


/// A struct that holds an IO object wrapped in a [`Lockable`] type `L`, 
/// for the purpose of forwarding various IO-related traits through the lock to the `IO` type.
///
/// This allows an IO object inside of a lock type (e.g., `Mutex<IO>`, `Arc<Mutex<IO>>`) 
/// to be used as a type that implements some IO-specific trait, 
/// such as those listed in the crate-level documentation. 
///
/// The following traits are forwarded to the `IO` instance through the `Lockable` wrapper:
/// * [`BlockIo`]
/// * [`KnownLength`]
/// * [`BlockReader`] and [`BlockWriter`]
/// * [`ByteReader`] and [`ByteWriter`]
/// * [`core2::io::Read`], [`core2::io::Write`], and [`core2::io::Seek`]
/// * [`core::fmt::Write`]
///
/// # Usage and Examples
/// The Rust compiler has difficulty inferring all of the types needed in this struct; 
/// therefore, you must typically specify at least two types: 
///  1. `L`: the type of the lock itself, which implements `Lockable`.
///  2. `IO`, the inner type inside of the `Lockable` lock type `L`.
///
/// Here's an example of the minimal types that must be specified:
/// ```no_run
/// // `storage_dev` has the type `Arc<spin::Mutex<dyn Storage Device + Send>>`
/// let storage_dev = storage_manager::storage_devices().next().unwrap();
/// let fail = LockableIo::from(storage_dev); // <-- Error: rustc will complain!
/// let success = LockableIo::<dyn StorageDevice + Send, spin::Mutex<_>, _>::from(storage_dev);
/// let rw = ReaderWriter::new(ByteReaderWriterWrapper::from(success));
/// ```
///
/// You can optionally specify the type `IO` within the `Lockable` type `L`,
/// but Rustc can infer that once you specify the first type `IO`.
/// You can also optionally specify the final parameter `B: Borrow<L>`,
/// but Rustc can also infer that based on your argument to `LockableIo::from()`.
/// You can ask Rustc to infer both of those using the `_` character, as shown above.
#[derive(Debug)]
pub struct LockableIo<'io, IO, L, B> 
    where IO: 'io + ?Sized,
          L: for <'a> Lockable<'a, IO> + ?Sized,
          B: Borrow<L>,
{
    /// The actual `Borrow`-able type, which obviously must be Sized.
    /// This is typically an `Arc` or the `Lockable` type itself, e.g., a `Mutex`.
    inner: B,
    /// Phew ... this one is nasty. 
    /// Basically, `_phantom1` ideally *should* be `PhantomData<&'io IO>`.
    /// 
    /// However, that causes the `LockableIo` struct to be non-`Sync` no matter what
    /// the underlying types of `IO`, `L`, and `B` are, which is unacceptable and wrong. 
    /// 
    /// We want `LockableIo` to be `Send` and/or `Sync` as long as 
    /// the underlying type `L` is `Send` and/or `Sync`.
    /// 
    /// Thus, we use this weird trait object inside of PhantomData to indicate
    /// that what this struct actually holds is effectively a borrow-able object of type `L`,
    /// which is anything that implements `Lockable` under the hood, 
    /// typically something like `Arc<Mutex< IO >>`.
    /// Note that the interior types (Guard, GuardMut) do not actually matter,
    /// we just need to use the `'io` lifetime and the `IO` type parameter here.
    /// 
    /// We also add `Send + Sync` to the inner trait object's bounds,
    /// merely to ensure that this `PhantomData` object does not *prevent* the outer `LockableIo`
    /// type from auto-implementing `Send` and/or `Sync`.
    /// We want the compiler to determine and auto-implement `Send` and/or `Sync`
    /// based *solely* on whether `L` is `Send` and/or `Sync`.
    /// 
    /// Read more here: <https://users.rust-lang.org/t/looking-for-a-deeper-understanding-of-phantomdata/32477>
    _phantom1: PhantomData<dyn Lockable<'io, IO, Guard = (), GuardMut = ()> + Send + Sync>,
    /// This can be a regular `PhantomData` because we want the compiler to 
    /// auto-implement `Send` and/or `Sync` based on whether `B` and `L` are `Send` and/or `Sync`.
    _phantom2: PhantomData<L>,
}

impl<'io, IO, L, B> From<B> for LockableIo<'io, IO, L, B> 
    where IO: 'io + ?Sized,
          L: for <'a> Lockable<'a, IO> + ?Sized,
          B: Borrow<L>,
{
    fn from(lockable_io: B) -> Self {
        LockableIo {
            inner: lockable_io,
            _phantom1: PhantomData,
            _phantom2: PhantomData,
        }
    }
}

impl<'io, IO, L, B> Clone for LockableIo<'io, IO, L, B> 
    where IO: 'io + ?Sized,
          L: for <'a> Lockable<'a, IO> + ?Sized,
          B: Borrow<L> + Clone,
{
    fn clone(&self) -> Self {
        LockableIo {
            inner: self.inner.clone(),
            _phantom1: PhantomData,
            _phantom2: PhantomData,
        }
    }
}

impl<'io, IO, L, B> Deref for LockableIo<'io, IO, L, B> 
    where IO: 'io + ?Sized,
          L: for <'a> Lockable<'a, IO> + ?Sized,
          B: Borrow<L>,
{
    type Target = L;
    fn deref(&self) -> &L {
        self.inner.borrow()
    }
}

// Implement (by delegation) various I/O traits for the `LockableIo` wrapper around Mutex<trait>.
impl<'io, IO, L, B> BlockIo for LockableIo<'io, IO, L, B> 
    where IO: BlockIo + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock() { fn block_size(&self) -> usize; } }
}

impl<'io, IO, L, B> KnownLength for LockableIo<'io, IO, L, B>
    where IO: KnownLength + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock() { fn len(&self) -> usize; } }
}
impl<'io, IO, L, B> BlockReader for LockableIo<'io, IO, L, B>
    where IO: BlockReader + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock_mut() { fn read_blocks(&mut self, buffer: &mut [u8], block_offset: usize) -> Result<usize, IoError>; } }
}
impl<'io, IO, L, B> BlockWriter for LockableIo<'io, IO, L, B>
    where IO: BlockWriter + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock_mut() {
        fn write_blocks(&mut self, buffer: &[u8], block_offset: usize) -> Result<usize, IoError>; 
        fn flush(&mut self) -> Result<(), IoError>;
    } }
}
impl<'io, IO, L, B> ByteReader for LockableIo<'io, IO, L, B>
    where IO: ByteReader + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock_mut() { fn read_at(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, IoError>; } }
}
impl<'io, IO, L, B> ByteWriter for LockableIo<'io, IO, L, B>
    where IO: ByteWriter + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock_mut() {
        fn write_at(&mut self, buffer: &[u8], offset: usize) -> Result<usize, IoError>; 
        fn flush(&mut self) -> Result<(), IoError>;
    } }
}
impl<'io, IO, L, B> core2::io::Read for LockableIo<'io, IO, L, B>
    where IO: core2::io::Read + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock_mut() { fn read(&mut self, buf: &mut [u8]) -> core2::io::Result<usize>; } }
}
impl<'io, IO, L, B> core2::io::Write for LockableIo<'io, IO, L, B>
    where IO: core2::io::Write + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock_mut() {
        fn write(&mut self, buf: &[u8]) -> core2::io::Result<usize>;
        fn flush(&mut self) -> core2::io::Result<()>;
    } }
}
impl<'io, IO, L, B> core2::io::Seek for LockableIo<'io, IO, L, B>
    where IO: core2::io::Seek + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock_mut() { fn seek(&mut self, position: core2::io::SeekFrom) -> core2::io::Result<u64>; } }
}
impl<'io, IO, L, B> core::fmt::Write for LockableIo<'io, IO, L, B>
    where IO: core::fmt::Write + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock_mut() {
        fn write_str(&mut self, s: &str) -> core::fmt::Result;
    } }
}


// Also implement the same set of I/O traits for an immutable reference `&LockableIo` wrapper,
// because `LockableIo` provides inner mutability.
// Here, we only re-implement the I/O traits with methods that require `&mut self`,
// because traits like `BlockIo` and `KnownLength` already have blanket implementations
// of themselves for `&self`.
impl<'io, IO, L, B> BlockReader for &LockableIo<'io, IO, L, B>
    where IO: BlockReader + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock_mut() { fn read_blocks(&mut self, buffer: &mut [u8], block_offset: usize) -> Result<usize, IoError>; } }
}
impl<'io, IO, L, B> BlockWriter for &LockableIo<'io, IO, L, B>
    where IO: BlockWriter + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock_mut() {
        fn write_blocks(&mut self, buffer: &[u8], block_offset: usize) -> Result<usize, IoError>; 
        fn flush(&mut self) -> Result<(), IoError>;
    } }
}
impl<'io, IO, L, B> ByteReader for &LockableIo<'io, IO, L, B>
    where IO: ByteReader + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock_mut() { fn read_at(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, IoError>; } }
}
impl<'io, IO, L, B> ByteWriter for &LockableIo<'io, IO, L, B>
    where IO: ByteWriter + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock_mut() {
        fn write_at(&mut self, buffer: &[u8], offset: usize) -> Result<usize, IoError>; 
        fn flush(&mut self) -> Result<(), IoError>;
    } }
}
impl<'io, IO, L, B> core2::io::Read for &LockableIo<'io, IO, L, B>
    where IO: core2::io::Read + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock_mut() { fn read(&mut self, buf: &mut [u8]) -> core2::io::Result<usize>; } }
}
impl<'io, IO, L, B> core2::io::Write for &LockableIo<'io, IO, L, B>
    where IO: core2::io::Write + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock_mut() {
        fn write(&mut self, buf: &[u8]) -> core2::io::Result<usize>;
        fn flush(&mut self) -> core2::io::Result<()>;
    } }
}
impl<'io, IO, L, B> core2::io::Seek for &LockableIo<'io, IO, L, B>
    where IO: core2::io::Seek + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock_mut() { fn seek(&mut self, position: core2::io::SeekFrom) -> core2::io::Result<u64>; } }
}
impl<'io, IO, L, B> core::fmt::Write for &LockableIo<'io, IO, L, B>
    where IO: core::fmt::Write + 'io + ?Sized, L: for <'a> Lockable<'a, IO> + ?Sized, B: Borrow<L>,
{
    delegate!{ to self.lock_mut() {
        fn write_str(&mut self, s: &str) -> core::fmt::Result;
    } }
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
/// # Example
/// Given a read request for a `byte_range` of `1500..3950` and a `block_size` of `512` bytes,
/// this function will return the following three transfer operations:
/// 1. Read 1 block (block `2`) and transfer the last 36 bytes of that block (`476..512`)
///    into the byte range `1500..1536`.
/// 2. Read 4 blocks (blocks `3..7`) and transfer all of those 2048 bytes
///    into the byte range `1536..3584`.
/// 3. Read 1 block (block `7`) and transfer the first 366 bytes of that block (`0..366`)
///    into the byte range `3584..3950`.
///
/// # Arguments
/// * `byte_range`: the absolute range of bytes where the I/O transfer starts and ends,
///    specified as absolute offsets from the beginning of the block-wise I/O stream.
/// * `block_size`: the size in bytes of each block in the block-wise I/O stream.
/// 
/// # Return
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
                    block_range: curr_block .. (end_byte.next_multiple_of(block_size) / block_size),
                    bytes_in_block_range: 0 .. (end_byte - curr_byte),
                });
                transfer_idx += 1;
                curr_byte = end_byte;
            }
        }
        // Otherwise, if the curr_byte is NOT block-aligned, then we can only do a single-block transfer.
        else {
            let end_byte = min(byte_range.end, curr_byte.next_multiple_of(block_size));
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

/// Rounds the given `value` down to the nearest `multiple`.
#[inline]
fn round_down(value: usize, multiple: usize) -> usize {
    (value / multiple) * multiple
}
