#![no_std]

extern crate alloc;

use alloc::vec::Vec;

use cpu::CpuId;
use crate_metadata::{LoadedSection, StrongSectionRef};
use sync_spin::SpinMutex;
use tls_initializer::{ClsDataImage, ClsInitializer, LocalStorageInitializerError};

static CLS_INITIALIZER: SpinMutex<ClsInitializer> = SpinMutex::new(ClsInitializer::new());
static CLS_SECTIONS: SpinMutex<Vec<(u32, ClsDataImage)>> = SpinMutex::new(Vec::new());

/// Adds a CLS section with a pre-determined offset to the global CLS
/// initializer.
///
/// The CLS register will not be updated until either [`reload`] or
/// [`reload_current_core`] is called.
pub fn add_static_section(
    section: LoadedSection,
    offset: usize,
    total_static_storage_size: usize,
) -> Result<StrongSectionRef, LocalStorageInitializerError> {
    CLS_INITIALIZER
        .lock()
        .add_existing_static_section(section, offset, total_static_storage_size)
}

/// Adds a dynamic CLS section to the global CLS initializer.
///
/// The CLS register will not be updated until either [`reload`] or
/// [`reload_current_core`] is called.
pub fn add_dynamic_section() {
    todo!();
}

/// Generates a new data image for the current core and sets the CLS register
/// accordingly.
pub fn reload_current_core() {
    let current_cpu = cpu::current_cpu().value();
    log::error!("reloading current core: {current_cpu:?}");

    fn rdmsr(msr: u32) -> u64 {
        unsafe { x86_64::registers::model_specific::Msr::new(msr).read() }
    }
    let rdmsr_cpuid = rdmsr(0xc0000103);
    log::error!("rdmsr cpuid: {rdmsr_cpuid}");

    let mut data = CLS_INITIALIZER.lock().get_data();
    // SAFETY: TODO
    log::info!("setting CLS");
    unsafe { data.set_as_current_cls() };
    log::info!("set CLS");

    let mut sections = CLS_SECTIONS.lock();
    log::error!("before: {sections:#0x?}");
    for (cpu, image) in sections.iter_mut() {
        if *cpu == current_cpu {
            core::mem::swap(image, &mut data);
            return;
        }
    }
    sections.push((current_cpu, data));
    log::error!("after: {sections:#0x?}");
}

pub fn reload() {
    let _initializer = CLS_INITIALIZER.lock();
    todo!("cls_allocator::reload");
    // FIXME: Reload CLS register on all cores.
}
