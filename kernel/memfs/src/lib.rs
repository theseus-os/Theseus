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
extern crate io;


use alloc::string::String;
use fs_node::{DirRef, WeakDirRef, File, FsNode};
use memory::{MappedPages, get_kernel_mmi_ref, allocate_pages_by_bytes, PteFlags};
use alloc::sync::Arc;
use spin::Mutex;
use fs_node::{FileOrDir, FileRef};
use io::{ByteReader, ByteWriter, IoError, KnownLength};

/// The struct that represents a file in memory that is backed by MappedPages
pub struct MemFile {
    /// The name of the file.
    name: String,
    /// The length in bytes of the file.
    /// Note that this is not the same as the capacity of its underlying MappedPages object. 
    len: usize,
    /// The underlying contents of this file in memory.
    mp: MappedPages,
    /// The parent directory that contains this file.
    parent: WeakDirRef,
}

impl MemFile {
    /// Allocates writable memory space for the given `contents` and creates a new file containing that content in the given `parent` directory.
    pub fn create(name: String, parent: &DirRef) -> Result<FileRef, &'static str> {
        let new_file = Self::from_mapped_pages(MappedPages::empty(), name, 0, parent)?;
        Ok(new_file)
    }

    /// Creates a new `MemFile` in the given `parent` directory with the contents of the given `mapped_pages`.
    pub fn from_mapped_pages(mapped_pages: MappedPages, name: String, len: usize, parent: &DirRef) -> Result<FileRef, &'static str> {
        let memfile = MemFile {
            name,
            len,
            mp: mapped_pages, 
            parent: Arc::downgrade(parent), 
        };
        let file_ref = Arc::new(Mutex::new(memfile)) as FileRef;
        parent.lock().insert(FileOrDir::File(file_ref.clone()))?; // adds the newly created file to the tree
        Ok(file_ref)
    }
}

impl ByteReader for MemFile {
    // read will throw an error if the read offset extends past the end of the file
    fn read_at(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, IoError> {
        if offset >= self.len {
            return Err(IoError::InvalidInput);
        }
        // read from the offset until the end of the file, but not more than the buffer length
        let read_bytes = core::cmp::min(self.len - offset, buffer.len());
        buffer[..read_bytes].copy_from_slice(
            self.mp.as_slice(offset, read_bytes).map_err(IoError::from)?
        ); 
        Ok(read_bytes) 
    }
}

impl ByteWriter for MemFile {
    fn write_at(&mut self, buffer: &[u8], offset: usize) -> Result<usize, IoError> {
        // error out if the underlying mapped pages are already allocated and not writeable
        if !self.mp.flags().is_writable() && self.mp.size_in_bytes() != 0 {
            return Err(IoError::from("MemFile::write(): existing MappedPages were not writable"));
        }
        
        let end = buffer.len() + offset;
        // check to see if we can fit the write buffer into the existing mapped pages region
        if end <= self.mp.size_in_bytes() {
            let dest_slice = self.mp.as_slice_mut::<u8>(offset, buffer.len())?;
            // actually perform the write operation
            dest_slice.copy_from_slice(buffer);
            // if the buffer written into the mapped pages exceeds the current size, we set the new size equal to 
            // this value, otherwise, the size remains the same
            if end > self.len { 
                self.len = end; 
            }
            Ok(buffer.len()) // we wrote all of the requested bytes successfully
        } 
        // if not, we need to reallocate a new mapped pages 
        else {
            // If the mapped pages are empty (this is the first allocation), we make them writable
            let prev_flags = if self.mp.size_in_bytes() == 0 {
                PteFlags::new().valid(true).writable(true).into()
            } 
            // Otherwise, use the existing mapped pages flags
            else {
                self.mp.flags()
            };
            
            let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
            let pages = allocate_pages_by_bytes(end).ok_or("could not allocate pages")?;
            let mut new_mapped_pages = kernel_mmi_ref.lock().page_table.map_allocated_pages(pages, prev_flags)?;
            
            // first, we need to copy over the bytes from the previous mapped pages
            {
                // copy_limit copies bytes to min(the write offset, all the bytes of the existing mapped pages)
                // The write does not overlap with existing content, so we copy all existing content
                let copy_limit = if offset > self.len { 
                    self.len
                } else { // Otherwise, we only copy up to where the overlap begins
                    offset
                };
                let existing_bytes = self.mp.as_slice(0, copy_limit)?;
                let copy_slice = new_mapped_pages.as_slice_mut::<u8>(0, copy_limit)?;
                copy_slice.copy_from_slice(existing_bytes);
            } 
            
            // second, we write the new content into the reallocated mapped pages
            {
                let dest_slice = new_mapped_pages.as_slice_mut::<u8>(offset, buffer.len())?;
                dest_slice.copy_from_slice(buffer); // writes the desired contents into the correct area in the mapped page
            }
            self.mp = new_mapped_pages;
            self.len = end;
            Ok(buffer.len())
        }
    }

    fn flush(&mut self) -> Result<(), IoError> { Ok(()) }
}


impl KnownLength for MemFile {
    fn len(&self) -> usize {
        self.len
    }
}

impl File for MemFile {
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
