use super::boot::{BootServices, MemoryMapIter};
use super::runtime::RuntimeServices;
use super::{cfg, Header, Revision};
use crate::proto::console::text;
use crate::{CStr16, Char16, Handle, Result, ResultExt, Status};
use core::marker::PhantomData;
use core::slice;

/// Marker trait used to provide different views of the UEFI System Table
pub trait SystemTableView {}

/// Marker struct associated with the boot view of the UEFI System Table
pub struct Boot;
impl SystemTableView for Boot {}

/// Marker struct associated with the run-time view of the UEFI System Table
pub struct Runtime;
impl SystemTableView for Runtime {}

/// UEFI System Table interface
///
/// The UEFI System Table is the gateway to all UEFI services which an UEFI
/// application is provided access to on startup. However, not all UEFI services
/// will remain accessible forever.
///
/// Some services, called "boot services", may only be called during a bootstrap
/// stage where the UEFI firmware still has control of the hardware, and will
/// become unavailable once the firmware hands over control of the hardware to
/// an operating system loader. Others, called "runtime services", may still be
/// used after that point, but require a rather specific CPU configuration which
/// an operating system loader is unlikely to preserve.
///
/// We handle this state transition by providing two different views of the UEFI
/// system table, the "Boot" view and the "Runtime" view. An UEFI application
/// is initially provided with access to the "Boot" view, and may transition
/// to the "Runtime" view through the ExitBootServices mechanism that is
/// documented in the UEFI spec. At that point, the boot view of the system
/// table will be destroyed (which conveniently invalidates all references to
/// UEFI boot services in the eye of the Rust borrow checker) and a runtime view
/// will be provided to replace it.
#[repr(transparent)]
pub struct SystemTable<View: SystemTableView> {
    table: &'static SystemTableImpl,
    _marker: PhantomData<View>,
}

// These parts of the UEFI System Table interface will always be available
impl<View: SystemTableView> SystemTable<View> {
    /// Return the firmware vendor string
    pub fn firmware_vendor(&self) -> &CStr16 {
        unsafe { CStr16::from_ptr(self.table.fw_vendor) }
    }

    /// Return the firmware revision
    pub fn firmware_revision(&self) -> Revision {
        self.table.fw_revision
    }

    /// Returns the revision of this table, which is defined to be
    /// the revision of the UEFI specification implemented by the firmware.
    pub fn uefi_revision(&self) -> Revision {
        self.table.header.revision
    }

    /// Returns the config table entries, a linear array of structures
    /// pointing to other system-specific tables.
    pub fn config_table(&self) -> &[cfg::ConfigTableEntry] {
        unsafe { slice::from_raw_parts(self.table.cfg_table, self.table.nr_cfg) }
    }
}

// These parts of the UEFI System Table interface may only be used until boot
// services are exited and hardware control is handed over to the OS loader
#[allow(clippy::mut_from_ref)]
impl SystemTable<Boot> {
    /// Returns the standard input protocol.
    pub fn stdin(&self) -> &mut text::Input {
        unsafe { &mut *self.table.stdin }
    }

    /// Returns the standard output protocol.
    pub fn stdout(&self) -> &mut text::Output {
        let stdout_ptr = self.table.stdout as *const _ as *mut _;
        unsafe { &mut *stdout_ptr }
    }

    /// Returns the standard error protocol.
    pub fn stderr(&self) -> &mut text::Output {
        let stderr_ptr = self.table.stderr as *const _ as *mut _;
        unsafe { &mut *stderr_ptr }
    }

    /// Access runtime services
    pub fn runtime_services(&self) -> &RuntimeServices {
        self.table.runtime
    }

    /// Access boot services
    pub fn boot_services(&self) -> &BootServices {
        unsafe { &*self.table.boot }
    }

