//! Offers types and macros to declare and access CPU-local storage (per-CPU variables).
//!
//! CPU-local variables cannot be used until after a given CPU has bee initialized,
//! i.e., its Local APIC (on x86_64) has been discovered and properly configured.
//!
//! Note that Rust offers the `#[thread_local]` attribute for thread-local storage (TLS),
//! but there is no equivalent for CPU-local storage.
//! On x86_64, TLS areas use the `fs` segment register for the TLS base,
//! and this crates uses the `gs` segment register for the CPU-local base.

#![no_std]
#![feature(thread_local)]
extern crate alloc;

use core::{
    arch::asm,
    marker::PhantomData,
    mem::{size_of, align_of},
};
use alloc::collections::{BTreeMap, btree_map::Entry};
use memory::{MappedPages, PteFlags};
use spin::Mutex;

#[cfg(target_arch = "x86_64")]
use x86_64::{registers::model_specific::{GsBase, KernelGsBase}, VirtAddr};

pub struct FixedCpuLocal {
    offset: usize,
    size: usize,
    align: usize,
}
// NOTE: These fields must be kept in sync with `cpu_local::FixedCpuLocal`.
impl FixedCpuLocal {
    const SELF_PTR_OFFSET: usize = 0;
    pub const CPU_ID:                       Self = Self { offset: 8,  size: 4, align: 4 };
    pub const PREEMPTION_COUNT:             Self = Self { offset: 12, size: 1, align: 1 };
    pub const TASK_SWITCH_PREEMPTION_GUARD: Self = Self { offset: 16, size: 8, align: 4 };
    pub const DROP_AFTER_TASK_SWITCH:       Self = Self { offset: 24, size: 8, align: 8 };
    pub const TEST_VALUE:                   Self = Self { offset: 32, size: 8, align: 8 };
}



/// A reference to a CPU-local variable.
///
/// Note that this struct doesn't contain an instance of the type `T`,
/// and dropping it has no effect.
pub struct CpuLocal<const OFFSET: usize, T>(PhantomData<*mut T>);
impl<const OFFSET: usize, T> CpuLocal<OFFSET, T> {
    /// Creates a 
    ///
    /// ## Safety
    /// The caller must guarantee that the type `T` is correct for the
    /// given `FixedCpuLocal`.
    /// This is unsafe because we currently do not have a way to guarantee
    /// that the
    pub const unsafe fn new_fixed(
        fixed: FixedCpuLocal,
    ) -> Self {
        // Compile-time sanity checks.
        assert!(OFFSET == fixed.offset);
        assert!(size_of::<T>()  == fixed.size);
        assert!(align_of::<T>() == fixed.align);
        Self(PhantomData)
    }

    /// Invokes the given `func` with a mutable reference to this `CpuLocal` variable.
    ///
    /// This will initialize this `CpuLocal` if it has not already been initialized.
    ///
    /// TODO: disable preemption
    /// Preemption will be temporarily disabled for the duration of this function
    /// in order to ensure that 
    ///
    /// If the caller has already disabled preemption, they can pass in an optional
    /// preemption guard to prove that preemption is currently disabled.
    pub fn with<F, R>(&self, func: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        let local_ref = unsafe {
            &mut *((self.self_ptr() + OFFSET) as *mut T)
        };
        func(local_ref)
    }

    /// Returns the value of the self pointer, which points to this CPU's `PerCpuData`.
    fn self_ptr(&self) -> usize {
        let self_ptr: usize;
        unsafe {
            #[cfg(target_arch = "x86_64")]
            asm!(
                concat!("mov {}, gs:[0]"), // the SELF_PTR_OFFSET is 0
                lateout(reg) self_ptr,
                options(nostack, preserves_flags, readonly, pure)
            );

            #[cfg(not(target_arch = "x86_64"))]
            todo!("CPU Locals are not yet supported on non-x86_64 platforms");
        };
        self_ptr 
    }
}

impl<const OFFSET: usize, T: Copy> CpuLocal<OFFSET, T> {
    /// Returns a copy of this `CpuLocal`'s inner value of type `T`.
    ///
    /// This is only available for types where `T: Copy`.
    pub fn get(&self) -> T {
        self.with(|v| *v)
    }
}


#[derive(Debug)]
struct CpuLocalDataImage(MappedPages);
impl CpuLocalDataImage {
    /// This function does 3 main things:
    /// 1. Allocates a new CPU-local data image for this CPU.
    /// 2. Sets the self pointer value such that it can be properly accessed.
    /// 3. Sets this CPU's base register (e.g., GsBase on x86_64) to the address
    ///    of this new data image, making it "currently active" and accessible.
    fn new() -> Result<CpuLocalDataImage, &'static str> {
        // 1. Allocate a single page to store each CPU's local data.
        let mut mp = memory::create_mapping(1, PteFlags::new().writable(true).valid(true))?;

