
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

lazy_static! {
    pub static ref MALLOC_LAYOUTS: Mutex<HashMap<VirtualAddress, Layout>> = Mutex::new(HashMap::new());
    pub static ref MMAP_MAPPINGS: Mutex<HashMap<VirtualAddress, MappedPages>> = Mutex::new(HashMap::new());
}

/// Creates a layout for the memory required and returns a pointer from the heap 
/// void *malloc(size_t size);
#[no_mangle]
pub unsafe extern "C" fn malloc (size: size_t) -> *mut c_void{
    debug!("In malloc");
    // we set the alignment to 1 byte, need to check malloc's alignment requirements
    const alignment: usize = 1;
    let layout = Layout::from_size_align(size, alignment).unwrap();
    let mem = unsafe{ alloc(layout) };

    // remove below,  just for testing
    unsafe {
        *mem = 9;
        *mem.offset(1) = 15;
    }

    let addr = VirtualAddress::new(mem as usize).unwrap();
    MALLOC_LAYOUTS.lock().insert(addr, layout);

    return mem as *mut c_void;
}

#[no_mangle]
pub unsafe extern "C" fn free(ptr: *mut c_void) {
    let addr = VirtualAddress::new(ptr as usize).unwrap();
    let layout = MALLOC_LAYOUTS.lock().remove(&addr).unwrap();
    dealloc(ptr as *mut u8, layout); 
}

/// void *mmap(void *addr, size_t length, int prot, int flags, int fd, off_t offset);
/// Creates a new mapping in the virtual address space.
/// 
/// # Arguments
/// * `addr`: Starting address of the new mapping. If it's NULL then the kernel decides the address of the new mapping.
/// * `length`: length of the mapping which must be greater than zero.
/// * `prot`: describes the desired memory protection of the mapping.
/// * `flags`: determines whether updates to the mapping are visible to other processes mapping the same region,
/// and whether updates are carried through to the underlying file
/// * `fd`: file descriptor,
/// * `offset`: offset in the file to write to. 
/// 
/// Right now we only consider the case of anonymous mappings without a given address.
#[no_mangle]
pub extern fn mmap(_addr: *mut c_void, length: size_t, prot: c_int, flags: c_int, fd: c_int, offset: off_t) -> *mut c_void {
    if flags & MAP_ANON == MAP_ANON {
        // allocate the number of pages
        let kernel_mmi_ref = memory::get_kernel_mmi_ref().unwrap();
        let allocator = FRAME_ALLOCATOR.try().unwrap();
        let pages = memory::allocate_pages_by_bytes(length as usize).unwrap();
        let mapped_pages = kernel_mmi_ref.lock().page_table.map_allocated_pages(pages, Default::default() , allocator.lock().deref_mut()).unwrap();

        // zero the memory?

        let addr = mapped_pages.start_address();
        MMAP_MAPPINGS.lock().insert(addr, mapped_pages);

        return addr.value() as *mut c_void;
    }

    // else if flags & MAP_SHARED == MAP_SHARED {

    // }

    else {
        error!("Unimplemented memory mapping!");
        return 0 as *mut c_void;
    }

}

#[no_mangle]
pub extern fn munmap(addr: *mut c_void, len: size_t) {
    let addr = VirtualAddress::new(addr as usize).unwrap();
    let mapped_pages = MMAP_MAPPINGS.lock().remove(&addr).unwrap();
    // mapped_pages.drop();
}
