//! Handles initialization of TLS data images during early OS initialization.
//!
//! This is only useful before the tasking subsystem is initialized, i.e.,
//! in the `nano_core`, `captain`, or `ap_start` crates.

#![no_std]

use local_storage_initializer::TlsDataImage;
use spin::Mutex;

static EARLY_TLS_IMAGE: Mutex<TlsDataImage> = Mutex::new(TlsDataImage::new());

/// Insert the current early TLS image with the given `new_tls_image`,
/// and loads the new image on this CPU.
///
/// If an early TLS image already exists, it is removed and dropped.
pub fn insert(new_tls_image: TlsDataImage) {
    // SAFETY: `new_tls_image` is only dropped if:
    // - `insert` is called again, in which case the next image will replace it
    //   before it is dropped.
    // - `drop` is called in which case the caller guarantees that the task
    //   subsystem has been initialized i.e. the early image has been replaced.
    unsafe { new_tls_image.set_as_current_tls() };
    *EARLY_TLS_IMAGE.lock() = new_tls_image;
}

/// Loads the existing (previously-initialized) early TLS image on this CPU.
pub fn reload() {
    // SAFETY: The early TLS image is only dropped if:
    // - `insert` is called, in which case the next image will replace it before it
    //   is dropped.
    // - `drop` is called in which case the caller guarantees that the task
    //   subsystem has been initialized i.e. the early image has been replaced.
    unsafe { EARLY_TLS_IMAGE.lock().set_as_current_tls() };
}

/// Clears the early TLS image
///
/// # Safety
///
/// This must only be called after the task subsystem is initialized on all
/// CPUs.
pub unsafe fn drop() {
    *EARLY_TLS_IMAGE.lock() = TlsDataImage::new();
}
