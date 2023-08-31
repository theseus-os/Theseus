//! Logic for generating thread-local storage (TLS) and CPU-local storage (CLS) images.
//!
//! The two key types are:
//! 1. [`LocalStorageInitializer`]: a "factory" that maintains a list of loaded sections
//!    in order to correctly generate new local storage data images.
//! 2. [`LocalStorageDataImage`]: a generated local storage data image that can be set
//!    as the current data image.
//!
//! TODO FIXME: currently we are unsure of the `virt_addr_value`s calculated 
//!             for TLS sections on aarch64. The placement of those sections in the
//!             TLS data image is correct, but relocations against them may not be.
//! TODO: We don't really need a TLS self pointer for CLS.

#![no_std]
#![feature(int_roundings)]

extern crate alloc;

use alloc::{sync::Arc, vec::Vec};
use core::{cmp::max, ops::Deref, marker::PhantomData};
use crate_metadata::{LoadedSection, SectionType, StrongSectionRef};
use memory_structs::VirtualAddress;
use rangemap::RangeMap;

#[cfg(target_arch = "x86_64")]
use {
    core::mem::size_of,
    x86_64::{registers::model_specific::FsBase, VirtAddr},
};

#[cfg(target_arch = "aarch64")]
use {
    cortex_a::registers::TPIDR_EL0,
    tock_registers::interfaces::Writeable,
};

pub type TlsInitializer = LocalStorageInitializer<Tls>;
pub type ClsInitializer = LocalStorageInitializer<Cls>;

/// A "factory" that creates local storage data images.
#[derive(Debug, Clone)]
pub struct LocalStorageInitializer<T>
where
    T: LocalStorage,
{
    /// The cached data image (with blank space for the TLS self pointer).
    /// This is used to avoid unnecessarily re-generating the local storage data image
    /// every time a new task is spawned if no data sections have been added.
    data_cache: Vec<u8>,
    /// The status of the above `data_cache`: whether it is ready to be used
    /// immediately or needs to be regenerated.
    cache_status: CacheStatus,
    /// The set of CLS/TLS data sections that are defined at link time
    /// and come from the statically-linked base kernel image (the nano_core).
    ///
    /// On x86_64, the ELF TLS ABI specifies that static TLS sections exist at **negative** offsets
    /// from the TLS self pointer, i.e., they exist **before** the TLS self pointer in memory.
    /// Thus, their actual location in memory depends on the size of **all** static TLS data sections.
    /// For example, the last section in this set (with the highest offset) will be placed
    /// right before the TLS self pointer in memory.
    ///
    /// On aarch64, the ELF TLS ABI specifies only positive offsets,
    /// and there is no TLS self pointer.
    static_section_offsets:  RangeMap<usize, StrongSectionRefWrapper>,
    /// The ending offset (an exclusive range end bound) of the last CLS/TLS section
    /// in the above set of `static_section_offsets`.
    /// This is the offset where the TLS self pointer exists.
    end_of_static_sections: usize,
    /// The set of CLS/TLS data sections that come from dynamically-loaded crate object files.
    /// We can control and arbitrarily assign their offsets, and thus,
    /// we place all of these sections **after** the static sections
    /// in the generated CLS/TLS data image.
    ///
    /// * On x86_64, these are placed right after the TLS self pointer,
    ///   which itself is right after the static CLS/TLS sections.
    /// * On aarch64, these are placed directly after the static CLS/TLS sections,
    ///   as there is no TLS self pointer.
    dynamic_section_offsets: RangeMap<usize, StrongSectionRefWrapper>,
    /// The ending offset (an exclusive range end bound) of the last CLS/TLS section
    /// in the above set of `dynamic_section_offsets`.
    end_of_dynamic_sections: usize,
    _phantom: PhantomData<T>,
} 

pub trait LocalStorage: private::Sealed {}

#[non_exhaustive]
#[derive(Debug)]
pub struct Cls {}

impl Sealed for Cls {
    unsafe fn set_as_current_base(ptr: u64) {
        #[cfg(target_arch = "x86_64")]
        {
            use x86_64::registers::{
                control::{Cr4, Cr4Flags},
                segmentation::{Segment64, GS},
            };
            unsafe { Cr4::update(|flags| flags.insert(Cr4Flags::FSGSBASE)) };
            unsafe { GS::write_base(VirtAddr::new(ptr)) };
        };
        #[cfg(target_arch = "aarch64")]
        {
            use cortex_a::registers::TPIDR_EL1;
            TPIDR_EL1.set(ptr);
        }
    }
}

