//! A compatibility layer for memfs files

use memfs::*;
use spin::{Once, Mutex};
use hashbrown::HashMap;
use alloc::sync::{Weak, Arc};
use alloc::string::String;
use alloc::borrow::ToOwned;
use alloc::vec::Vec;
use fs_node::{Directory, DirRef, FileRef, FileOrDir};
use core::ops::DerefMut;
use vfs_node::VFSDirectory;
use core::sync::atomic::{AtomicI32, Ordering};
use core::slice::{from_raw_parts, from_raw_parts_mut};

use crate:: {
    types::*,
    errno::*,
    fcntl::*,
    c_str::*,
};

/// Max number of file descriptors available
pub const MAX_FILE_DESCRIPTORS: usize = 1024;

lazy_static! {
    /// File Descriptor Table. The index into the table refers to the file descriptor number and the value stored is a reference to the file.
    static ref FILE_DESCRIPTORS: Mutex<Vec<Option<FileRef>>> = Mutex::new(Vec::new());
    /// File Information Table. The index into the table refers to the file descriptor number and the value stored is the file information for that descriptor.
    static ref FILE_INFO: Mutex<[FileInfo; MAX_FILE_DESCRIPTORS]> = Mutex::new([FileInfo{offset:0}; MAX_FILE_DESCRIPTORS]);
}

/// The directory for all files created through the libc interface.
static LIBC_DIRECTORY: Once<DirRef> = Once::new();

/// Returns a reference to the libc directory wrapped in a Mutex,
/// if it exists and has been initialized.
pub fn get_libc_directory() -> Option<&'static DirRef> {
    LIBC_DIRECTORY.try()
}

#[derive(Debug, Copy, Clone)]
/// Struct that stores all additional information about a file.
/// Right now we store just the file offset but it should eventually include all mode and protection bits.
/// There is one such object for each file descriptor.
struct FileInfo {
    /// the byte which the file will start reading or writing from
    pub offset: usize
}

impl FileInfo {
    pub fn clear(&mut self) {
        self.offset = 0;
    }
}

/// Initializes the file descriptor table
pub fn init_file_descriptors() -> Result<(), &'static str>{
    let mut descriptors = FILE_DESCRIPTORS.lock();
    // we don't use 0,1,2 because they're standard file descriptors
    descriptors.push(Some(create_file(&String::from("stdin"))?));
    descriptors.push(Some(create_file(&String::from("stdout"))?));
    descriptors.push(Some(create_file(&String::from("stderr"))?));

    // initialize the empty table of file descriptors with None 
    for _ in 3..MAX_FILE_DESCRIPTORS {
        descriptors.push(None);
    }

    Ok(())
}

/// Continues to try to create a directory until succesful
pub fn create_libc_directory() -> Result<(), &'static str> {
    let libc_dir = VFSDirectory::new(String::from("libc"), root::get_root())?;
    let dir_ref = LIBC_DIRECTORY.call_once(|| libc_dir);
    Ok(())
}

/// Creates a file in the libc directory
pub fn create_file(file_name: &String) -> Result<FileRef, &'static str> {
    MemFile::new(file_name.to_owned(), get_libc_directory().ok_or("libc directory not created")?)
}

/// Returns the smallest file descriptor that's available,
/// or if there are none available, reports error "Quota exceeded".
pub fn find_smallest_fd(descriptors: &[Option<FileRef>]) -> c_int {
    match descriptors.iter().position(|x| x.is_none()) {
        Some(x) => {
            x as c_int
        },
        None => {
            error!("libc::fs::find_smallest_fd(): no descriptor available");
            ERRNO.store(EDQUOT, Ordering::Relaxed);            
            -1
        }
    }
}

/// Assigns a file the minimum file descriptor available and stores a reference to the file in the file descriptor table
pub fn associate_file_with_smallest_fd(file: &FileRef) -> c_int{
    let mut descriptors = FILE_DESCRIPTORS.lock();
    let fd = find_smallest_fd(descriptors.deref_mut());
    if fd >= 0 {
        descriptors[fd as usize] = Some(file.clone());
    }
    fd
}

/// Returns the file descriptor to the OS and clears the file information associated with it.
pub fn return_fd_to_system(fd: c_int) {
    FILE_DESCRIPTORS.lock()[fd as usize] = None;
    FILE_INFO.lock()[fd as usize].clear();
}

