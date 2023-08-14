//! Logic for generating Thread-Local Storage (TLS) data image for TLS areas.
//!
//! The two key types are:
//! 1. [`TlsInitializer`]: a "factory" that maintains a list of loaded TLS sections
//!    in order to correctly generate new TLS data images.
//! 2. [`TlsDataImage`]: a generated TLS data image that can be used as the TLS area
//!    for a single task.
//!
//! TODO FIXME: currently we are unsure of the `virt_addr_value`s calculated 
//!             for TLS sections on aarch64. The placement of those sections in the
//!             TLS data image is correct, but relocations against them may not be.

#![no_std]
#![feature(int_roundings)]

extern crate alloc;

use alloc::{sync::Arc, vec::Vec, boxed::Box};
use core::{cmp::max, ops::Deref};
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

/// A "factory" that creates Thread-Local Storage (TLS) data images,
/// which are used to initialize a new `Task`'s TLS area.
#[derive(Debug, Clone)]
pub struct TlsInitializer {
    /// The cached data image (with blank space for the TLS self pointer).
    /// This is used to avoid unnecessarily re-generating the TLS data image
    /// every time a new task is spawned if no TLS data sections have been added.
    data_cache: Vec<u8>,
    /// The status of the above `data_cache`: whether it is ready to be used
    /// immediately or needs to be regenerated.
    cache_status: CacheStatus,
    /// The set of TLS data sections that are defined at link time
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
    /// The ending offset (an exclusive range end bound) of the last TLS section
    /// in the above set of `static_section_offsets`.
    /// This is the offset where the TLS self pointer exists.
    end_of_static_sections: usize,
    /// The set of TLS data sections that come from dynamically-loaded crate object files.
    /// We can control and arbitrarily assign their offsets, and thus,
    /// we place all of these sections **after** the static sections
    /// in the generated TLS data image.
    ///
    /// * On x86_64, these are placed right after the TLS self pointer,
    ///   which itself is right after the static TLS sections.
    /// * On aarch64, these are placed directly after the static TLS sections,
    ///   as there is no TLS self pointer.
    dynamic_section_offsets: RangeMap<usize, StrongSectionRefWrapper>,
    /// The ending offset (an exclusive range end bound) of the last TLS section
    /// in the above set of `dynamic_section_offsets`.
    end_of_dynamic_sections: usize,
} 

/// On x86_64, a TLS self pointer exists at the 0th index/offset into each TLS data image,
/// which is just a pointer to itself.
#[cfg(target_arch = "x86_64")]
const TLS_SELF_POINTER_SIZE: usize = size_of::<usize>();

/// On aarch64, there is no TLS self pointer.
#[cfg(target_arch = "aarch64")]
const TLS_SELF_POINTER_SIZE: usize = 0;

/// Errors that may occur when adding TLS sections to a `TlsInitializer`.
#[derive(Debug)]
pub enum TlsInitializerError {
    /// Inserting a TLS section at the included offset
    /// would erroneously overlap with an existing section. 
    /// This indicates a link-time bug or a bug in the symbol parsing code
    /// that invokes the [`TlsInitializer::add_existing_static_tls_section()`].
    OverlapWithExistingSection(usize),
    /// The included virtual address calculated for a TLS section was invalid.
    InvalidVirtualAddress(usize),
    /// There was insufficient space to insert a TLS section into the TLS data image.
    NoRemainingSpace,
}