impl LocalStorage for Cls {}

#[non_exhaustive]
#[derive(Debug)]
pub struct Tls {}

impl Sealed for Tls {
    unsafe fn set_as_current_base(ptr: u64) {
        #[cfg(target_arch = "x86_64")]
        FsBase::write(VirtAddr::new(ptr));

        #[cfg(target_arch = "aarch64")]
        TPIDR_EL0.set(ptr);
    }
}

impl LocalStorage for Tls {}

use private::Sealed;
mod private {
    pub trait Sealed {
        /// Moves `ptr` into the associated register.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn set_as_current_base(ptr: u64);
    }
}

/// On x86_64, a TLS self pointer exists at the 0th index/offset into each TLS data image,
/// which is just a pointer to itself.
#[cfg(target_arch = "x86_64")]
const TLS_SELF_POINTER_SIZE: usize = size_of::<usize>();

/// On aarch64, there is no TLS self pointer.
#[cfg(target_arch = "aarch64")]
const TLS_SELF_POINTER_SIZE: usize = 0;

/// Errors that may occur when adding sections to a `LocalStorageInitializer`.
#[derive(Debug)]
pub enum LocalStorageInitializerError {
    /// Inserting a CLS/TLS section at the included offset
    /// would erroneously overlap with an existing section. 
    /// This indicates a link-time bug or a bug in the symbol parsing code
    /// that invokes the [`LocalStorageInitializer::add_existing_static_tls_section()`].
    OverlapWithExistingSection(usize),
    /// The included virtual address calculated for a CLS/TLS section was invalid.
    InvalidVirtualAddress(usize),
    /// There was insufficient space to insert a CLS/TLS section into the data image.
    NoRemainingSpace,
}