/// Opens and possibly creates a file.
/// Only those files can be opened that were also created through the libc interface.
/// 
/// # Arguments
/// * `path`: The acceptable "path" values are currently just the file name. All files are created in the libc directory. 
/// * `oflag`: Specifies different conditions for opening the file. Currently only the O_CREAT flag is supported.
/// * `mode`: The file mode bits to be applied when a new file is created. Currently thse bits are ignored.
#[no_mangle]
pub extern "C" fn open(path: &CStr, oflag: c_int, mode: mode_t) -> c_int {
    let file_name = path.to_owned().into_string().unwrap();
    let mut file_ref = match get_libc_directory() {
        Some(dir) => dir.lock().get_file(&file_name),
        None => {
            error!("libc::fs::open(): libc directory not initialized");
            return -1;
        }
    };

    let rt = 
        // check if this file is already created, and return file descriptor
        if file_ref.is_some() {
            associate_file_with_smallest_fd(&file_ref.unwrap())
        }
        // if the file is not created and the O_CREAT flag is set, create the file
        else if oflag & O_CREAT == O_CREAT {
            let (fd, file) = match create_file(&file_name){
                Ok(x) => (associate_file_with_smallest_fd(&x), x),
                Err(x) => {
                    error!("libc::fs::open(): could not create file: {:?}", x);
                    return -1
                }
            };
            
            let ret = 
                if fd >= 0 {
                    // here unwrap is safe because to get to this point of the function, the libc directory must be initialized
                    match get_libc_directory().unwrap().lock().insert(FileOrDir::File(file)){
                        Ok(x) => fd,
                        Err(x) => {
                            error!("libc::fs:open(): Could not add file to directory");
                            ERRNO.store(EAGAIN,Ordering::Relaxed);
                            -1
                        }
                    }
                }
                else { fd };
            ret
        }
        else {
            error!("libc::fs::open(): Flag {} is not supported", oflag);
            ERRNO.store(EINVAL, Ordering::Relaxed);
            -1
        };    
    
    rt
}

/// Closes a file descriptor, so that it no longer refers to any file and may be reused.
/// If this is the last file descriptor referring to a file, the file is deleted.
#[no_mangle]
pub extern "C" fn close(fd: c_int) -> c_int {
    // The number of strong references a file must have to be deleted.
    // One reference is from the file descriptor "fd", and one is from where it is stored in the libc directory.
    // If there is more than one file descriptor opened for this file then the count will be greater than 2.
    const STRONG_COUNT_TO_DELETE: usize = 2;

    let rt = match FILE_DESCRIPTORS.lock()[fd as usize].as_ref() {
        Some(x) => {
            if Arc::strong_count(x) == STRONG_COUNT_TO_DELETE {
                match get_libc_directory() {
                    Some(dir) => dir.lock().remove(&FileOrDir::File(x.clone())),
                    None => {
                        error!("libc::fs::close(): libc directory not initialized!");
                        return -1;
                    } 
                };
            }
            0
        },
        None => {
            error!("libc::fs::close(): No file found for the given file descriptor");
            ERRNO.store(ENOENT, Ordering::Relaxed);
            -1
        }
    };

    return_fd_to_system(fd);
    rt
}

/// Reads bytes from a file into a buffer
/// 
/// # Arguments
/// * `fd`: file descriptor
/// * `buf`: buffer to write bytes to 
/// * `count`: number of bytes to read
pub extern "C" fn read(fd: c_int, buf: *mut c_void, count: size_t) -> ssize_t {
    let mut file_info = FILE_INFO.lock();
    match FILE_DESCRIPTORS.lock()[fd as usize].as_ref() {
        Some(file) => {
            let offset = file_info[fd as usize].offset;
            let mut buf = unsafe{ from_raw_parts_mut(buf as *mut u8, count as usize) };
                match file.lock().read(buf, offset) {
                Ok(x) => {
                    file_info[fd as usize].offset += x;
                    x as ssize_t
                },
                Err(x) => {
                    error!("libc::fs::read(): Could not read from file: {:?}", x);
                    ERRNO.store(EAGAIN, Ordering::Relaxed);
                    -1
                }

            }
        },
        None => {
            error!("libc::fs::read(): No file found for the given file descriptor");
            ERRNO.store(ENOENT, Ordering::Relaxed);
            -1
        }
    }
}
pub extern "C" fn write(fd: c_int, buf: *const c_void, count: size_t) -> ssize_t {
    let mut file_info = FILE_INFO.lock();
    match FILE_DESCRIPTORS.lock()[fd as usize].as_ref() {
        Some(file) => {
            let mut buf = unsafe{ from_raw_parts(buf as *const u8, count as usize) };
            let offset = file_info[fd as usize].offset;
                match file.lock().write(buf, offset) {
                Ok(x) => {
                    file_info[fd as usize].offset += x;                    
                    x as ssize_t
                },
                Err(x) => {
                    error!("libc::fs::read(): Could not read from file: {:?}", x);
                    ERRNO.store(EAGAIN, Ordering::Relaxed);
                    -1
                }

            }
        },
        None => {
            error!("libc::fs::read(): No file found for the given file descriptor");
            ERRNO.store(ENOENT, Ordering::Relaxed);
            -1
        }
    }
}