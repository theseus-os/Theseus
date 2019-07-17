
use core::cell::UnsafeCell;
use uefi::proto::Protocol;
use uefi::table::boot::{BootServices, SearchType};
use uefi::{Handle, Result};

use alloc::vec::Vec;

/// Utility functions for the UEFI boot services.
pub trait BootServicesExt {
    /// Returns all the handles implementing a certain protocol.
    fn find_handles<P: Protocol>(&self) -> Result<Vec<Handle>>;

    /// Returns a protocol implementation, if present on the system.
    ///
    /// The caveats of `BootServices::handle_protocol()` also apply here.
    fn find_protocol<P: Protocol>(&self) -> Result<&UnsafeCell<P>>;
}

impl BootServicesExt for BootServices {
    fn find_handles<P: Protocol>(&self) -> Result<Vec<Handle>> {
        // Search by protocol.
        let search_type = SearchType::from_proto::<P>();

        // Determine how much we need to allocate.
        let (status1, buffer_size) = self.locate_handle(search_type, None)?.split();

        // Allocate a large enough buffer.
        let mut buffer = Vec::with_capacity(buffer_size);

        unsafe {
            buffer.set_len(buffer_size);
        }

        // Perform the search.
        let (status2, buffer_size) = self.locate_handle(search_type, Some(&mut buffer))?.split();

        // Once the vector has been filled, update its size.
        unsafe {
            buffer.set_len(buffer_size);
        }

        // Emit output, with warnings
        status1
            .into_with_val(|| buffer)
            .map(|completion| completion.with_status(status2))
    }

    fn find_protocol<P: Protocol>(&self) -> Result<&UnsafeCell<P>> {
        // Retrieve all handles implementing this.
        let (status1, handles) = self.find_handles::<P>()?.split();

        // There should be at least one, otherwise find_handles would have
        // aborted with a NOT_FOUND error.
        let handle = *handles.first().unwrap();

        // Similarly, if the search is implemented properly, trying to open
        // the first output handle should always succeed
        // FIXME: Consider using the EFI 1.1 LocateProtocol API instead
        let (status2, protocol) = self.handle_protocol::<P>(handle)?.split();

        // Emit output, with warnings
        status1
            .into_with_val(|| protocol)
            .map(|completion| completion.with_status(status2))
    }
}