impl<T> LocalStorageInitializer<T>
where
    T: LocalStorage,
{
    /// Creates an empty local storage initializer with no data sections.
    pub const fn new() -> Self {
        LocalStorageInitializer {
            // The data image will be generated lazily on the next request to use it.
            data_cache: Vec::new(),
            cache_status: CacheStatus::Invalidated,
            static_section_offsets: RangeMap::new(),
            end_of_static_sections: 0,
            dynamic_section_offsets: RangeMap::new(),
            end_of_dynamic_sections: 0,
            _phantom: PhantomData,
        }
    }

    
    /// Add a CLS/TLS section that has pre-determined offset, e.g.,
    /// one that was specified in the statically-linked base kernel image.
    ///
    /// This function modifies the `tls_section`'s starting virtual address field
    /// to hold the proper value such that this `tls_section` can be correctly used
    /// as the source of a relocation calculation (e.g., when another section depends on it).
    /// * On x86_64, that value will be the negative offset from the end of 
    ///   all the static TLS sections, i.e., where the TLS self pointer exists in memory,
    ///   to the start of this section in the TLS image.
    ///   * `VirtAddr = -1 * (total_static_tls_size - offset);`
    /// * On aarch64, that value will simply be the given `offset`.
    ///   * `VirtAddr = offset;`.
    ///   * However, on aarch64, the actual *location* of this section in the TLS data image
    ///     is given by `offset + max(16, TLS_segment_align)`.
    ///     The ELF TLS ABI on aarch64 specifies that this augmented value is
    ///     the real offset used to *access* this TLS variable from the TLS base address
    ///     (from the beginning of all sections).
    ///
    /// ## Arguments
    /// * `section`: the section present in base kernel image.
    /// * `offset`: the offset of this section as determined by the linker.
    ///    This corresponds to the "value" of this section's symbol in the ELF file.
    /// * `total_static_size`: the total size of all statically-known CLS/TLS sections,
    ///    including both TLS BSS (`.tbss`) and TLS data (`.tdata`) sections for TLS.
    ///
    /// ## Return
    /// * A reference to the newly added and properly modified section, if successful.
    /// * An error if inserting the given `tls_section` at the given `offset`
    ///   would overlap with an existing section. 
    ///   An error occurring here would indicate a link-time bug 
    ///   or a bug in the symbol parsing code that invokes this function.
    #[cfg_attr(target_arch = "aarch64", allow(unused_variables))]
    pub fn add_existing_static_section(
        &mut self,
        mut section: LoadedSection,
        offset: usize,
        total_static_size: usize,
    ) -> Result<StrongSectionRef, LocalStorageInitializerError> {
        #[cfg(target_arch = "aarch64")]
        let original_offset = offset;
        #[cfg(target_arch = "aarch64")]
        let offset = max(16, 8 /* TODO FIXME: pass in the TLS segment's alignment */) + offset;

        let range = offset .. (offset + section.size);
        if self.static_section_offsets.contains_key(&range.start) || 
            self.static_section_offsets.contains_key(&(range.end - 1))
        {
            return Err(LocalStorageInitializerError::OverlapWithExistingSection(offset));
        }

        // Calculate the new value of this section's virtual address based on its offset.
        #[cfg(target_arch = "x86_64")]
        let virt_addr_value = (total_static_size - offset).wrapping_neg();

        #[cfg(target_arch = "aarch64")]
        let virt_addr_value = original_offset;

        section.virt_addr = VirtualAddress::new(virt_addr_value)
            .ok_or(LocalStorageInitializerError::InvalidVirtualAddress(virt_addr_value))?;
        self.end_of_static_sections = max(self.end_of_static_sections, range.end);
        let section_ref = Arc::new(section);
        self.static_section_offsets.insert(range, StrongSectionRefWrapper(section_ref.clone()));
        self.cache_status = CacheStatus::Invalidated;
        Ok(section_ref)
    }

    /// Inserts the given `section` into this CLS/TLS area at the next index
    /// (i.e., offset into the CLS/TLS area) where the section will fit.
    ///
    /// This also modifies the virtual address field of the given `section`
    /// to hold the proper value based on that offset, which is necessary
    /// for calculating relocation entries that depend on this section.
    ///
    /// Returns a tuple of:
    /// 1. The index at which the new section was inserted, 
    ///    which is the offset from the beginning of the CLS/TLS area where the section data starts.
    /// 2. The modified section as a `StrongSectionRef`.
    ///
    /// Returns an Error if there is no remaining space that can fit the section.
    pub fn add_new_dynamic_section(
        &mut self,
        mut section: LoadedSection,
        alignment: usize,
    ) -> Result<(usize, StrongSectionRef), LocalStorageInitializerError> {
        let mut start_index = None;
        // First, we find the next "gap" big enough to fit the new TLS section.
        // On x86_64, we skip the first `TLS_SELF_POINTER_SIZE` bytes, reserved for the TLS self pointer.
        #[cfg(target_arch = "x86_64")]
        let start_of_search: usize = TLS_SELF_POINTER_SIZE;

        // On aarch64, the ELF TLS ABI specifies that we must 
        #[cfg(target_arch = "aarch64")]
        let start_of_search: usize = max(16, 8 /* TODO FIXME: pass in the TLS SEGMENT's alignment (not the section's alignment)*/);

        for gap in self.dynamic_section_offsets.gaps(&(start_of_search .. usize::MAX)) {
            let aligned_start = gap.start.next_multiple_of(alignment);
            if aligned_start + section.size <= gap.end {
                start_index = Some(aligned_start);
                break;
            }
        }

        let start = start_index.ok_or(LocalStorageInitializerError::NoRemainingSpace)?;

        // Calculate this section's virtual address based the range we reserved for it.
        #[cfg(target_arch = "x86_64")]
        let virt_addr_value = start;

        #[cfg(target_arch = "aarch64")]
        let virt_addr_value = start + self.end_of_static_sections - max(16, 8 /* TODO FIXME: pass in the TLS segment's alignment */);

        section.virt_addr = VirtualAddress::new(virt_addr_value)
            .ok_or(LocalStorageInitializerError::InvalidVirtualAddress(virt_addr_value))?;
        let range = start .. (start + section.size);
        let section_ref = Arc::new(section);
        self.end_of_dynamic_sections = max(self.end_of_dynamic_sections, range.end);
        self.dynamic_section_offsets.insert(range, StrongSectionRefWrapper(section_ref.clone()));
        // Now that we've added a new section, the cached data is invalid.
        self.cache_status = CacheStatus::Invalidated;
        Ok((start, section_ref))
    }

    /// Invalidates the cached data image in this `LocalStorageInitializer` area.
    /// 
    /// This is useful for when a CLS/TLS section's data has been modified,
    /// e.g., while performing relocations, 
    /// and thus the data image needs to be re-created by re-reading the section data.
    pub fn invalidate(&mut self) {
        self.cache_status = CacheStatus::Invalidated;
    }

    /// Returns a new copy of the data image.
    ///
    /// This function lazily generates the image data on demand, if needed.
    pub fn get_data(&mut self) -> LocalStorageDataImage<T> {
        let total_section_size = self.end_of_static_sections + self.end_of_dynamic_sections;
        let required_capacity = if total_section_size > 0 { total_section_size + TLS_SELF_POINTER_SIZE } else { 0 };
        if required_capacity == 0 {
            return LocalStorageDataImage::new();
        }

        // An internal function that iterates over all TLS sections and copies their data into the new data image.
        fn copy_tls_section_data(
            new_data: &mut Vec<u8>,
            section_offsets: &RangeMap<usize, StrongSectionRefWrapper>,
            end_of_previous_range: &mut usize,
        ) {
            for (range, sec) in section_offsets.iter() {
                // Insert padding bytes into the data vec to ensure the section data is inserted at the correct index.
                let num_padding_bytes = range.start.saturating_sub(*end_of_previous_range);
                new_data.extend(core::iter::repeat(0).take(num_padding_bytes));

                // Insert the section data into the new data vec.
                log::info!("adding sec: {sec:?}");
                if (sec.typ == SectionType::TlsData) | (sec.typ == SectionType::Cls) {
                    let sec_mp = sec.mapped_pages.lock();
                    let sec_data: &[u8] = sec_mp.as_slice(sec.mapped_pages_offset, sec.size).unwrap();
                    new_data.extend_from_slice(sec_data);
                } else {
                    // For TLS BSS sections (.tbss), fill the section size with all zeroes.
                    new_data.extend(core::iter::repeat(0).take(sec.size));
                }
                *end_of_previous_range = range.end;
            }
        }

        if self.cache_status == CacheStatus::Invalidated {
            // log::debug!("LocalStorageInitializer was invalidated, re-generating data.\n{:#X?}", self);

            // On some architectures, such as x86_64, the ABI convention REQUIRES that
            // the TLS area data starts with a pointer to itself (the TLS self pointer).
            // Also, all data for "existing" (statically-linked) TLS sections must
            // come *before* the TLS self pointer, i.e., at negative offsets from the TLS self pointer.
            // Thus, we handle that here by appending space for a pointer (one `usize`)
            // to the `new_data` vector after we insert the static TLS data sections.
            // The location of the new pointer value is the conceptual "start" of the TLS image,
            // and that's what should be used for the value of the TLS register (e.g., `FS_BASE` MSR on x86_64).
            //
            // On aarch64, `TLS_SELF_POINTER_SIZE` is 0, so this is still correct.
            let mut new_data: Vec<u8> = Vec::with_capacity(required_capacity);
            
            // Iterate through all static TLS sections and copy their data into the new data image.
            let mut end_of_previous_range: usize = 0;
            copy_tls_section_data(&mut new_data, &self.static_section_offsets, &mut end_of_previous_range);
            assert_eq!(end_of_previous_range, self.end_of_static_sections);

            // Append space for the TLS self pointer immediately after the end of the last static TLS data section;
            // its actual value will be filled in later (in `get_data()`) after a new copy of the TLS data image is made.
            #[cfg(target_arch = "x86_64")]
            new_data.extend_from_slice(&[0u8; TLS_SELF_POINTER_SIZE]);

            // Iterate through all dynamic TLS sections and copy their data into the new data image.
            // If needed (as on x86_64), we already pushed room for the TLS self pointer above.
            end_of_previous_range = TLS_SELF_POINTER_SIZE;
            copy_tls_section_data(&mut new_data, &self.dynamic_section_offsets, &mut end_of_previous_range);
            if self.end_of_dynamic_sections != 0 {
                // this assertion only makes sense if there are any dynamic sections
                assert_eq!(end_of_previous_range, self.end_of_dynamic_sections);
            }

            self.data_cache = new_data;
            self.cache_status = CacheStatus::Fresh;
        }

        // Here, the `data_cache` is guaranteed to be fresh and ready to use.
        #[cfg(target_arch = "x86_64")] {
            let mut data_copy = self.data_cache.clone();
            // Every time we create a new copy of the TLS data image, we have to re-calculate
            // and re-assign the TLS self pointer value (located after the static TLS section data),
            // because the virtual address of that new TLS data image copy will be unique.
            // Note that we only do this if the data_copy actually contains any TLS data.
            let self_ptr_offset = self.end_of_static_sections;
            let Some(dest_slice) = data_copy.get_mut(
                self_ptr_offset .. (self_ptr_offset + TLS_SELF_POINTER_SIZE)
            ) else {
                panic!("BUG: offset of TLS self pointer was out of bounds in the TLS data image:\n{:02X?}", data_copy);
            };
            let tls_self_ptr_value = dest_slice.as_ptr() as u64;
            dest_slice.copy_from_slice(&tls_self_ptr_value.to_ne_bytes());
            LocalStorageDataImage {
                ptr: tls_self_ptr_value,
                data: data_copy,
                _phantom: PhantomData,
            }
        }

        #[cfg(target_arch = "aarch64")] {
            let cloned = self.data_cache.clone();
            LocalStorageDataImage {
                ptr: cloned.as_ptr() as u64,
                data: cloned,
                _phantom: PhantomData,
            }
        }
    }
}

