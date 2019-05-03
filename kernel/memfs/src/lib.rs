#![no_std]

//! This crate contains an implementation of an in-memory filesystem backed by MappedPages from the memory crate
//! This crate allocates memory at page-size granularity, so it's inefficient with memory when creating small files
//! Currently, the read and write operations of the RamFile follows the interface of the std::io read/write operations of the Rust standard library

// #[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate fs_node;
extern crate memory;
extern crate irq_safety;


// use alloc::vec::Vec;
use core::ops::DerefMut;
use alloc::string::String;
use fs_node::{DirRef, WeakDirRef, File, FsNode};
use memory::{MappedPages, get_kernel_mmi_ref, allocate_pages_by_bytes, PageTable, FRAME_ALLOCATOR, EntryFlags};
use alloc::sync::Arc;
use spin::Mutex;
use fs_node::{FileOrDir, FileRef};

/// The struct that represents a file in memory that is backed by MappedPages
pub struct MemFile {
    /// The name of the file.
    name: String,
    /// The size in bytes of the file.
    /// Note that this is not the same as the capacity of its underlying MappedPages object. 
    size: usize,
    /// The underlying contents of this file in memory.
    mp: MappedPages,
    /// The parent directory that contains this file.
    parent: WeakDirRef,
}

impl MemFile {
    /// Allocates writable memory space for the given `contents` and creates a new file containing that content in the given `parent` directory.
    pub fn new(name: String, parent: &DirRef) -> Result<FileRef, &'static str> {
        let new_file = Self::from_mapped_pages(MappedPages::empty(), name, 0, parent)?;
        Ok(new_file)
    }

    /// Creates a new `MemFile` in the given `parent` directory with the contents of the given `mapped_pages`.
    pub fn from_mapped_pages(mapped_pages: MappedPages, name: String, size: usize, parent: &DirRef) -> Result<FileRef, &'static str> {
        let memfile = MemFile {
            name: name, 
            size: size, 
            mp: mapped_pages, 
            parent: Arc::downgrade(parent), 
        };
        let file_ref = Arc::new(Mutex::new(memfile)) as Arc<Mutex<File + Send>>;
        parent.lock().insert(FileOrDir::File(file_ref.clone()))?; // adds the newly created file to the tree
        Ok(file_ref)
    }
}

impl File for MemFile {
    // read will throw an error if the read offset extends past the end of the file
    fn read(&self, buffer: &mut [u8], offset: usize) -> Result<usize, &'static str> {
        if offset > self.size {
            return Err("read offset exceeds file size");
        }
        // read from the offset until the end of the file, but not more than the buffer length
        let read_bytes = core::cmp::min(self.size - offset, buffer.len());
        buffer[..read_bytes].copy_from_slice(self.mp.as_slice(offset, read_bytes)?); 
        Ok(read_bytes) 
    }

    fn write(&mut self, buffer: &[u8], offset: usize) -> Result<usize, &'static str> {
        // error out if the underlying mapped pages are already allocated and not writeable
        if !self.mp.flags().is_writable() && self.mp.size_in_bytes() != 0 {
            return Err("MemFile::write(): existing MappedPages were not writable");
        }
        
        let end = buffer.len() + offset;
        // check to see if we can fit the write buffer into the existing mapped pages region
        if end <= self.mp.size_in_bytes() {
            let dest_slice = self.mp.as_slice_mut::<u8>(offset, buffer.len())?;
            // actually perform the write operation
            dest_slice.copy_from_slice(buffer);
            // if the buffer written into the mapped pages exceeds the current size, we set the new size equal to 
            // this value, otherwise, the size remains the same
            if end > self.size { 
                self.size = end; 
            }
            Ok(buffer.len()) // we wrote all of the requested bytes successfully
        } 
        // if not, we need to reallocate a new mapped pages 
        else {
            // If the mapped pages are empty (this is the first allocation), we make them writable
            let prev_flags = if self.mp.size_in_bytes() == 0 {
                EntryFlags::WRITABLE
            } 
            // Otherwise, use the existing mapped pages flags
            else {
                self.mp.flags()
            };
            
            let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
			let mut kernel_mmi = kernel_mmi_ref.lock();
            if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
                let mut allocator = try!(FRAME_ALLOCATOR.try().ok_or("Couldn't get Frame Allocator"));
                let pages = allocate_pages_by_bytes(end).ok_or("could not allocate pages")?;
                let mut new_mapped_pages = active_table.map_allocated_pages(pages, prev_flags, allocator.lock().deref_mut())?;            
                
                // first, we need to copy over the bytes from the previous mapped pages
                {
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
                    copy_slice.copy_from_slice(existing_bytes);
                } 
                
                // second, we write the new content into the reallocated mapped pages
                {
                    let mut dest_slice = new_mapped_pages.as_slice_mut::<u8>(offset, buffer.len())?;
                    dest_slice.copy_from_slice(buffer); // writes the desired contents into the correct area in the mapped page
                }
                self.mp = new_mapped_pages;
                self.size = end;
                Ok(buffer.len())
            }
            else {
                Err("could not get active table")
            }
        }
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
    
    fn get_parent_dir(&self) -> Option<DirRef> {
        self.parent.upgrade()
    }

    fn set_parent_dir(&mut self, new_parent: WeakDirRef) {
        self.parent = new_parent;
    }
}