impl TlsInitializer {
    /// Creates an empty TLS initializer with no TLS data sections.
    pub const fn empty() -> TlsInitializer {
        TlsInitializer {
            // The data image will be generated lazily on the next request to use it.
            data_cache: Vec::new(),
            cache_status: CacheStatus::Invalidated,
            static_section_offsets: RangeMap::new(),
            end_of_static_sections: 0,
            dynamic_section_offsets: RangeMap::new(),
            end_of_dynamic_sections: 0,
        }
    }

    
    /// Add a TLS section that has pre-determined offset, e.g.,
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
    /// * `tls_section`: the TLS section present in base kernel image.
    /// * `offset`: the offset of this section as determined by the linker.
    ///    This corresponds to the "value" of this section's symbol in the ELF file.
    /// * `total_static_tls_size`: the total size of all statically-known TLS sections,
    ///    including both TLS BSS (`.tbss`) and TLS data (`.tdata`) sections.
    ///
    /// ## Return
    /// * A reference to the newly added and properly modified section, if successful.
    /// * An error if inserting the given `tls_section` at the given `offset`
    ///   would overlap with an existing section. 
    ///   An error occurring here would indicate a link-time bug 
    ///   or a bug in the symbol parsing code that invokes this function.
    #[cfg_attr(target_arch = "aarch64", allow(unused_variables))]
    pub fn add_existing_static_tls_section(
        &mut self,
        mut tls_section: LoadedSection,
        offset: usize,
        total_static_tls_size: usize,
    ) -> Result<StrongSectionRef, TlsInitializerError> {
        #[cfg(target_arch = "aarch64")]
        let original_offset = offset;
        #[cfg(target_arch = "aarch64")]
        let offset = max(16, 8 /* TODO FIXME: pass in the TLS segment's alignment */) + offset;

        let range = offset .. (offset + tls_section.size);
        if self.static_section_offsets.contains_key(&range.start) || 
            self.static_section_offsets.contains_key(&(range.end - 1))
        {
            return Err(TlsInitializerError::OverlapWithExistingSection(offset));
        }

        // Calculate the new value of this section's virtual address based on its offset.
        #[cfg(target_arch = "x86_64")]
        let virt_addr_value = (total_static_tls_size - offset).wrapping_neg();

        #[cfg(target_arch = "aarch64")]
        let virt_addr_value = original_offset;

        tls_section.virt_addr = VirtualAddress::new(virt_addr_value)
            .ok_or(TlsInitializerError::InvalidVirtualAddress(virt_addr_value))?;
        self.end_of_static_sections = max(self.end_of_static_sections, range.end);
        let section_ref = Arc::new(tls_section);
        self.static_section_offsets.insert(range, StrongSectionRefWrapper(section_ref.clone()));
        self.cache_status = CacheStatus::Invalidated;
        Ok(section_ref)
    }

    /// Inserts the given `section` into this TLS area at the next index
    /// (i.e., offset into the TLS area) where the section will fit.
    ///
    /// This also modifies the virtual address field of the given `section`
    /// to hold the proper value based on that offset, which is necessary
    /// for calculating relocation entries that depend on this section.
    ///
    /// Returns a tuple of:
    /// 1. The index at which the new section was inserted, 
    ///    which is the offset from the beginning of the TLS area where the section data starts.
    /// 2. The modified section as a `StrongSectionRef`.
    ///
    /// Returns an Error if there is no remaining space that can fit the section.
    pub fn add_new_dynamic_tls_section(
        &mut self,
        mut section: LoadedSection,
        alignment: usize,
    ) -> Result<(usize, StrongSectionRef), TlsInitializerError> {
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

        let start = start_index.ok_or(TlsInitializerError::NoRemainingSpace)?;

        // Calculate this section's virtual address based the range we reserved for it.
        #[cfg(target_arch = "x86_64")]
        let virt_addr_value = start;

        #[cfg(target_arch = "aarch64")]
        let virt_addr_value = start + self.end_of_static_sections - max(16, 8 /* TODO FIXME: pass in the TLS segment's alignment */);

        section.virt_addr = VirtualAddress::new(virt_addr_value)
            .ok_or(TlsInitializerError::InvalidVirtualAddress(virt_addr_value))?;
        let range = start .. (start + section.size);
        let section_ref = Arc::new(section);
        self.end_of_dynamic_sections = max(self.end_of_dynamic_sections, range.end);
        self.dynamic_section_offsets.insert(range, StrongSectionRefWrapper(section_ref.clone()));
        // Now that we've added a new section, the cached data is invalid.
        self.cache_status = CacheStatus::Invalidated;
        Ok((start, section_ref))
    }

