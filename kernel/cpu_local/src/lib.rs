//! Offers types and macros to declare and access CPU-local storage (per-CPU variables).
//!
//! CPU-local variables cannot be used until after a given CPU has been initialized,
//! i.e., its Local APIC (on x86_64) has been discovered and properly configured.
//! Currently, the [`init()`] routine in this crate should be invoked by
//! another init routine from the `per_cpu` crate.
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
use preemption::{hold_preemption, PreemptionGuard};
use spin::Mutex;

#[cfg(target_arch = "x86_64")]
use x86_64::{registers::model_specific::GsBase, VirtAddr};

/// Info use to obtain a reference to a CPU-local variable; see [`CpuLocal::new()`].
///
/// This struct is marked `#[non_exhaustive]`, so it cannot be instantiated elsewhere.
#[non_exhaustive]
pub struct FixedCpuLocal {
    pub offset: usize,
    pub size:   usize,
    pub align:  usize,
}

// NOTE: These fields must be kept in sync with `cpu_local::FixedCpuLocal`.
pub enum CpuLocalField {
    CpuId,
    PreemptionCount,
    TaskSwitchPreemptionGuard,
    DropAfterTaskSwitch,
}

impl CpuLocalField {
    pub const fn offset(&self) -> usize {
        match self {
            Self::CpuId => 8,
            Self::PreemptionCount => 12,
            Self::TaskSwitchPreemptionGuard => 16,
            Self::DropAfterTaskSwitch => 24,
        }
    }

    // TODO: Do we even need to know the size and alignment?

    // pub const fn size(&self) -> usize {
    //     match self {
    //         Self::CpuId => 4,
    //         Self::PreemptionCount => 1,
    //         Self::TaskSwitchPreemptionGuard => 8,
    //         Self::DropAfterTaskSwitch => 8,
    //     }
    // }

    // pub const fn align(&self) -> usize {
    //     match self {
    //         Self::CpuId => 4,
    //         Self::PreemptionCount => 1,
    //         Self::TaskSwitchPreemptionGuard => 4,
    //         Self::DropAfterTaskSwitch => 8,
    //     }
    // }
}

// TODO: Could come up with a more imaginative name.
pub unsafe trait Field: Sized {
    const FIELD: CpuLocalField;

    // const SIZE_CHECK: () = assert!(Self::FIELD.size() == size_of::<Self>());
    // const ALIGN_CHECK: () = assert!(Self::FIELD.align() == align_of::<Self>());
}

/// A reference to a CPU-local variable.
///
/// Note that this struct doesn't contain an instance of the type `T`,
/// and dropping it has no effect.
pub struct CpuLocal<T: Field>(PhantomData<*mut T>);

impl<T> CpuLocal<T>
where
    T: Field,
{
    /// Creates a new reference to a predefined CPU-local variable.
    ///
    /// # Arguments
    /// * `fixed`: information about this CPU-local variable.
    ///    Must be one of the public constants defined in [`FixedCpuLocal`].
    ///
    /// # Safety
    /// This is unsafe because we currently do not have a way to guarantee
    /// that the caller-provided type `T` is the same as the type intended for use
    /// by the given `FixedCpuLocal`.
    ///
    /// Thus, the caller must guarantee that the type `T` is correct for the
    /// given `FixedCpuLocal`.
    pub const unsafe fn new_fixed() -> Self {
        // FIXME: Should this still be unsafe?
        Self(PhantomData)
    }

    /// Invokes the given `func` with an immutable reference to this `CpuLocal` variable.
    ///
    /// Preemption will be disabled for the duration of this function
    /// in order to ensure that this task cannot be switched away from
    /// or migrated to another CPU.
    ///
    /// If the caller has already disabled preemption, it is more efficient to
    /// use the [`with_preempt()`] function, which allows the caller to pass in
    /// an existing preemption guard to prove that preemption is already disabled.
    pub fn with<F, R>(&self, func: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        let guard = hold_preemption();
        self.with_preempt(&guard, func)
    }

    /// Invokes the given `func` with an immutable reference to this `CpuLocal` variable.
    ///
    /// This function accepts an existing preemption guard, which efficiently proves
    /// that preemption has already been disabled.
    pub fn with_preempt<F, R>(&self, _guard: &PreemptionGuard, func: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        let ptr_to_cpu_local = self.self_ptr() + T::FIELD.offset();
        let local_ref = unsafe { &*(ptr_to_cpu_local as *const T) };
        func(local_ref)
    }

    /// Invokes the given `func` with a mutable reference to this `CpuLocal` variable.
    ///
    /// Interrupts will be disabled for the duration of this function
    /// in order to ensure atomicity while this per-CPU state is being modified.
    pub fn with_mut<F, R>(&self, func: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        let _held_interrupts = irq_safety::hold_interrupts();
        let ptr_to_cpu_local = self.self_ptr() + T::FIELD.offset();
        let local_ref_mut = unsafe { &mut *(ptr_to_cpu_local as *mut T) };
        func(local_ref_mut)
    }

    /// Returns the value of the self pointer, which points to this CPU's `PerCpuData`.
    fn self_ptr(&self) -> usize {
        let self_ptr: usize;
        unsafe {
            #[cfg(target_arch = "x86_64")]
            asm!(
                "mov {}, gs:[0]", // the self ptr offset is 0
                lateout(reg) self_ptr,
                options(nostack, preserves_flags, readonly, pure)
            );

            #[cfg(not(target_arch = "x86_64"))]
            todo!("CPU Locals are not yet supported on non-x86_64 platforms");
        };
        self_ptr 
    }
}

