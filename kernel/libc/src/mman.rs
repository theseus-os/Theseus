
use alloc::alloc::{Layout, alloc, dealloc};
use kernel_config::memory::PAGE_SIZE;
use hashbrown::HashMap;
use memory::{MappedPages, VirtualAddress, FRAME_ALLOCATOR};
use spin::Mutex;
use libm::ceil;
use core::ops::DerefMut;

use crate::types::*;

pub const PROT_NONE: c_int = 0x0000;
pub const PROT_EXEC: c_int = 0x0001;
pub const PROT_WRITE: c_int = 0x0002;
pub const PROT_READ: c_int = 0x0004;


pub const MAP_SHARED: c_int = 0x0001;
pub const MAP_PRIVATE: c_int = 0x0002;
pub const MAP_TYPE: c_int = 0x000F;
pub const MAP_FIXED: c_int = 0x0010;
pub const MAP_ANON: c_int = 0x0020;
pub const MAP_ANONYMOUS: c_int = MAP_ANON;

pub const NULL: *mut c_void = 0 as *mut c_void;

lazy_static! {
    pub static ref MALLOC_LAYOUTS: Mutex<HashMap<VirtualAddress, Layout>> = Mutex::new(HashMap::new());
    pub static ref MMAP_MAPPINGS: Mutex<HashMap<VirtualAddress, MappedPages>> = Mutex::new(HashMap::new());
}

/// Returns a pointer to a portion of heap memory of "size" bytes.
#[no_mangle]
pub unsafe extern "C" fn malloc (size: size_t) -> *mut c_void{
    // TODO: we set the alignment to 1 byte, need to check malloc's alignment requirements
    const alignment: usize = 1;
    let layout =  match Layout::from_size_align(size, alignment) {
        Ok(x) => x,
        Err(x) => return NULL,
    };

    let mem = unsafe{ alloc(layout) };

    match VirtualAddress::new(mem as usize) {
        Ok(x) => MALLOC_LAYOUTS.lock().insert(x, layout),
        Err(x) => return NULL,
    };    

    return mem as *mut c_void;
}

/// Deallocates the memory pointed to by "ptr"
#[no_mangle]
pub unsafe extern "C" fn free(ptr: *mut c_void) {
    let layout = match VirtualAddress::new(ptr as usize) {
        Ok(x) =>  MALLOC_LAYOUTS.lock().remove(&x),
        Err(x) => {
            error!("libc::mman::free() could not convert ptr to virtual address: {:?}",x);    
            return;
        }
    };
    match layout {
        Some(x) => dealloc(ptr as *mut u8, x),
        None => {
            error!("libc::mman::free() layout was not found");                
            return;
        }
    }; 
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
/// Right now this function only implements anonymous mappings without a given address, and ignores all protection flags.
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
        // zero the memory?
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

/// Unmaps a mapping starting at "addr" of "len" bytes
#[no_mangle]
pub extern fn munmap(addr: *mut c_void, len: size_t) {
    match VirtualAddress::new(addr as usize) {
        Ok(x) => {MMAP_MAPPINGS.lock().remove(&x);},
        Err(x) => error!("libc::mman::munmap(): Couldn't retrieve mapping: {:?}", x),
    };
}
