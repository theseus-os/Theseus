//! This module provides the `File` structure, representing an opaque handle to a
//! directory / file, as well as providing functions for opening new files.
//!
//! Usually a file system implementation will return a "root" file, representing
//! `/` on that volume, and with that file it is possible to enumerate and open
//! all the other files on that volume.

mod dir;
mod info;

use crate::prelude::*;
use crate::{CStr16, Char16, Guid, Result, Status};
use bitflags::bitflags;
use core::ffi::c_void;
use core::mem;
use core::ptr;
use ucs2;

pub use self::dir::Directory;
pub use self::info::{
    Align, FileInfo, FileProtocolInfo, FileSystemInfo, FileSystemVolumeLabel, FromUefi,
};

/// A file represents an abstraction of some contiguous block of data residing
/// on a volume.
///
/// Dropping this structure will result in the file handle being closed.
///
/// Files have names, and a fixed size.
///
/// A `File` may also be used to access a directory, but the interface for doing
/// so is awkward. If you know that a file is a directory, consider wrapping it
/// into a `Directory` for improved ergonomics.
pub struct File<'imp>(&'imp mut FileImpl);

impl<'imp> File<'imp> {
    pub(super) unsafe fn new(ptr: *mut FileImpl) -> Self {
        File(&mut *ptr)
    }

    /// Try to open a file relative to this file/directory.
    ///
    /// # Arguments
    /// * `filename`    Path of file to open, relative to this File
    /// * `open_mode`   The mode to open the file with
    /// * `attributes`  Only valid when `FILE_MODE_CREATE` is used as a mode
    ///
    /// # Errors
    /// * `uefi::Status::INVALID_PARAMETER`  The filename exceeds the maximum length of 255 chars
    /// * `uefi::Status::NOT_FOUND`          Could not find file
    /// * `uefi::Status::NO_MEDIA`           The device has no media
    /// * `uefi::Status::MEDIA_CHANGED`      The device has a different medium in it
    /// * `uefi::Status::DEVICE_ERROR`       The device reported an error
    /// * `uefi::Status::VOLUME_CORRUPTED`   The filesystem structures are corrupted
    /// * `uefi::Status::WRITE_PROTECTED`    Write/Create attempted on readonly file
    /// * `uefi::Status::ACCESS_DENIED`      The service denied access to the file
    /// * `uefi::Status::OUT_OF_RESOURCES`    Not enough resources to open file
    /// * `uefi::Status::VOLUME_FULL`        The volume is full
    pub fn open(
        &mut self,
        filename: &str,
        open_mode: FileMode,
        attributes: FileAttribute,
    ) -> Result<File> {
        const BUF_SIZE: usize = 255;
        if filename.len() > BUF_SIZE {
            Err(Status::INVALID_PARAMETER.into())
        } else {
            let mut buf = [0u16; BUF_SIZE + 1];
            let mut ptr = ptr::null_mut();

            let len = ucs2::encode(filename, &mut buf)?;
            let filename = unsafe { CStr16::from_u16_with_nul_unchecked(&buf[..=len]) };

            unsafe { (self.0.open)(self.0, &mut ptr, filename.as_ptr(), open_mode, attributes) }
                .into_with_val(|| unsafe { File::new(ptr) })
        }
    }

    /// Close this file handle. Same as dropping this structure.
    pub fn close(self) {}

    /// Closes and deletes this file
    ///
    /// # Warnings
    /// * `uefi::Status::WARN_DELETE_FAILURE` The file was closed, but deletion failed
    pub fn delete(self) -> Result {
        let result = (self.0.delete)(self.0).into();
        mem::forget(self);
        result
    }

    /// Read data from file
    ///
    /// If the file is not a directory, try to read as much as possible into `buffer`. Returns the
    /// number of bytes that were actually read.
    ///
    /// If the file is a directory, try to read the next directory entry, which is a `FileInfo`,
    /// into `buffer`. If it is too small, report the required buffer size as part of the error.
    /// If there are no more directory entries, report success as if zero bytes were read. You
    /// should provide a buffer which is suitably aligned for holding a `FileInfo` in this case, or
    /// be prepared to only manipulate your data via `ptr::read_unaligned()`.
    ///
    /// Because enumerating directory entries is so involved, it is recommended to wrap the `File`
    /// into a `Directory` if you intend to do it.
    ///
    /// # Arguments
    /// * `buffer`  The target buffer of the read operation
    ///
    /// # Errors
    /// * `uefi::Status::NO_MEDIA`           The device has no media
    /// * `uefi::Status::DEVICE_ERROR`       The device reported an error, the file was deleted,
    ///                                      or the end of the file was reached before the `read()`.
    /// * `uefi::Status::VOLUME_CORRUPTED`   The filesystem structures are corrupted
    /// * `uefi::Status::BUFFER_TOO_SMALL`   The buffer is too small to hold a directory entry,
    ///                                      and the required buffer size is provided as output.
    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Option<usize>> {
        let mut buffer_size = buffer.len();
        unsafe { (self.0.read)(self.0, &mut buffer_size, buffer.as_mut_ptr()) }.into_with(
            || buffer_size,
            |s| {
                if s == Status::BUFFER_TOO_SMALL {
                    Some(buffer_size)
                } else {
                    None
                }
            },
        )
    }

