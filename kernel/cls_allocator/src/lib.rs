#![no_std]

extern crate alloc;

use alloc::{sync::Arc, vec::Vec};

use cpu::CpuId;
use crate_metadata::{LoadedSection, StrongSectionRef};
use local_storage_initializer::{ClsDataImage, ClsInitializer, LocalStorageInitializerError};
use sync_spin::SpinMutex;

static CLS_INITIALIZER: SpinMutex<ClsInitializer> = SpinMutex::new(ClsInitializer::new());
static CLS_REGIONS: SpinMutex<Vec<(CpuId, ClsDataImage)>> = SpinMutex::new(Vec::new());

/// Adds a CLS section with a pre-determined offset to the global CLS
/// initializer.
///
/// The CLS register will not be updated until either [`reload`] or
/// [`reload_current_cpu`] is called.
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
/// [`reload_current_cpu`] is called.
pub fn add_dynamic_section(
    section: LoadedSection,
    alignment: usize,
) -> Result<(usize, Arc<LoadedSection>), LocalStorageInitializerError> {
    CLS_INITIALIZER
        .lock()
        .add_new_dynamic_section(section, alignment)
}

/// Generates a new data image for the current CPU and sets the CLS register
/// accordingly.
pub fn reload_current_cpu() {
    let current_cpu = cpu::current_cpu();

    let mut data = CLS_INITIALIZER.lock().get_data();

    let mut regions = CLS_REGIONS.lock();
    for (cpu, image) in regions.iter_mut() {
        if *cpu == current_cpu {
            // We disable interrupts so that we can safely access `image` and `data` without
            // them being changed under our nose.
            let _guard = irq_safety::hold_interrupts();

            data.inherit(image);

            // SAFETY: We only drop `data` after another image has been set as the current
            // CPU local storage.
            unsafe { data.set_as_current_cls() };

            core::mem::swap(image, &mut data);
            return;
        }
    }

    // SAFETY: We only drop `data` after another image has been set as the current
    // CPU local storage.
    unsafe { data.set_as_current_cls() };
    regions.push((current_cpu, data));
}

pub fn reload() {
    todo!("cls_allocator::reload");
    // FIXME: Reload CLS register on all CPUs.
}
