//! File system support protocols.

use super::file::{Directory, File, FileImpl};
use crate::proto::Protocol;
use crate::{unsafe_guid, Result, Status};
use core::ptr;

/// Allows access to a FAT-12/16/32 file system.
///
/// This interface is implemented by some storage devices
/// to allow file access to the contained file systems.
#[repr(C)]
#[unsafe_guid("964e5b22-6459-11d2-8e39-00a0c969723b")]
#[derive(Protocol)]
pub struct SimpleFileSystem {
    revision: u64,
    open_volume: extern "win64" fn(this: &mut SimpleFileSystem, root: &mut *mut FileImpl) -> Status,
}

impl SimpleFileSystem {
    /// Open the root directory on a volume.
    ///
    /// # Errors
    /// * `uefi::Status::UNSUPPORTED` - The volume does not support the requested filesystem type
    /// * `uefi::Status::NO_MEDIA` - The device has no media
    /// * `uefi::Status::DEVICE_ERROR` - The device reported an error
    /// * `uefi::Status::VOLUME_CORRUPTED` - The file system structures are corrupted
    /// * `uefi::Status::ACCESS_DENIED` - The service denied access to the file
    /// * `uefi::Status::OUT_OF_RESOURCES` - The volume was not opened
    /// * `uefi::Status::MEDIA_CHANGED` - The device has a different medium in it
    pub fn open_volume(&mut self) -> Result<Directory> {
        let mut ptr = ptr::null_mut();
        (self.open_volume)(self, &mut ptr)
            .into_with_val(|| unsafe { Directory::from_file(File::new(ptr)) })
    }
}
