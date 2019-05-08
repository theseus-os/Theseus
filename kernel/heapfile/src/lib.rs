//! An implementation of in-memory files, backed by heap memory, i.e., `Vec`s.

#![no_std]

#[cfg(test)]
#[macro_use] extern crate std;

// #[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate memory;
extern crate fs_node;


use alloc::{
    vec::Vec,
    sync::Arc,
    string::String,
};
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
    pub fn new(name: String, parent: &DirRef) -> Result<FileRef, &'static str> {
        Self::from_vec(Vec::new(), name, parent)
    }

    /// Creates a new `HeapFile` in the given `parent` directory with the contents of the given `Vec`.
    /// No additional allocation or reallocation is performed.
    pub fn from_vec(vec: Vec<u8>, name: String, parent: &DirRef) -> Result<FileRef, &'static str> {
        let hf = HeapFile {
            name: name, 
            vec: vec, 
            parent: Arc::downgrade(parent), 
        };
        let file_ref = Arc::new(Mutex::new(hf)) as Arc<Mutex<File + Send>>;
        parent.lock().insert(FileOrDir::File(file_ref.clone()))?;
        Ok(file_ref)
    }
}

impl File for HeapFile {
    fn read(&self, buffer: &mut [u8], offset: usize) -> Result<usize, &'static str> {
        if offset > self.vec.len() {
            return Err("read offset exceeds file size");
        }
        // read from the offset until the end of the file, but not more than the buffer length
        let read_bytes = core::cmp::min(self.vec.len() - offset, buffer.len());
        buffer[..read_bytes].copy_from_slice(&self.vec[offset..read_bytes]); 
        Ok(read_bytes) 
    }

    fn write(&mut self, buffer: &[u8], offset: usize) -> Result<usize, &'static str> {
        if offset > self.vec.len() {
            return Err("offset out of bounds");
        }

        // optimization for first write of an empty HeapFile
        if self.vec.is_empty() {
            self.vec = buffer.to_vec();
            return Ok(buffer.len());
        }
        
        let end_bound = buffer.len() + offset;
        // first, do a fast memcpy of everything that can fit in the existing vector.
        let copy_count = core::cmp::min(self.vec.len() - offset, buffer.len());
        self.vec[offset..(offset + copy_count)].copy_from_slice(&buffer[..copy_count]);

        // second, if necessary, resize capacity and extend the vec with everything beyond its bounds.
        if end_bound > self.vec.len() {
            // reallocation is needed, we should do it all at once for better performance
            let additional_capacity = end_bound - self.vec.len();
            self.vec.reserve(additional_capacity);
            self.vec.extend_from_slice(&buffer[copy_count..]);
        }
        else {
            // no reallocation needed
        }
        Ok(buffer.len())
    }

    fn size(&self) -> usize {
        self.vec.len()
    }

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



/******************** TESTS BELOW *******************/

// To run this:  cargo test heapfile_test -- --nocapture
#[test]
pub fn heapfile_test() {
    println!("in heapfile_test");

    let mut hf = HeapFile {
        name: String::from("test"),
        vec: Vec::new(),
        parent: unsafe { core::mem::uninitialized() },
    };

    println!("calling write1");
    let count = hf.write(&[1, 2, 3], 0).unwrap();
    println!("after write1: count {}, {:?}", count, hf.vec);

    println!("calling write2");
    let count = hf.write(&[2; 16], 3).unwrap();
    println!("after write2: count {}, {:?}", count, hf.vec);

    println!("calling write3");
    let count = hf.write(&[9; 40], 7).unwrap();
    println!("after write3: count {}, {:?}", count, hf.vec);

    println!("calling write4");
    let count = hf.write(&[10, 11, 12, 13], 27).unwrap();
    println!("after write4: count {}, {:?}", count, hf.vec);

    println!("calling write5");
    let count = hf.write(&[20; 5], 45).unwrap();
    println!("after write5: count {}, {:?}", count, hf.vec);

    // the uninialized `parent` mem will cause a crash when dropped,
    // so we must forget it to prevent it from being dropped.
    let HeapFile { parent, .. } = hf;
    core::mem::forget(parent);
}