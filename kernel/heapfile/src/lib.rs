//! An implementation of in-memory files, backed by heap memory, i.e., `Vec`s.

#![no_std]

// #[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate memory;
extern crate fs_node;
extern crate io;


use alloc::{
    vec::Vec,
    sync::Arc,
    string::String,
};
use io::{ByteReader, ByteWriter, IoError, KnownLength};
use spin::Mutex;
use fs_node::{FileOrDir, FileRef, DirRef, WeakDirRef, File, FsNode};
use memory::MappedPages;

/// A file in memory that is backed by the heap, i.e., a `Vec`.
pub struct HeapFile {
    /// The name of the file.
    name: String,
    /// The actual byte contents of the file.
    vec: Vec<u8>,
    /// The parent directory that contains this file.
    parent: WeakDirRef,
}

impl HeapFile {
    /// Creates a new file with empty content in the given `parent` directory. 
    /// No allocation is performed.
    pub fn create(name: String, parent: &DirRef) -> Result<FileRef, &'static str> {
        Self::from_vec(Vec::new(), name, parent)
    }

    /// Creates a new `HeapFile` in the given `parent` directory with the contents of the given `Vec`.
    /// No additional allocation or reallocation is performed.
    pub fn from_vec(vec: Vec<u8>, name: String, parent: &DirRef) -> Result<FileRef, &'static str> {
        let hf = HeapFile {
            name, 
            vec, 
            parent: Arc::downgrade(parent), 
        };
        let file_ref = Arc::new(Mutex::new(hf)) as FileRef;
        parent.lock().insert(FileOrDir::File(file_ref.clone()))?;
        Ok(file_ref)
    }
}

impl ByteReader for HeapFile {
    fn read_at(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, IoError> {
        if offset >= self.vec.len() {
            return Err(IoError::InvalidInput);
        }
        // read from the offset until the end of the file, but not more than the buffer length
        let read_bytes = core::cmp::min(self.vec.len() - offset, buffer.len());
        buffer[..read_bytes].copy_from_slice(&self.vec[offset..read_bytes]); 
        Ok(read_bytes) 
    }
}

impl ByteWriter for HeapFile {
    fn write_at(&mut self, buffer: &[u8], offset: usize) -> Result<usize, IoError> {
        let final_len = offset + buffer.len();
        // Handle the need for reallocation and padding bytes.
        if final_len > self.vec.len() {
            self.vec.resize(final_len, 0u8);
        }

        // Now, `self.vec` is long enough to accommodate the entire `buffer`.
        self.vec[offset..].copy_from_slice(buffer);
        
        Ok(buffer.len())
    }

    fn flush(&mut self) -> Result<(), IoError> { Ok(()) }
}

impl KnownLength for HeapFile {
    fn len(&self) -> usize {
        self.vec.len()
    }
}

impl File for HeapFile {
    fn as_mapping(&self) -> Result<&MappedPages, &'static str> {
        Err("Mapping a HeapFile as a MappedPages object is unimplemented")
    }
}

impl FsNode for HeapFile {
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