impl<T> CpuLocal<T>
where
    T: Copy + Field,
{
    /// Returns a copy of this `CpuLocal`'s inner value of type `T`.
    ///
    /// This is a convenience function only available for types where `T: Copy`.
    pub fn get(&self) -> T {
        self.with(|v| *v)
    }
}


/// The underlying memory region for each CPU's per-CPU data.
#[derive(Debug)]
struct CpuLocalDataRegion(MappedPages);
impl CpuLocalDataRegion {
    /// Allocates a new CPU-local data image.
    fn new(size_of_per_cpu_data: usize) -> Result<CpuLocalDataRegion, &'static str> {
        let mp = memory::create_mapping(
            size_of_per_cpu_data,
            PteFlags::new().writable(true).valid(true),
        )?;
        Ok(CpuLocalDataRegion(mp))
    }

    /// Sets this CPU's base register (e.g., GsBase on x86_64) to the address
    /// of this CPU-local data image, making it "currently active" and accessible.
    fn set_as_current_cpu_local_base(&self) {
        let self_ptr_value = self.0.start_address().value();

        #[cfg(target_arch = "x86_64")] {
            let gsbase_val = VirtAddr::new_truncate(self_ptr_value as u64);
            log::warn!("Writing value {:#X} to GSBASE", gsbase_val);
            GsBase::write(gsbase_val);
        }

        #[cfg(not(target_arch = "x86_64"))]
        todo!("CPU-local variable access is not yet implemented on this architecture")
    }
}


/// Initializes the CPU-local data region for this CPU.
///
/// Note: this is invoked by the `per_cpu` crate;
///       other crates do not need to invoke this.
pub fn init<P>(
    cpu_id: u32,
    size_of_per_cpu_data: usize,
    per_cpu_data_initializer: impl FnOnce(usize) -> P
) -> Result<(), &'static str> {
    /// The global set of all per-CPU data regions.
    static CPU_LOCAL_DATA_REGIONS: Mutex<BTreeMap<u32, CpuLocalDataRegion>> = Mutex::new(BTreeMap::new());

    let mut regions = CPU_LOCAL_DATA_REGIONS.lock();
    let entry = regions.entry(cpu_id);
    let data_region = match entry {
        Entry::Vacant(v) => v.insert(CpuLocalDataRegion::new(size_of_per_cpu_data)?),
        Entry::Occupied(_) => return Err("BUG: cannot init CPU-local data more than once"),
    };

    // Run the given initializer function to initialize the per-CPU data region.
    {
        let self_ptr = data_region.0.start_address().value();
        let initial_value = per_cpu_data_initializer(self_ptr);
        // SAFETY:
        // ✅ We just allocated memory for the self ptr above, it is only accessible by us.
        // ✅ That memory is mutable (writable) and is aligned to a page boundary.
        // ✅ The memory is at least as large as `size_of::<P>()`.
        unsafe { core::ptr::write(self_ptr as *mut P, initial_value) };
    }

    // Set the new CPU-local data region as active and ready to be used on this CPU.
    data_region.set_as_current_cpu_local_base();

    // TODO Remove, temp tests
    if true {
        // NOTE: The fact that this previously compiled in `cpu_local` is
        // indicative of an unsafe API, as the offsets (and types) are
        // completely arbitrary. There's nothing stopping us from trying to
        // access CpuLocal::<16, String> which would be undefined behaviour.

        // let test_value = CpuLocal::<32, u64>(PhantomData);
        // test_value.with(|tv| log::warn!("Got test_value: {:#X}", *tv));
        // log::warn!("Setting test_value to 0x12345678...");
        // test_value.with_mut(|tv| { *tv = 0x12345678; });
        // test_value.with(|tv| log::warn!("Got test_value: {:#X}", *tv));

        // let test_string = CpuLocal::<40, alloc::string::String>(PhantomData);
        // test_string.with(|s| log::warn!("Got test_string: {:?}", s));
        // let new_str = ", world!";
        // log::warn!("Appending {:?} to test_string...", new_str);
        // test_string.with_mut(|s| s.push_str(new_str));
        // test_string.with(|s| log::warn!("Got test_string: {:?}", s));
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
