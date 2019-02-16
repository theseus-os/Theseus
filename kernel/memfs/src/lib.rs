#![no_std]
#![feature(alloc)]

//! This crate contains an implementation of an in-memory filesystem backed by MappedPages from the memory crate
//! This crate allocates memory at page-size granularity, so it's inefficient with memory when creating small files
//! Currently, the read and write operations of the RamFile follows the interface of the std::io read/write operations of the Rust standard library

#[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate fs_node;
extern crate memory;
extern crate irq_safety;


// use alloc::vec::Vec;
use core::ops::DerefMut;
use alloc::string::String;
use fs_node::{DirRef, WeakDirRef, File, FsNode};
use memory::{MappedPages, FRAME_ALLOCATOR};
use memory::EntryFlags;
use alloc::sync::Arc;
use spin::Mutex;
use alloc::boxed::Box;
use fs_node::{FileOrDir, FileRef};

/// The struct that represents a file in memory that is backed by MappedPages
pub struct MemFile {
    /// The name of the file
    name: String,
    // The size of the file in bytes
    size: usize,
    /// The contents or a seqeunce of bytes as a file: this primitive can be changed into a more complex struct as files become more complex
    mp: MappedPages,
    /// A weak reference to the parent directory
    parent: WeakDirRef,
}

impl MemFile {
    /// Allocates writable memory space for the given `contents` and creates a new file containing that content in the given `parent` directory.
    pub fn new(name: String, parent: &DirRef) -> Result<FileRef, &'static str> {
        let new_file = Self::from_mapped_pages(MappedPages::empty(), name, 0, parent)?;
        Ok(new_file) // 0 because we're creating an empty file
    }

    /// Creates a new `MemFile` in the given `parent` directory with the contents of the given `mapped_pages`.
    pub fn from_mapped_pages(mapped_pages: MappedPages, name: String, size: usize, parent: &DirRef) -> Result<FileRef, &'static str> {
        let memfile = MemFile {
            name: name, 
            size: size, 
            mp: mapped_pages, 
            parent: Arc::downgrade(parent), 
        };
        let file_ref = Arc::new(Mutex::new(Box::new(memfile) as Box<File + Send>));
        parent.lock().insert_child(FileOrDir::File(file_ref.clone()))?; // adds the newly created file to the tree
        Ok(file_ref)
    }
}

impl File for MemFile {
    fn read(&self, buffer: &mut [u8], offset: usize) -> Result<usize, &'static str> {
        if offset > self.size {
            return Err("read offset exceeds file size");
        }
        // we can only copy up to the end of the given buffer or up to the end of the file
        let count = core::cmp::min(buffer.len(), self.size);
        buffer[..count].copy_from_slice(self.mp.as_slice(offset, count)?); 
        Ok(count)
    }

    fn write(&mut self, buffer: &[u8], offset: usize) -> Result<usize, &'static str> {
        if offset > self.size {
            return Err("write offset exceeds file size");
        }
        if buffer.len() + offset <= self.mp.size_in_bytes() {
            { // scoped this so that the mutable borrow on mapped_pages ends as soon as possible
                // Gets a mutuable reference to the byte portion of the newly mapped pages
                let dest_slice = self.mp.as_slice_mut::<u8>(offset, buffer.len())?;
                dest_slice.copy_from_slice(buffer); // writes the desired contents into the correct area in the mapped page
                // if the buffer written into the mapped pages exceeds the current size, we set the new size equal to 
                // this value
                if (buffer.len() + offset) > self.size { 
                    self.size = buffer.len() + offset; 
                }
            }    
            Ok(self.mp.size_in_bytes())
        } else { // we'll allocate a new set of mapped pages
            // Obtain the active kernel page table
            let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
            if let memory::PageTable::Active(ref mut active_table) = kernel_mmi_ref.lock().page_table {
                let mut allocator = try!(FRAME_ALLOCATOR.try().ok_or("Couldn't get Frame Allocator"));
                // Allocate and map the least number of pages we need to store the information contained in the buffer
                // we'll allocate the buffer length plus the offset because that's guranteed to be the most bytes we
                // need (because it entered this conditional statement)
                let pages = memory::allocate_pages_by_bytes(buffer.len() + offset).ok_or("could not allocate pages")?;
                let mut mapped_pages = active_table.map_allocated_pages(pages,  EntryFlags::WRITABLE, allocator.lock().deref_mut())?;            
                { // scoped so that the mutable borrow on mapped_pages ends as soon as possible
                    // first need to copy over the bytes from the previous mapped pages
                    // this copies bytes up to the write offset 
                    let existing_bytes = self.mp.as_slice(0, offset)?;
                    let mut copy_slice = mapped_pages.as_slice_mut::<u8>(0, offset)?;
                    copy_slice.copy_from_slice(existing_bytes);
                }
                {
                    // now write the new content into the mapped pages
                    // Gets a mutable reference to the byte portion of the newly mapped pages
                    let mut dest_slice = mapped_pages.as_slice_mut::<u8>(offset, buffer.len())?;
                    dest_slice.copy_from_slice(buffer); // writes the desired contents into the correct area in the mapped page
                }
                self.mp = mapped_pages;
                self.size = buffer.len();
                return Ok(buffer.len());
            }
            return Err("could not get active table");
        }
    }

    fn delete(self) -> Result<(), &'static str> { 
        Err("unimplemented")
    }

    fn size(&self) -> usize {
        self.size
    }

    fn as_mapping(&self) -> Result<&MappedPages, &'static str> {
        Ok(&self.mp)
    }
    
}

impl FsNode for MemFile {
    fn get_name(&self) -> String {
        self.name.clone()
    }
    
    /// Returns a pointer to the parent if it exists
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        self.parent.upgrade().ok_or("couldn't upgrade parent")
    }
}