    /// Write data to file
    ///
    /// Write `buffer` to file, increment the file pointer.
    ///
    /// If an error occurs, returns the number of bytes that were actually written. If no error
    /// occured, the entire buffer is guaranteed to have been written successfully.
    ///
    /// Opened directories cannot be written to.
    ///
    /// # Arguments
    /// * `buffer`  Buffer to write to file
    ///
    /// # Errors
    /// * `uefi::Status::UNSUPPORTED`        Attempted to write in a directory.
    /// * `uefi::Status::NO_MEDIA`           The device has no media
    /// * `uefi::Status::DEVICE_ERROR`       The device reported an error or the file was deleted.
    /// * `uefi::Status::VOLUME_CORRUPTED`   The filesystem structures are corrupted
    /// * `uefi::Status::WRITE_PROTECTED`    Attempt to write to readonly file
    /// * `uefi::Status::ACCESS_DENIED`      The file was opened read only.
    /// * `uefi::Status::VOLUME_FULL`        The volume is full
    pub fn write(&mut self, buffer: &[u8]) -> Result<(), usize> {
        let mut buffer_size = buffer.len();
        unsafe { (self.0.write)(self.0, &mut buffer_size, buffer.as_ptr()) }
            .into_with_err(|_| buffer_size)
    }

    /// Get the file's current position
    ///
    /// # Errors
    /// * `uefi::Status::UNSUPPORTED`    Attempted to get the position of an opened directory
    /// * `uefi::Status::DEVICE_ERROR`   An attempt was made to get the position of a deleted file
    pub fn get_position(&mut self) -> Result<u64> {
        let mut pos = 0u64;
        (self.0.get_position)(self.0, &mut pos).into_with_val(|| pos)
    }

    /// Sets the file's current position
    ///
    /// Set the position of this file handle to the absolute position specified by `position`.
    ///
    /// Seeking past the end of the file is allowed, it will trigger file growth on the next write.
    ///
    /// The special value 0xFFFF_FFFF_FFFF_FFFF may be used to seek to the end of the file.
    ///
    /// Seeking directories is only allowed with the special value 0, which has the effect of
    /// resetting the enumeration of directory entries.
    ///
    /// # Arguments
    /// * `position` The new absolution position of the file handle
    ///
    /// # Errors
    /// * `uefi::Status::UNSUPPORTED`    Attempted a nonzero seek on a directory.
    /// * `uefi::Status::DEVICE_ERROR`   An attempt was made to set the position of a deleted file
    pub fn set_position(&mut self, position: u64) -> Result {
        (self.0.set_position)(self.0, position).into()
    }

