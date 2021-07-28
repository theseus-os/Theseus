// The code is excepted from the x86_64 version. Below is the
// original licence information.
//
// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.


use core::{mem, slice, ops::Deref};
use alloc::sync::Arc;
use spin::Mutex;
use zerocopy::FromBytes;
use memory_structs::PageRange;
pub use page_allocator::*;
pub use memory_structs::*;

/// Dummy entry flags. For ARM micro controllers, there is no page
/// table entry nor flags. This struct is mainly used to be compatible
/// with existing upper layer codes which expect this struct. Likewise,
/// the variants in this enum have no special meaning, just to make
/// existing code compile.
#[derive(Clone, Copy, Debug)]
pub enum EntryFlags {
    WRITABLE
}

impl EntryFlags {
    /// Always return true.
    pub fn is_writable(&self) -> bool {
        true
    }

    /// Always return true.
    pub fn is_executable(&self) -> bool {
        true
    }
}

/// Pages that are ready to read/write. Since ARM micro controllers
/// do not have paging mechanism, `MappedPages` is merely a wrapper
/// around `AllocatedPages`.
#[derive(Debug)]
pub struct MappedPages {
    pages: AllocatedPages,
    flags: EntryFlags
}

impl Deref for MappedPages {
    type Target = PageRange;
    fn deref(&self) -> &PageRange {
        self.pages.deref()
    }
}

impl MappedPages {
    /// Returns an empty MappedPages object that performs no allocation or mapping actions. 
    /// Can be used as a placeholder, but will not permit any real usage. 
    pub fn empty() -> MappedPages {
        MappedPages {
            pages: AllocatedPages::empty(),
            flags: EntryFlags::WRITABLE
        }
    }

    /// Returns the flags that describe this `MappedPages` page table permissions.
    pub fn flags(&self) -> EntryFlags {
        self.flags
    }


    /// Reinterprets this `MappedPages`'s underlying memory region as a struct of the given type `T`,
    /// i.e., overlays a struct on top of this mapped memory region. 
    /// 
    /// # Requirements
    /// The type `T` must implement the `FromBytes` trait, which is similar to the requirements 
    /// of a "plain old data" type, in that it cannot contain Rust references (`&` or `&mut`).
    /// This makes sense because there is no valid way to reinterpret a region of untyped memory 
    /// as a Rust reference. 
    /// In addition, if we did permit that, a Rust reference created from unchecked memory contents
    /// could never be valid, safe, or sound, as it could allow random memory access 
    /// (just like with an arbitrary pointer dereference) that could break isolation.
    /// 
    /// To satisfy this condition, you can use `#[derive(FromBytes)]` on your struct type `T`,
    /// which will only compile correctly if the struct can be validly constructed 
    /// from "untyped" memory, i.e., an array of bytes.
    /// 
    /// # Arguments
    /// `offset`: the offset into the memory region at which the struct is located (where it should start).
    /// 
    /// Returns a reference to the new struct (`&T`) that is formed from the underlying memory region,
    /// with a lifetime dependent upon the lifetime of this `MappedPages` object.
    /// This ensures safety by guaranteeing that the returned struct reference 
    /// cannot be used after this `MappedPages` object is dropped and unmapped.
    pub fn as_type<T: FromBytes>(&self, offset: usize) -> Result<&T, &'static str> {
        let size = mem::size_of::<T>();

        // check that size of the type T fits within the size of the mapping
        let end = offset + size;
        if end > self.size_in_bytes() {
            return Err("requested type and offset would not fit within the MappedPages bounds");
        }

        // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
        let t: &T = unsafe { 
            &*((self.pages.start_address().value() + offset) as *const T)
        };

