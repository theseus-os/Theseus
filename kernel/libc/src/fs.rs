//! A compatibility layer for memfs files
use memfs::*;
use spin::Mutex;
use hashbrown::HashMap;
use alloc::sync::{Weak, Arc};
use alloc::string::String;
use alloc::borrow::ToOwned;
use alloc::vec::Vec;
use fs_node::{Directory, DirRef, FileRef, FileOrDir};
use core::ops::DerefMut;
use vfs_node::VFSDirectory;

use crate:: {
    types::*,
    errno::*,
    fcntl::*,
    c_str::*,
};


// TODO: Should this be in the kernel config crate?
/// Max number of file descriptors available
pub const MAX_FILE_DESCRIPTORS: u16 = 1024;

lazy_static! {
    /// File Descriptor Table. The index into the table refers the file descriptor and the value stored is a reference to the file.
    pub static ref FILE_DESCRIPTORS: Mutex<Vec<Option<FileRef>>> = Mutex::new(Vec::new());
    /// Stores the 1-to-1 mapping of paths to the files. 
    /// This is used for quick searches for the file in open() as well as used to store all files created by the libc interface.
    /// The key is the file name rather than the complete path since all files are created in the cwd.
    pub static ref LIBC_DIRECTORY: DirRef = VFSDirectory::new_libc(String::from("libc"), root::get_root());
}

/// Creates a file in the libc directory
pub fn create_file(file_name: &String) -> FileRef {
    MemFile::new(file_name.to_owned(), &LIBC_DIRECTORY).unwrap()
}

/// Returns the smallest file descriptor that's available,
/// or if there are none available, return error code "Quota exceeded"
pub fn find_smallest_fd(descriptors: &[Option<FileRef>]) -> c_int {
    let fd = descriptors.iter().position(|x| x.is_none());
    let fd = match fd {
            Some(x) => x as c_int,
            None => -EDQUOT,
        };
    fd
}

/// Assigns a file the minimum file descriptor available and stores a reference to the file in the file descriptor table
pub fn associate_file_with_smallest_fd(file: &FileRef, descriptors: &mut [Option<FileRef>]) -> c_int{
    let fd = find_smallest_fd(descriptors);
    if fd >= 0 {
        descriptors[fd as usize] = Some(file.clone());
    }

    fd
}

/// Returns the file descriptor to the OS so that it can be reused
pub fn return_fd_to_system(fd: c_int, descriptors: &mut [Option<FileRef>]) {
    descriptors[fd as usize] = None;
}

/// Opens and possibly creates a file.
/// Right now only those files can be opened that were also created through the libc interface.
/// The acceptable "path" values are currently just the file name. All files are created in the libc directory.
#[no_mangle]
pub extern "C" fn open(path: &CStr, oflag: c_int, mode: mode_t) -> c_int {
    let mut directory = LIBC_DIRECTORY.lock();
    let mut descriptors = FILE_DESCRIPTORS.lock();

    let file_name = path.to_owned().into_string().unwrap();
    let mut file_ref = directory.get_file(&file_name);
    let ret = 
        // check if this file is already created, and return file descriptor
        if file_ref.is_some() {
            associate_file_with_smallest_fd(&file_ref.unwrap(), descriptors.deref_mut())
        }
        // if the file is not created and the O_CREAT flag is set, create the file
        else if oflag & O_CREAT == O_CREAT {
            let file = create_file(&file_name);
            let fd = associate_file_with_smallest_fd(&file, descriptors.deref_mut());
            if fd >= 0 {
                let _ = directory.insert(FileOrDir::File(file));
            }
            fd
        }
        else {
            error!("Flag {} is not supported", oflag);
            -EUNIMPLEMENTED
        };    
    
    ret
}

/// Closes a file descriptor, so that it no longer refers to any file and may be reused.
/// If this is the last file descriptor referring to a file, the file is deleted
#[no_mangle]
pub extern "C" fn close(fd: c_int) -> c_int {
    let mut directory = LIBC_DIRECTORY.lock();
    let mut descriptors = FILE_DESCRIPTORS.lock();

    let count = Arc::strong_count(descriptors[fd as usize].as_ref().unwrap()); 
    if count == 2 {
        let file = descriptors[fd as usize].as_ref().unwrap().clone();
        directory.remove(&FileOrDir::File(file));
    }

    descriptors[fd as usize] = None;
    return_fd_to_system(fd, descriptors.deref_mut());

    return 0;
}

// /// Deletes a name from the file system and possibly the file it refers to.
// fn unlink(path: &CStr) -> c_int {

// }