    /// Queries some information about a file
    ///
    /// The information will be written into a user-provided buffer.
    /// If the buffer is too small, the required buffer size will be returned as part of the error.
    ///
    /// The buffer must be aligned on an `<Info as Align>::alignment()` boundary.
    ///
    /// # Arguments
    /// * `buffer`  Buffer that the information should be written into
    ///
    /// # Errors
    /// * `uefi::Status::UNSUPPORTED`        The file does not possess this information type
    /// * `uefi::Status::NO_MEDIA`           The device has no medium
    /// * `uefi::Status::DEVICE_ERROR`       The device reported an error
    /// * `uefi::Status::VOLUME_CORRUPTED`   The file system structures are corrupted
    /// * `uefi::Status::BUFFER_TOO_SMALL`   The buffer is too small for the requested
    pub fn get_info<'buf, Info: FileProtocolInfo>(
        &mut self,
        buffer: &'buf mut [u8],
    ) -> Result<&'buf mut Info, Option<usize>> {
        let mut buffer_size = buffer.len();
        Info::assert_aligned(buffer);
        unsafe { (self.0.get_info)(self.0, &Info::GUID, &mut buffer_size, buffer.as_mut_ptr()) }
            .into_with(
                || unsafe { Info::from_uefi(buffer.as_ptr() as *mut c_void) },
                |s| {
                    if s == Status::BUFFER_TOO_SMALL {
                        Some(buffer_size)
                    } else {
                        None
                    }
                },
            )
    }

    /// Sets some information about a file
    ///
    /// There are various restrictions on the information that may be modified using this method.
    /// The simplest one is that it is usually not possible to call it on read-only media. Further
    /// restrictions specific to a given information type are described in the corresponding
    /// `FileProtocolInfo` type documentation.
    ///
    /// # Arguments
    /// * `info`  Info that should be set for the file
    ///
    /// # Errors
    /// * `uefi::Status::UNSUPPORTED`       The file does not possess this information type
    /// * `uefi::Status::NO_MEDIA`          The device has no medium
    /// * `uefi::Status::DEVICE_ERROR`      The device reported an error
    /// * `uefi::Status::VOLUME_CORRUPTED`  The file system structures are corrupted
    /// * `uefi::Status::WRITE_PROTECTED`   Attempted to set information on a read-only media
    /// * `uefi::Status::ACCESS_DENIED`     Requested change is invalid for this information type
    /// * `uefi::Status::VOLUME_FULL`       Not enough space left on the volume to change the info
    pub fn set_info<Info: FileProtocolInfo>(&mut self, info: &Info) -> Result {
        let info_ptr = info as *const Info as *const c_void;
        let info_size = mem::size_of_val(&info);
        unsafe { (self.0.set_info)(self.0, &Info::GUID, info_size, info_ptr).into() }
    }

    /// Flushes all modified data associated with the file handle to the device
    ///
    /// # Errors
    /// * `uefi::Status::NO_MEDIA`           The device has no media
    /// * `uefi::Status::DEVICE_ERROR`       The device reported an error
    /// * `uefi::Status::VOLUME_CORRUPTED`   The filesystem structures are corrupted
    /// * `uefi::Status::WRITE_PROTECTED`    The file or medium is write protected
    /// * `uefi::Status::ACCESS_DENIED`      The file was opened read only
    /// * `uefi::Status::VOLUME_FULL`        The volume is full
    pub fn flush(&mut self) -> Result {
        (self.0.flush)(self.0).into()
    }
}

impl<'imp> Drop for File<'imp> {
    fn drop(&mut self) {
        let result: Result = (self.0.close)(self.0).into();
        // The spec says this always succeeds.
        result.expect_success("Failed to close file");
    }
}

/// The function pointer table for the File protocol.
#[repr(C)]
pub(super) struct FileImpl {
    revision: u64,
    open: unsafe extern "win64" fn(
        this: &mut FileImpl,
        new_handle: &mut *mut FileImpl,
        filename: *const Char16,
        open_mode: FileMode,
        attributes: FileAttribute,
    ) -> Status,
    close: extern "win64" fn(this: &mut FileImpl) -> Status,
    delete: extern "win64" fn(this: &mut FileImpl) -> Status,
    read: unsafe extern "win64" fn(
        this: &mut FileImpl,
        buffer_size: &mut usize,
        buffer: *mut u8,
    ) -> Status,
    write: unsafe extern "win64" fn(
        this: &mut FileImpl,
        buffer_size: &mut usize,
        buffer: *const u8,
    ) -> Status,
    get_position: extern "win64" fn(this: &mut FileImpl, position: &mut u64) -> Status,
    set_position: extern "win64" fn(this: &mut FileImpl, position: u64) -> Status,
    get_info: unsafe extern "win64" fn(
        this: &mut FileImpl,
        information_type: &Guid,
        buffer_size: &mut usize,
        buffer: *mut u8,
    ) -> Status,
    set_info: unsafe extern "win64" fn(
        this: &mut FileImpl,
        information_type: &Guid,
        buffer_size: usize,
        buffer: *const c_void,
    ) -> Status,
    flush: extern "win64" fn(this: &mut FileImpl) -> Status,
}

/// Usage flags describing what is possible to do with the file.
///
/// SAFETY: Using a repr(C) enum is safe here because this type is only sent to
///         the UEFI implementation, and never received from it.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u64)]
pub enum FileMode {
    /// The file can be read from
    Read = 1,

    /// The file can be read from and written to
    ReadWrite = 2 | 1,

    /// The file can be read, written, and will be created if it does not exist
    CreateReadWrite = (1 << 63) | 2 | 1,
}

bitflags! {
    /// Attributes describing the properties of a file on the file system.
    pub struct FileAttribute: u64 {
        /// File can only be opened in [`FileMode::READ`] mode.
        const READ_ONLY = 1;
        /// Hidden file, not normally visible to the user.
        const HIDDEN = 1 << 1;
        /// System file, indicates this file is an internal operating system file.
        const SYSTEM = 1 << 2;
        /// This file is a directory.
        const DIRECTORY = 1 << 4;
        /// This file is compressed.
        const ARCHIVE = 1 << 5;
        /// Mask combining all the valid attributes.
        const VALID_ATTR = 0x37;
    }
}