        Ok(t)
    }


    /// Same as [`as_type()`](#method.as_type), but returns a *mutable* reference to the type `T`.
    /// 
    /// Thus, it checks to make sure that the underlying mapping is writable.
    pub fn as_type_mut<T: FromBytes>(&mut self, offset: usize) -> Result<&mut T, &'static str> {
        let size = mem::size_of::<T>();

        // check flags to make sure mutability is allowed (otherwise a page fault would occur on a write)
        if !self.flags.is_writable() {
            return Err("as_type_mut(): MappedPages were not writable");
        }
        
        // check that size of type T fits within the size of the mapping
        let end = offset + size;
        if end > self.size_in_bytes() {
            return Err("requested type and offset would not fit within the MappedPages bounds");
        }

        // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
        let t: &mut T = unsafe {
            &mut *((self.pages.start_address().value() + offset) as *mut T)
        };

        Ok(t)
    }


    /// Reinterprets this `MappedPages`'s underlying memory region as a slice of any type.
    /// 
    /// It has similar type requirements as the [`as_type()`](#method.as_type) method.
    /// 
    /// # Arguments
    /// * `byte_offset`: the offset (in number of bytes) into the memory region at which the slice should start.
    /// * `length`: the length of the slice, i.e., the number of `T` elements in the slice. 
    ///   Thus, the slice will go from `offset` to `offset` + (sizeof(`T`) * `length`).
    /// 
    /// Returns a reference to the new slice that is formed from the underlying memory region,
    /// with a lifetime dependent upon the lifetime of this `MappedPages` object.
    /// This ensures safety by guaranteeing that the returned slice 
    /// cannot be used after this `MappedPages` object is dropped and unmapped.
    pub fn as_slice<T: FromBytes>(&self, byte_offset: usize, length: usize) -> Result<&[T], &'static str> {

        // check that size of slice fits within the size of the mapping
        let end = byte_offset + (length * mem::size_of::<T>());
        if end > self.size_in_bytes() {
            return Err("requested slice length and offset would not fit within the MappedPages bounds");
        }

        // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
        let slc: &[T] = unsafe {
            slice::from_raw_parts((self.pages.start_address().value() + byte_offset) as *const T, length)
        };

        Ok(slc)
    }


    /// Same as [`as_slice()`](#method.as_slice), but returns a *mutable* slice. 
    /// 
    /// Thus, it checks to make sure that the underlying mapping is writable.
    pub fn as_slice_mut<T: FromBytes>(&mut self, byte_offset: usize, length: usize) -> Result<&mut [T], &'static str> {
        
        // check flags to make sure mutability is allowed (otherwise a page fault would occur on a write)
        if !self.flags.is_writable() {
            return Err("as_slice_mut(): MappedPages were not writable");
        }

        // check that size of slice fits within the size of the mapping
        let end = byte_offset + (length * mem::size_of::<T>());
        if end > self.size_in_bytes() {
            return Err("requested slice length and offset would not fit within the MappedPages bounds");
        }

        // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
        let slc: &mut [T] = unsafe {
            slice::from_raw_parts_mut((self.pages.start_address().value() + byte_offset) as *mut T, length)
        };

        Ok(slc)
    }


    /// Reinterprets this `MappedPages`'s underlying memory region as an executable function with any signature.
    /// 
    /// # Arguments
    /// * `offset`: the offset (in number of bytes) into the memory region at which the function starts.
    /// * `space`: a hack to satisfy the borrow checker's lifetime requirements.
    /// 
    /// Returns a reference to the function that is formed from the underlying memory region,
    /// with a lifetime dependent upon the lifetime of the given `space` object. 
    ///
    /// TODO FIXME: this isn't really safe as it stands now. 
    /// Ideally, we need to have an integrated function that checks with the mod_mgmt crate 
    /// to see if the size of the function can fit (not just the size of the function POINTER, which will basically always fit)
    /// within the bounds of this `MappedPages` object;
    /// this integrated function would be based on the given string name of the function, like "task::this::foo",
    /// and would invoke this as_func() function directly.
    /// 
    /// We have to accept space for the function pointer to exist, because it cannot live in this function's stack. 
    /// It has to live in stack of the function that invokes the actual returned function reference,
    /// otherwise there would be a lifetime issue and a guaranteed page fault. 
    /// So, the `space` arg is a hack to ensure lifetimes;
    /// we don't care about the actual value of `space`, as the value will be overwritten,
    /// and it doesn't matter both before and after the call to this `as_func()`.
    /// 
    /// The generic `F` parameter is the function type signature itself, e.g., `fn(String) -> u8`.
    /// 
    /// # Examples
    /// Here's how you might call this function:
    /// ```
    /// type PrintFuncSignature = fn(&str) -> Result<(), &'static str>;
    /// let mut space = 0; // this must persist throughout the print_func being called
    /// let print_func: &PrintFuncSignature = mapped_pages.as_func(func_offset, &mut space).unwrap();
    /// print_func("hi");
    /// ```
    /// Because Rust has lexical lifetimes, the `space` variable must have a lifetime at least as long as the  `print_func` variable,
    /// meaning that `space` must still be in scope in order for `print_func` to be invoked.
    /// 
    #[doc(hidden)]
    pub fn as_func<'a, F>(&self, offset: usize, space: &'a mut usize) -> Result<&'a F, &'static str> {
        let size = mem::size_of::<F>();

        // check flags to make sure these pages are executable (otherwise a page fault would occur when this func is called)
        if !self.flags.is_executable() {
            return Err("as_func(): MappedPages were not executable");
        }

        // check that size of the type F fits within the size of the mapping
        let end = offset + size;
        if end > self.size_in_bytes() {
            return Err("requested type and offset would not fit within the MappedPages bounds");
        }

        *space = self.pages.start_address().value() + offset; 

        // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
        let t: &'a F = unsafe {
            mem::transmute(space)
        };

        Ok(t)
    }
}

/// A dummy struct representing a page table. ARM micro controllers do not have
/// paging mechanism. This struct is merely used to be compatible with other
/// existing codes. Mapping `AllocatedPages` always succeeds and returns
/// a `MappedPages`.
pub struct PageTable;

impl PageTable {
    /// Always succeeds and returns a `MappedPages` wrapping the given
    /// `AllocatedPages`.
    pub fn map_allocated_pages(
        &mut self, pages: AllocatedPages,
        _flags: EntryFlags, _allocator: &mut AreaFrameAllocator
    ) -> Result<MappedPages, &'static str> {
        Ok(MappedPages{
            pages,
            flags: EntryFlags::WRITABLE
        })
    }
}

/// A dummy frame allocator. ARM micro controllers do not have paging mechanism. 
/// This struct is merely used to be compatible with other existing codes.
pub struct AreaFrameAllocator;

/// A dummy struct. ARM micro controllers do not have paging mechanism. 
/// This struct is merely used to be compatible with other existing codes.
#[allow(dead_code)]
pub struct MemoryManagementInfo {
    pub page_table: PageTable
}

pub type MmiRef = Arc<Mutex<MemoryManagementInfo>>;
pub type FrameAllocatorRef = Arc<Mutex<AreaFrameAllocator>>;

pub fn get_kernel_mmi_ref() -> Option<MmiRef> {
    Some(Arc::new(Mutex::new(MemoryManagementInfo{page_table: PageTable})))
}

pub fn get_frame_allocator_ref() -> Option<FrameAllocatorRef> {
    Some(Arc::new(Mutex::new(AreaFrameAllocator)))
}
