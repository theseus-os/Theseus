use crate::Node;

/// A file.
///
/// The methods don't take a mutable reference to `self` as types implementing
/// `File` should use interior mutability.
pub trait File: Node {
    /// Read some bytes from the file into the specified buffer, returning how
    /// many bytes were read.
    fn read(&self, buffer: &mut [u8]) -> Result<usize, &'static str>;

    /// Write a buffer into this writer, returning how many bytes were written.
    fn write(&self, buffer: &[u8]) -> Result<usize, &'static str>;

    /// Seek to an offset, in bytes, in a stream.
    ///
    /// A seek beyond the end of a stream is allowed, but behaviour is defined
    /// by the implementation.
    ///
    /// If the seek operation completed successfully, this method returns the
    /// new position from the start of the stream.
    fn seek(&self, pos: SeekFrom) -> Result<usize, &'static str>;
}

/// Enumeration of possible methods to seek within a file.
///
/// It is used by the [`File::seek`] method.
pub enum SeekFrom {
    /// Sets the offset to the provided number of bytes.
    Start(usize),

    /// Sets the offset to the size of this object plus the specified number of
    /// bytes.
    ///
    /// It is possible to seek beyond the end of an object, but it's an error to
    /// seek before byte 0.
    End(isize),

    /// Sets the offset to the current position plus the specified number of
    /// bytes.
    ///
    /// It is possible to seek beyond the end of an object, but it's an error to
    /// seek before byte 0.
    Current(isize),
}