/// An initialized data image ready to be used by a CPU/new task.
/// 
/// The data is opaque, but one can obtain a pointer to the CLS/TLS area.
/// 
/// The data is "immutable" with respect to Theseus task management functions
/// at the language level. However, it will be modified by CLS/TLS accesses.
#[derive(Debug)]
pub struct LocalStorageDataImage<T>
where
    T: LocalStorage,
{
    data: Vec<u8>,
    ptr: u64,
    _phantom: PhantomData<T>,
}

impl<T> LocalStorageDataImage<T>
where
    T: LocalStorage,
{
    /// Creates an empty data image.
    pub const fn new() -> Self {
        Self {
            data: Vec::new(),
            ptr: 0,
            _phantom: PhantomData,
        }
    }

    /// Inherits the data from another data image.
    ///
    /// # Panics
    ///
    /// Panics if the other image has a longer length, or (on x86_64) if the other
    /// image has a differently sized static area.
    pub fn inherit(&mut self, other: &Self) {
        #[cfg(target_arch = "aarch64")]
        {
            let other_len = other.data.len();
            assert!(other_len <= self.data.len());
            self.data[..other_len].clone_from_slice(&other.data[..]);
        }
        #[cfg(target_arch = "x86_64")]
        {
            if !other.data.is_empty() {
                // ```
                //                ptr
                //                 V
                //        +--------+----------+---------+
                // other: | static | self ptr | dynamic |
                //        +--------+----------+---------+
                //        +--------+----------+-------------+
                //  self: | static | self ptr |   dynamic   |
                //        +--------+----------+-------------+
                //                 ^
                //                ptr
                // ```
                let self_static_len = self.ptr as usize - self.data.as_ptr() as usize;
                let other_static_len = other.ptr as usize - other.data.as_ptr() as usize;

                assert_eq!(self_static_len, other_static_len);
                assert!(other.data.len() <= self.data.len());

                self.data[..self_static_len].clone_from_slice(&other.data[..self_static_len]);
                self.data[(self_static_len + TLS_SELF_POINTER_SIZE)..other.data.len()]
                    .clone_from_slice(&other.data[(self_static_len + TLS_SELF_POINTER_SIZE)..]);
            }
        }
    }
}

