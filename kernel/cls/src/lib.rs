#![no_std]

extern crate alloc;

mod arch;

use alloc::collections::BTreeMap;
use cpu::CpuId;
use memory::PteFlags;
use sync_spin::Mutex;

pub use cls_proc::cpu_local;

pub fn init(_cpu_id: CpuId) -> Result<(), ()> {
    // TODO: Check it isn't being initialised twice.
    static CPU_LOCAL_DATA_REGIONS: Mutex<BTreeMap<u32, u32>> =
        Mutex::new(BTreeMap::new());

    let data_region =
        memory::create_mapping(4096, PteFlags::new().writable(true).valid(true)).map_err(|_| ())?;
    let pointer = data_region.start_address().value() as *mut u8;

    // TODO: Support custom initialisers.
    unsafe { core::ptr::write_bytes::<u8>(pointer, 0, 4096) };

    arch::set_cls_register(data_region.start_address());

    Ok(())
}

pub fn allocate_cls(cpu_id: CpuId) -> Result<(), ()> {
    todo!();
}
