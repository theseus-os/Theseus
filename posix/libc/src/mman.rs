//! memory management functions 

use kernel_config::memory::PAGE_SIZE;
use hashbrown::HashMap;
use memory::{MappedPages, VirtualAddress, FRAME_ALLOCATOR};
use spin::Mutex;
use libm::ceil;
use core::ops::DerefMut;
use core::sync::atomic::{AtomicI32, Ordering};
use libc::*;

use crate::unistd::NULL;


lazy_static! {
    /// Stores the anonymous mappings created by mmap 
    pub static ref MMAP_MAPPINGS: Mutex<HashMap<VirtualAddress, MappedPages>> = Mutex::new(HashMap::new());
}


/// Creates a new mapping in the virtual address space.
/// 
/// # Arguments
/// * `addr`: Starting address of the new mapping. If it's NULL then the kernel decides the address of the new mapping.
/// * `length`: length of the mapping which must be greater than zero.
/// * `prot`: describes the desired memory protection of the mapping.
/// * `flags`: determines whether updates to the mapping are visible to other processes mapping the same region, and whether updates are carried through to the underlying file
/// * `fd`: file descriptor,
/// * `offset`: offset in the file where the mapping will start. It must be a multiple of the page size. 
/// 
/// Currently this function only implements anonymous mappings without a given address, and ignores all protection flags.
#[no_mangle]
pub extern fn mmap(_addr: *mut c_void, length: size_t, prot: c_int, flags: c_int, fd: c_int, offset: off_t) -> *mut c_void {
    // Most systems support MAP_ANON and MAP_FIXED
    if flags & MAP_ANON == MAP_ANON {
        // allocate the number of pages
        let kernel_mmi_ref = memory::get_kernel_mmi_ref().unwrap();
        let allocator = FRAME_ALLOCATOR.try().unwrap();
        let pages = memory::allocate_pages_by_bytes(length as usize).unwrap();
        let mapped_pages = kernel_mmi_ref.lock().page_table.map_allocated_pages(pages, Default::default() , allocator.lock().deref_mut()).unwrap();

        let mapped_pages = match memory::get_kernel_mmi_ref() {
            Some(kernel_mmi_ref) => {
                match FRAME_ALLOCATOR.try() {
                    Some(allocator) => {
                        match memory::allocate_pages_by_bytes(length as usize) {
                            Some(pages) => {
                                kernel_mmi_ref.lock().page_table.map_allocated_pages(pages, Default::default() , allocator.lock().deref_mut())
                            },
                            None => return NULL,
                        } 
                    },
                    None => return NULL,
                }
            },
            None => return NULL,
        };

        match mapped_pages {
            Ok(mp) => {
                let addr = mp.start_address();
                MMAP_MAPPINGS.lock().insert(addr, mp);
                return addr.value() as *mut c_void;
            }
            Err(x) => {
                error!("libc::mman::mmap(): could not get mapped pages: {:?}", x);
                return NULL;
            },
        }
        
    }

    else {
        error!("libc::mman::mmap(): unimplemented flag!");
        return NULL;
    }
}

/// Unmaps memory mapped pages
/// # Arguments
/// * `addr`: starting virtual address of the mapped pages
/// * `len`: the size of the mapping in bytes
#[no_mangle]
pub extern fn munmap(addr: *mut c_void, len: size_t) {
    match VirtualAddress::new(addr as usize) {
        Ok(x) => {MMAP_MAPPINGS.lock().remove(&x);},
        Err(x) => error!("libc::mman::munmap(): Couldn't retrieve mapping: {:?}", x),
    };
}