pub type ClsDataImage = LocalStorageDataImage<Cls>;

pub type TlsDataImage = LocalStorageDataImage<Tls>;

impl LocalStorageDataImage<Cls> {
    /// Sets the data image.
    ///
    /// # Safety
    ///
    /// The data image must not be dropped until another data image replaces it.
    pub unsafe fn set_as_current_cls(&self) {
        // SAFETY: We guarantee that the length of `data` never changes and hence that it is never
        // reallocated. The caller guarantees that `self` and by extension `data` is never dropped.
        // NOTE: This is technically undefined behaviour because we cast the `*const ptr` into a
        // `*mut ptr` in the code generated by `cls_macros`. Obviously, it goes through a system
        // register that Rust cannot possible reason about so it's probably fine?
        unsafe { Cls::set_as_current_base(self.ptr) };
    }
}

impl LocalStorageDataImage<Tls> {
    /// Sets the data image.
    ///
    /// # Safety
    ///
    /// The data image must not be dropped until another data image replaces it, or until
    /// thread-local storage will never be accessed from the current thread again.
    pub unsafe fn set_as_current_tls(&self) {
        // SAFETY: We guarantee that the length of `data` never changes and hence that it is never
        // reallocated. The caller guarantees that `self` and by extension `data` is never dropped.
        unsafe { Tls::set_as_current_base(self.ptr) };
    }
}

/// The status of a cached data image.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CacheStatus {
    /// The cached data image is up to date and can be used immediately.
    Fresh,
    /// The cached data image is out of date and needs to be regenerated.
    Invalidated,
}

/// A wrapper around a `StrongSectionRef` that implements `PartialEq` and `Eq` 
/// so we can use it in a `RangeMap`.
#[derive(Debug, Clone)]
struct StrongSectionRefWrapper(StrongSectionRef);

impl Deref for StrongSectionRefWrapper {
    type Target = StrongSectionRef;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PartialEq for StrongSectionRefWrapper {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for StrongSectionRefWrapper {}