    /// Invalidates the cached data image in this `TlsInitializer` area.
    /// 
    /// This is useful for when a TLS section's data has been modified,
    /// e.g., while performing relocations, 
    /// and thus the data image needs to be re-created by re-reading the section data.
    pub fn invalidate(&mut self) {
        self.cache_status = CacheStatus::Invalidated;
    }

    /// Returns a new copy of the TLS data image.
    ///
    /// This function lazily generates the TLS image data on demand, if needed.
    pub fn get_data(&mut self) -> TlsDataImage {
        let total_section_size = self.end_of_static_sections + self.end_of_dynamic_sections;
        let required_capacity = if total_section_size > 0 { total_section_size + TLS_SELF_POINTER_SIZE } else { 0 };
        if required_capacity == 0 {
            return TlsDataImage { _data: None, ptr: 0 };
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
                if sec.typ == SectionType::TlsData {
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
            // log::debug!("TlsInitializer was invalidated, re-generating data.\n{:#X?}", self);

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
            let mut data_copy: Box<[u8]> = self.data_cache.as_slice().into();
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
            let tls_self_ptr_value = dest_slice.as_ptr() as usize;
            dest_slice.copy_from_slice(&tls_self_ptr_value.to_ne_bytes());
            TlsDataImage {
                _data: Some(data_copy),
                ptr:   tls_self_ptr_value,
            }
        }

        #[cfg(target_arch = "aarch64")] {
            let data_copy: Box<[u8]> = self.data_cache.as_slice().into();
            let ptr = data_copy.as_ptr() as usize;
            TlsDataImage {
                _data: Some(data_copy),
                ptr,
            }
        }
    }
}

/// An initialized TLS area data image ready to be used by a new task.
/// 
/// The data is opaque, but one can obtain a pointer to the TLS area.
/// 
/// The enclosed opaque data is stored as a boxed slice (`Box<[u8]>`)
/// instead of a vector (`Vec<u8>`) because it is instantiated once upon task creation
/// and should never be expanded or shrunk.
/// 
/// The data is "immutable" with respect to Theseus task management functions
/// at the language level.
/// However, the data within this TLS area will be modified directly by code
/// that executes "in" this task, e.g., instructions that access the current TLS area.
#[derive(Debug)]
pub struct TlsDataImage {
    // The data is wrapped in an Option to avoid allocating an empty boxed slice
    // when there are no TLS data sections.
    _data: Option<Box<[u8]>>,
    ptr:   usize,
}
impl TlsDataImage {
    /// Creates an empty TLS data image with no TLS section content.
    pub const fn empty() -> TlsDataImage {
        Self { _data: None, ptr: 0 }
    }

    /// Sets the current CPU's TLS register to point to this TLS data image.
    ///
    /// * On x86_64, this writes to the `FsBase` MSR.
    /// * On ARMv8, this writes to `TPIDR_EL0`.
    pub fn set_as_current_tls_base(&self) {
        #[cfg(target_arch = "x86_64")]
        FsBase::write(VirtAddr::new_truncate(self.ptr as u64));

        #[cfg(target_arch = "aarch64")]
        TPIDR_EL0.set(self.ptr as u64);
    }

    pub fn set_as_current_cls_base(&self) {
        // FIXME: Aarch64
        #[cfg(target_arch = "x86_64")]
        {
            use x86_64::registers::control::{Cr4, Cr4Flags};
            unsafe { Cr4::update(|flags| flags.insert(Cr4Flags::FSGSBASE)) };

            unsafe {
                core::arch::asm!(
                    "wrgsbase {}",
                    in(reg) self.ptr as u64,
                    options(nomem, preserves_flags, nostack),
                )
            }
        };
    }
}

/// The status of a cached TLS area data image.
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
impl Eq for StrongSectionRefWrapper { }
