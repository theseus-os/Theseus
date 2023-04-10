#![no_std]

extern crate alloc;

mod arch;

use alloc::collections::BTreeMap;
use cpu::CpuId;
use memory::PteFlags;
use sync_spin::Mutex;

pub use cls_proc::cpu_local;

const CLS_SIZE: usize = 4096;

static CPU_LOCAL_DATA_REGIONS: Mutex<BTreeMap<CpuId, u32>> = Mutex::new(BTreeMap::new());

pub fn init(cpu_id: CpuId) -> Result<(), ()> {
    // TODO: Check it isn't being initialised twice.

    CPU_LOCAL_DATA_REGIONS.lock().insert(cpu_id, 0);

    let data_region = memory::create_mapping(CLS_SIZE, PteFlags::new().writable(true).valid(true))
        .map_err(|_| ())?;
    let pointer = data_region.start_address().value() as *mut u8;

    // TODO: Support custom initialisers.
    unsafe { core::ptr::write_bytes::<u8>(pointer, 0, CLS_SIZE) };

    arch::set_cls_register(data_region.start_address());

    Ok(())
}

/// Allocates a region in the CLS.
///
/// Returns the offset into the CLS at which the regions starts.
pub fn allocate_in_cls(cpu_id: CpuId, size: usize) -> Result<usize, ()> {
    let mut data_regions = CPU_LOCAL_DATA_REGIONS.lock();
    let offset = data_regions.get_mut(&cpu_id).ok_or(())?;

    let free_bytes = CLS_SIZE - *offset as usize;

    if size > free_bytes {
        return Err(());
    }

    let current_offset = *offset as usize;
    *offset += size as u32;

    Ok(current_offset)
}