    pub fn boot_services_test(&self) -> u64 {
        return 0;
    }
    /// Exit the UEFI boot services
    ///
    /// After this function completes, UEFI hands over control of the hardware
    /// to the executing OS loader, which implies that the UEFI boot services
    /// are shut down and cannot be used anymore. Only UEFI configuration tables
    /// and run-time services can be used, and the latter requires special care
    /// from the OS loader. We model this situation by consuming the
    /// `SystemTable<Boot>` view of the System Table and returning a more
    /// restricted `SystemTable<Runtime>` view as an output.
    ///
    /// The handle passed must be the one of the currently executing image,
    /// which is received by the entry point of the UEFI application. In
    /// addition, the application must provide storage for a memory map, which
    /// will be retrieved automatically (as having an up-to-date memory map is a
    /// prerequisite for exiting UEFI boot services).
    ///
    /// The storage must be aligned like a `MemoryDescriptor`.
    ///
    /// The size of the memory map can be estimated by calling
    /// `BootServices::memory_map_size()`. But the memory map can grow under the
    /// hood between the moment where this size estimate is returned and the
    /// moment where boot services are exited, and calling the UEFI memory
    /// allocator will not be possible after the first attempt to exit the boot
    /// services. Therefore, UEFI applications are advised to allocate storage
    /// for the memory map right before exiting boot services, and to allocate a
    /// bit more storage than requested by memory_map_size.
    ///
    /// If `exit_boot_services` succeeds, it will return a runtime view of the
    /// system table which more accurately reflects the state of the UEFI
    /// firmware following exit from boot services, along with a high-level
    /// iterator to the UEFI memory map.
    pub fn exit_boot_services<'buf>(
        self,
        image: Handle,
        mmap_buf: &'buf mut [u8],
    ) -> Result<(SystemTable<Runtime>, MemoryMapIter<'buf>)> {
        unsafe {
            let boot_services = self.boot_services();

            loop {
                // Fetch a memory map, propagate errors and split the completion
                // FIXME: This sad pointer hack works around a current
                //        limitation of the NLL analysis (see Rust bug 51526).
                let mmap_buf = &mut *(mmap_buf as *mut [u8]);
                let mmap_comp = boot_services.memory_map(mmap_buf)?;
                let (mmap_status, (mmap_key, mmap_iter)) = mmap_comp.split();

                // Try to exit boot services using this memory map key
                let result = boot_services.exit_boot_services(image, mmap_key);

                // Did we fail because the memory map was updated concurrently?
                if result.status() == Status::INVALID_PARAMETER {
                    // If so, fetch another memory map and try again
                    continue;
                } else {
                    // If not, report the outcome of the operation
                    return result.map(|comp| {
                        let st = SystemTable {
                            table: self.table,
                            _marker: PhantomData,
                        };
                        comp.map(|_| (st, mmap_iter)).with_status(mmap_status)
                    });
                }
            }
        }
    }

    /// Clone this boot-time UEFI system table interface
    ///
    /// This is unsafe because you must guarantee that the clone will not be
    /// used after boot services are exited. However, the singleton-based
    /// designs that Rust uses for memory allocation, logging, and panic
    /// handling require taking this risk.
    pub unsafe fn unsafe_clone(&self) -> Self {
        SystemTable {
            table: self.table,
            _marker: PhantomData,
        }
    }
}

// These parts of the UEFI System Table interface may only be used after exit
// from UEFI boot services
impl SystemTable<Runtime> {
    /// Access runtime services
    ///
    /// This is unsafe because UEFI runtime services require an elaborate
    /// CPU configuration which may not be preserved by OS loaders. See the
    /// "Calling Conventions" chapter of the UEFI specification for details.
    pub unsafe fn runtime_services(&self) -> &RuntimeServices {
        self.table.runtime
    }
}

/// The actual UEFI system table
#[repr(C)]
struct SystemTableImpl {
    header: Header,
    /// Null-terminated string representing the firmware's vendor.
    fw_vendor: *const Char16,
    /// Revision of the UEFI specification the firmware conforms to.
    fw_revision: Revision,
    stdin_handle: Handle,
    stdin: *mut text::Input,
    stdout_handle: Handle,
    stdout: *mut text::Output<'static>,
    stderr_handle: Handle,
    stderr: *mut text::Output<'static>,
    /// Runtime services table.
    runtime: &'static RuntimeServices,
    /// Boot services table.
    boot: *const BootServices,
    /// Number of entires in the configuration table.
    nr_cfg: usize,
    /// Pointer to beginning of the array.
    cfg_table: *const cfg::ConfigTableEntry,
}

impl<View: SystemTableView> super::Table for SystemTable<View> {
    const SIGNATURE: u64 = 0x5453_5953_2049_4249;
}
