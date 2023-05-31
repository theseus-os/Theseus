//! A crate for CPU-local storage.
//!
//! This crate is currently not used, but it is an eventual successor to the
//! `cpu_local` part of `cpu_local_preemption`.
//!
//! This crate uses a similar concept to thread-local storage, where a pointer
//! to a CPU-local area is stored in a designated register (`gs` on x64). This
//! is done by overwriting the segment prefix used by TLS accesses. See the
//! `cls_parser` tool for more details.

#![no_std]
#![feature(allocator_api, error_in_core, negative_impls)]

extern crate alloc;

mod arch;
mod cell;
mod error;

pub use cell::CpuLocalCell;
pub use error::{AllocError, MappingError};

use alloc::collections::BTreeMap;
use cpu::CpuId;
use memory::{MappedPages, PteFlags};
use sync_spin::Mutex;

pub use cls_proc::cpu_local;

/// The size of the CPU-local storage in bytes.
const CLS_SIZE: usize = 4096;

/// The state of all CPU-local storage areas.
static STATE: Mutex<State> = Mutex::new(State::new());

struct State {
    bytes_used: usize,
    data_regions: BTreeMap<CpuId, MappedPages>,
}

impl State {
    const fn new() -> Self {
        Self {
            bytes_used: 0,
            data_regions: BTreeMap::new(),
        }
    }
}

pub fn init(cpu_id: CpuId) -> Result<(), MappingError> {
    let data_region = memory::create_mapping(CLS_SIZE, PteFlags::new().writable(true).valid(true))
        .map_err(|_| MappingError)?;
    let pointer = data_region.start_address().value() as *mut u8;

    // TODO: Support custom initialisers.
    unsafe { core::ptr::write_bytes::<u8>(pointer, 0, CLS_SIZE) };

    let mut state = STATE.lock();
    assert_eq!(state.bytes_used, 0);
    // TODO: Check CLS isn't being initialised twice.
    arch::set_cls_register(data_region.start_address());
    state.data_regions.insert(cpu_id, data_region);

    Ok(())
}

/// Allocates a region in the CLS.
///
/// Returns the offset into the CLS at which the regions starts.
pub fn allocate(size: usize) -> Result<usize, AllocError> {
    let mut state = STATE.lock();
    let offset = state.bytes_used;

    if offset + size > CLS_SIZE {
        return Err(AllocError);
    }
    state.bytes_used += size;

    for (_, region_start) in state.data_regions.iter() {
        let pointer = (region_start.start_address().value() + offset) as *mut u8;
        // TODO: Use custom initialiser.
        unsafe { core::ptr::write_bytes::<u8>(pointer, 0, size) };
    }

    Ok(offset)
}
