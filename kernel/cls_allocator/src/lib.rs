#![no_std]

extern crate alloc;

use alloc::{sync::Arc, vec::Vec};

use cpu::CpuId;
use crate_metadata::{LoadedSection, StrongSectionRef};
use sync_spin::SpinMutex;
use tls_initializer::{ClsDataImage, ClsInitializer, LocalStorageInitializerError};

static CLS_INITIALIZER: SpinMutex<ClsInitializer> = SpinMutex::new(ClsInitializer::new());
static CLS_SECTIONS: SpinMutex<Vec<(CpuId, ClsDataImage)>> = SpinMutex::new(Vec::new());

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
pub fn add_dynamic_section(
    section: LoadedSection,
    alignment: usize,
) -> Result<(usize, Arc<LoadedSection>), LocalStorageInitializerError> {
    CLS_INITIALIZER
        .lock()
        .add_new_dynamic_section(section, alignment)
}

/// Generates a new data image for the current core and sets the CLS register
/// accordingly.
pub fn reload_current_core() {
    log::info!("one");
    let current_cpu = cpu::current_cpu();

    log::info!("twop");
    let mut data = CLS_INITIALIZER.lock().get_data();

    log::info!("three");
    let mut sections = CLS_SECTIONS.lock();
    for (cpu, image) in sections.iter_mut() {
        if *cpu == current_cpu {
            // We disable preemption so that we can safely access `image` and `data` without
            // it being changed under our noses.
            let _guard = preemption::hold_preemption();

            log::info!("old image: {:0x?}", image);
            log::info!("new image: {:0x?}", data);
            data.inherit(image);
            log::info!("nww image: {:0x?}", data);
            // SAFETY: We only drop `data` after another image has been set as the current
            // CPU local storage.
            unsafe { data.set_as_current_cls() };

            core::mem::swap(image, &mut data);
            return;
        }
    }
    log::info!("three");

    // SAFETY: We only drop `data` after another image has been set as the current
    // CPU local storage.
    unsafe { data.set_as_current_cls() };
    sections.push((current_cpu, data));
}

pub fn reload() {
    let _initializer = CLS_INITIALIZER.lock();
    todo!("cls_allocator::reload");
    // FIXME: Reload CLS register on all cores.
}
