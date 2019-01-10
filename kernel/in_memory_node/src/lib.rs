#![no_std]
#![feature(alloc)]

/// This crate contains a very basic, generic concrete implementation of the Directory
/// and File traits. 
/// The VFSDirectory and InMemoryFile are intended to be used as regular nodes within the filesystem
/// that require no special functionality as well as for inspiration for creating other concrete implementations
/// of the Directory and File traits. 

#[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate fs_node;
extern crate memory;
extern crate irq_safety;

use alloc::vec::Vec;
use core::ops::DerefMut;
use alloc::string::String;
use fs_node::{StrongAnyDirRef, WeakDirRef, File, FileDirectory};
use memory::{MappedPages, FRAME_ALLOCATOR};
use memory::EntryFlags;


pub struct InMemoryFile {
    /// The name of the file
    name: String,
    // The size of the file in bytes
    size: usize,
    /// The string contents as a file: this primitive can be changed into a more complex struct as files become more complex
    contents: MappedPages,
    /// A weak reference to the parent directory
    parent: WeakDirRef,
}

impl InMemoryFile {
    pub fn new(name: String, contents: &mut [u8], parent: WeakDirRef) -> Result<InMemoryFile, &'static str> {
        let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or("create_contiguous_mapping(): KERNEL_MMI was not yet initialized!")?;
        debug!("ABOUT TO PRINT before if let");
        if let memory::PageTable::Active(ref mut active_table) = kernel_mmi_ref.lock().page_table {
            debug!("ABOUT TO PRINT after if let");
            let mut allocator = try!(FRAME_ALLOCATOR.try().ok_or("Couldn't get Frame Allocator")).lock();
            let pages = memory::allocate_pages_by_bytes(contents.len()).ok_or("could not allocate pages")?;
            let mut mapped_pages = active_table.map_allocated_pages(pages,  EntryFlags::WRITABLE, allocator.deref_mut())?;            
            { // scope this so that the mutable borrow on mapped_pages ends as soon as possible
                let mut dest_slice = mapped_pages.as_slice_mut::<u8>(0, contents.len())?;
                dest_slice.copy_from_slice(contents);
            }
            return Ok(InMemoryFile {
            name: name, 
            size: contents.len(),
            contents: mapped_pages,
            parent: parent
            })
        }
        return Err("could not get active table");
    }
}

impl File for InMemoryFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, &'static str> {
        let num_bytes_read = self.size;
        debug!("before call");
        debug!("length of buffer is {}", buf.len());
        debug!("length of contents is {}", num_bytes_read);
        buf.copy_from_slice(self.contents.as_slice_mut(0, num_bytes_read)?);
        debug!("{}", String::from_utf8(buf.to_vec()).unwrap());
        debug!("READFUNCTION");
        debug!("after call");
        return Ok(num_bytes_read);
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, &'static str> {
        let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or("create_contiguous_mapping(): KERNEL_MMI was not yet initialized!")?;
        if let memory::PageTable::Active(ref mut active_table) = kernel_mmi_ref.lock().page_table {
            let mut allocator = try!(FRAME_ALLOCATOR.try().ok_or("Couldn't get Frame Allocator")).lock();
            let pages = memory::allocate_pages_by_bytes(buf.len()).ok_or("could not allocate pages")?;
            let mapped_pages = active_table.map_allocated_pages(pages,  EntryFlags::WRITABLE, allocator.deref_mut())?;
            self.contents = mapped_pages;
            return Ok(self.contents.size_in_bytes())
        }
        return Err("could not get active table");

    }

    fn seek(&self) { unimplemented!(); }
    fn delete(&self) { unimplemented!(); }
    fn size(&self) -> usize {
        return self.size;
    }
}

impl FileDirectory for InMemoryFile {
    fn get_name(&self) -> String {
        self.name.clone()
    }
    
    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<StrongAnyDirRef, &'static str> {
        return match self.parent.upgrade() {
            Some(parent) => Ok(parent),
            None => Err("could not upgrade parent")
        }
    }

    fn set_parent(&mut self, parent_pointer: WeakDirRef) {
        self.parent = parent_pointer
    }
}