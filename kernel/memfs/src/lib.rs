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
use memory::{MappedPages, FRAME_ALLOCATOR, EntryFlags};
use alloc::sync::Arc;
use spin::Mutex;
use alloc::boxed::Box;
use fs_node::{FileOrDir, FileRef};

/// The struct that represents a file in memory that is backed by MappedPages
pub struct MemFile {
    /// The name of the file
    name: String,
    // The size of the file in bytes (this is the actual length of meaningful content in the file rather than the size of this file's 
    // mapped pages collection)
    size: usize,
    /// The contents or a seqeunce of bytes as a file: this primitive can be changed into a more complex struct as files become more complex
    mp: MappedPages,
    /// A weak reference to the parent directory
    parent: WeakDirRef,
}

impl MemFile {
    /// Allocates writable memory space for the given `contents` and creates a new file containing that content in the given `parent` directory.
    pub fn new(name: String, parent: &DirRef) -> Result<FileRef, &'static str> {
        let empty_pages = MappedPages::empty();        
        let new_file = Self::from_mapped_pages(empty_pages, name, 0, parent)?;
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
    // read will throw an error if the read offset extends past the end of the file
    fn read(&self, buffer: &mut [u8], offset: usize) -> Result<usize, &'static str> {
        if offset > self.size {
            return Err("read offset exceeds file size");
        } else {
            let read_bytes;
            if buffer.len() + offset > self.size {
                // The amount of bytes we can read is limited by the end of the file;
                read_bytes = self.size - offset;
            } else {
                // Otherwise, we read the entire buffer because the end of the buffer doesn't extend
                // past the end of the file
                read_bytes = buffer.len();
            }
            buffer[..read_bytes].copy_from_slice(self.mp.as_slice(offset, read_bytes)?); 
            Ok(read_bytes) 
        }
    }

    fn write(&mut self, buffer: &[u8], offset: usize) -> Result<usize, &'static str> {
        // The pages are already allocated and are not writeable
        if !self.mp.flags().is_writable() && self.mp.size_in_bytes() != 0 {
            return Err("MemFile::write(): existing MappedPages were not writable");
        }
        
        let end = buffer.len() + offset;
        if end <= self.mp.size_in_bytes() { // We don't perform any realloaction
            // Gets a mutuable reference to the byte portion of the newly mapped pages
            let dest_slice = self.mp.as_slice_mut::<u8>(offset, buffer.len())?;
            // The destination slice is guranteed to be the same length as the source slice by the virtue
            // of entering this conditional
            dest_slice.copy_from_slice(buffer); // writes the desired contents into the correct area in the mapped page
            // if the buffer written into the mapped pages exceeds the current size, we set the new size equal to 
            // this value, otherwise, the size remains the same
            if end > self.size { 
                self.size = end; 
            }
            Ok(buffer.len())
        } else { // we'll allocate a new set of mapped pages
            // If the mapped pages are empty (i.e. haven't been allocated), we make them writable
            // Otherwise, use the existing entry flags
            let prev_flags;
            if self.mp.size_in_bytes() == 0 {
                prev_flags = EntryFlags::WRITABLE;
            } else {
                prev_flags = self.mp.flags();
            }
            // Obtain the active kernel page table
            let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
            if let memory::PageTable::Active(ref mut active_table) = kernel_mmi_ref.lock().page_table {
                let mut allocator = try!(FRAME_ALLOCATOR.try().ok_or("Couldn't get Frame Allocator"));
                // Allocate and map the least number of pages we need to store the information contained in the buffer
                // we'll allocate the buffer length plus the offset because that's guranteed to be the most bytes we
                // need (because it entered this conditional statement)
                let pages = memory::allocate_pages_by_bytes(buffer.len() + offset).ok_or("could not allocate pages")?;
                let mut new_mapped_pages = active_table.map_allocated_pages(pages, prev_flags, allocator.lock().deref_mut())?;            
                { // scoped so that this mutable borrow on mapped_pages ends before the next borrow
                    // first need to copy over the bytes from the previous mapped pages
                    // this copies bytes to min(the write offset, all the bytes of the existing mapped pages)
                    let copy_limit;
                    // The write does not overlap with existing content, so we copy all existing content
                    if offset > self.size { 
                        copy_limit = self.size;
                    } else { // Otherwise, we only copy up to where the overlap begins
                        copy_limit = offset;
                    }
                    let existing_bytes = self.mp.as_slice(0, copy_limit)?;
                    let mut copy_slice = new_mapped_pages.as_slice_mut::<u8>(0, copy_limit)?;
                    // Destination slice is guaranteed to be the same length as the source slice because they both index
                    // with the same beginning and end parameters
                    copy_slice.copy_from_slice(existing_bytes);
                } 
                {
                    // now write the new content into the mapped pages
                    // Gets a mutable reference to the byte portion of the newly mapped pages
                    let mut dest_slice = new_mapped_pages.as_slice_mut::<u8>(offset, buffer.len())?;
                    // Destination slice is guaranteed to be the same length as the source slice because
                    // we allocated enough MappedPages capacity to store the entire write buffer
                    dest_slice.copy_from_slice(buffer); // writes the desired contents into the correct area in the mapped page
                }
                self.mp = new_mapped_pages;
                self.size = end;
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