        // 2. Set up the self pointer for the new data image.
        let self_ptr_value = mp.start_address().value();
        let self_ptr_dest = mp.as_type_mut::<usize>(0)?;
        *self_ptr_dest = self_ptr_value;

        // 3. Set the base register used for CPU-local data.
        {
            #[cfg(target_arch = "x86_64")] {
                let gsbase_val = VirtAddr::new_truncate(self_ptr_value as u64);
                log::warn!("Writing value {:#X} to GSBASE", gsbase_val);
                GsBase::write(gsbase_val);
                KernelGsBase::write(gsbase_val);
            }

            #[cfg(not(target_arch = "x86_64"))]
            todo!("Per-cpu storage is not yet implemented on this architecture")
        }

        Ok(CpuLocalDataImage(mp))
    }
}


#[inline(never)]
pub fn init<P>(
    cpu_id: u32,
    per_cpu_data_initializer: impl FnOnce(usize) -> P
) -> Result<(), &'static str> {
    static CPU_LOCAL_DATA_REGIONS: Mutex<BTreeMap<u32, CpuLocalDataImage>> = Mutex::new(BTreeMap::new());

    let mut regions = CPU_LOCAL_DATA_REGIONS.lock();
    log::warn!("cpu_local::init(CPU {}): {:?}", cpu_id, regions);
    let entry = regions.entry(cpu_id);
    let data_image = match entry {
        Entry::Vacant(v) => v.insert(CpuLocalDataImage::new()?),
        Entry::Occupied(_) => return Err("BUG: cannot init CPU-local data more than once"),
    };

    let self_ptr = data_image.0.start_address().value();
    // Run the given initializer function on the per-CPU data.
    let new_data_image = CpuLocal::<{FixedCpuLocal::SELF_PTR_OFFSET}, P>(PhantomData);
    new_data_image.with(|per_cpu_data_mut| {
        *per_cpu_data_mut = per_cpu_data_initializer(self_ptr);
    });

    // TODO Remove, temp tests
    if true {
        let test_value = CpuLocal::<8, u64>(PhantomData);
        test_value.with(|tv| log::warn!("Got test_value: {:#X}", *tv));
        log::warn!("Setting test_value to 0x12345678...");
        test_value.with(|tv| { *tv = 0x12345678; });
        test_value.with(|tv| log::warn!("Got test_value: {:#X}", *tv));
        core::mem::forget(test_value);

        let test_string = CpuLocal::<16, alloc::string::String>(PhantomData);
        test_string.with(|s| log::warn!("Got test_string: {:?}", s));
        let new_str = ", world!";
        log::warn!("Appending {:?} to test_string...", new_str);
        test_string.with(|s| s.push_str(new_str));
        test_string.with(|s| log::warn!("Got test_string: {:?}", s));
        core::mem::forget(test_string);
    }
    Ok(())
}


// NOTE:
// we don't currently use this because we always load a pointer to the CpuLocal
// instead of loading or storing the value directly.
// If/when we wish to support these direct loads/stores of values from/to a
// GS-based offset, then we can uncomment this module.
/*
mod load_store_direct {

    mod sealed {
        pub(crate) trait SingleMovGs {
            unsafe fn load(offset: usize) -> Self;
            unsafe fn store(offset: usize, val: Self);
        }
    }
    pub(crate) use sealed::SingleMovGs;

    macro_rules! impl_single_mov_gs {
        ($type:ty, $reg:ident, $reg_str:literal) => {
            impl SingleMovGs for [u8; size_of::<$type>()] {
                #[inline]
                unsafe fn load(offset: usize) -> Self {
                    let val: $type;
                    asm!(
                        concat!("mov ", $reg_str, ", gs:[{}]"),
                        lateout($reg) val, in(reg) offset,
                        options(nostack, preserves_flags, readonly, pure)
                    );
                    val.to_ne_bytes()
                }
                #[inline]
                unsafe fn store(offset: usize, val: Self) {
                    asm!(
                        concat!("mov gs:[{}], ", $reg_str),
                        in(reg) offset, in($reg) <$type>::from_ne_bytes(val),
                        options(nostack, preserves_flags)
                    );
                }
            }
        };
    }

    impl_single_mov_gs!(u64, reg,      "{}");
    impl_single_mov_gs!(u32, reg,      "{:e}");
    impl_single_mov_gs!(u16, reg,      "{:x}");
    impl_single_mov_gs!(u8,  reg_byte, "{}");

    /// Load `SIZE` bytes from the offset relative to the GsBase segment register.
    #[inline]
    unsafe fn load<const SIZE: usize>(offset: usize) -> [u8; SIZE]
    where
        [u8; SIZE]: SingleMovGs,
    {
        SingleMovGs::load(offset)
    }

    /// Store `val` at the offset relative to the GsBase segment register.
    #[inline]
    unsafe fn store<const SIZE: usize>(offset: usize, val: [u8; SIZE])
    where
        [u8; SIZE]: SingleMovGs,
    {
        SingleMovGs::store(offset, val)
    }
}
*/
