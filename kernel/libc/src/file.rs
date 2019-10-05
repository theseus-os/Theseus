//! A compatibility layer for memfs files
use memfs::*;
use spin::Mutex;
use hashbrown::HashMap;
use alloc::sync::{Weak, Arc};
use alloc::string::String;
use alloc::borrow::ToOwned;
use fs_node::{DirRef, FileRef};

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
    // TODO: better way to store free file descriptors, for a faster search?
    /// Array to store the avaiability of file descriptors 
    pub static ref FILE_DESCRIPTORS: Mutex<[bool; MAX_FILE_DESCRIPTORS as usize]> = Mutex::new([true; MAX_FILE_DESCRIPTORS as usize]);
    /// Stores all the files created through the libc interface and their corresponding file descriptors. For now each file can only be associated with one file descriptor.
    pub static ref FILES: Mutex<HashMap<c_int, FileRef>> = Mutex::new(HashMap::new());
    /// Stores the 1-to-1 mapping of file directories to their file descriptors
    pub static ref FILE_DIRECTORIES: Mutex<HashMap<String, c_int>> = Mutex::new(HashMap::new());
}

/// Returns the smallest file descriptor that's available,
/// or if there none available, return error code "Quota exceeded"
pub fn find_smallest_fd() -> c_int {
    let mut fds = FILE_DESCRIPTORS.lock();
    let fd = fds.iter().position(|&x| x);
    let fd = match fd {
            Some(x) => {
                fds[x as usize] = false;
                x as c_int
            },
            None => -EDQUOT,
        };
    fd
}

/// Returns the file descriptor to the OS so that it can be reused
pub fn return_fd_to_system(fd: c_int) {
    FILE_DESCRIPTORS.lock()[fd as usize] = true;
}

/// Helper function to get current working directory
fn get_cwd() -> Option<DirRef> {
	if let Some(taskref) = task::get_my_current_task() {
        let locked_task = &taskref.lock();
        let curr_env = locked_task.env.lock();
        return Some(Arc::clone(&curr_env.working_dir));
    }

    None
}

/// Creates a file in the current directory. We assume that the path is only the file name.
pub fn create_file(path: &String) -> FileRef {
    // //separate the name of the file from its parent directory
    // let split = path.rfind("/").unwrap();
    // // we want the "/" to be included in the parent directory, so split off from the next character
    // let file_name = path.split_off(split + 1); 
    MemFile::new(path.to_owned(), &get_cwd().unwrap()).unwrap()
}

fn get_file(fd: c_int) -> FileRef{
    FILES.lock().remove(&fd).unwrap()
}

fn remove_fd_from_directory(path: String) {
    let directories = FILE_DIRECTORIES.lock().remove(&path);
}

/// Open and possibly creates a file.
/// Right now only those files can be opened that were also created through the libc interface.
/// The acceptable "path" values are currently just the file name. All files are created in the cwd.
#[no_mangle]
pub extern "C" fn open(path: &CStr, oflag: c_int, mode: mode_t) -> c_int {
    let mut directories = FILE_DIRECTORIES.lock();
    let file_path = path.to_owned().into_string().unwrap();
    let ret = 
        // check if this file is already created, and return file descriptor
        if directories.contains_key(&file_path) {
            directories.get(&file_path).unwrap().to_owned()
        }
        // if the file is not created and the O_CREAT flag is set, create the file
        else if oflag & O_CREAT == O_CREAT {
            let fd = find_smallest_fd();
            if fd >= 0 {
                let file = create_file(&file_path);
                FILES.lock().insert(fd, file);
                directories.insert(file_path, fd);
            }
            fd
        }
        else {
            error!("Flag {} is not supported", oflag);
            EUNIMPLEMENTED
        };    
    
    ret
}

/// closes a file descriptor, so that it no longer refers to any file and may be reused.
/// Since there is only one file descriptor per file, it also deletes the file.
#[no_mangle]
pub extern "C" fn close(fd: c_int) -> c_int {
    let file = get_file(fd);
    let path = file.lock().get_name();
    remove_fd_from_directory(path);
    return_fd_to_system(fd);

    return 0;
}

// /// Deletes a name and possibly the file it refers to
// fn unlink(path: &CStr) -> c_int {
// }