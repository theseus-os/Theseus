use libc::{c_void, c_int, size_t, off_t};
use libc::{MAP_FAILED, PROT_READ, PROT_WRITE};
use memory::{MappedPages, EntryFlags};
use errno::*;

use alloc::{
    vec::Vec,
};
use spin::Mutex;



/// The set of active mappings created by users of this crate.
/// This is not unified with all other mappings in Theseus,
/// in order to keep those invisible and safe from outside accessors.
static MAPPINGS: Mutex<Vec<MappedPages>> = Mutex::new(Vec::new());


#[no_mangle]
pub unsafe extern "C" fn mlock(addr: *const c_void, len: size_t) -> c_int {
    // Theseus doesn't currently swap pages to disk, so pages are always locked.
    0
}

#[no_mangle]
pub unsafe extern "C" fn munlock(addr: *const c_void, len: size_t) -> c_int {
    // Theseus doesn't currently swap pages to disk, so pages are always locked.
    0
}

#[no_mangle]
pub unsafe extern "C" fn mlockall(flags: c_int) -> c_int {
    // Theseus doesn't currently swap pages to disk, so pages are always locked.
    0
}

#[no_mangle]
pub unsafe extern "C" fn munlockall() -> c_int {
    // Theseus doesn't currently swap pages to disk, so pages are always locked.
    0
}

#[no_mangle]
pub unsafe extern "C" fn mmap(
    addr: *mut c_void,
    len: size_t,
    prot: c_int,
    flags: c_int,
    fd: c_int,
    offset: off_t,
) -> *mut c_void {

    fn inner(
        addr: *mut c_void,
        len: size_t,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        offset: off_t,
    ) -> Result<*mut c_void, &'static str> {

        debug!("mmap::inner(): addr: {:X?}, len: {:X?}, prot: {:X?}, flags: {:X?}, fd: {:X?}, offset: {:X?}",
            addr, len, prot, flags, fd, offset
        );

        let pages = if !addr.is_null() {
            let vaddr = memory::VirtualAddress::new(addr as usize)
                .ok_or("addr was an invalid virtual address")?;
            memory::allocate_pages_by_bytes_at(vaddr, len)
                .or_else(|_| memory::allocate_pages_by_bytes(len).ok_or("out of virtual memory"))
        } else {
            memory::allocate_pages_by_bytes(len).ok_or("out of virtual memory")
        }?;

        let flags = entry_flags_from_prot(prot);
        let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or("Theseus memory subsystem not yet initialized.")?;
        let mp = kernel_mmi_ref.lock().page_table.map_allocated_pages(pages, flags)?;

        debug!("mmap::inner(): created {:X?}", mp);
        
        let start_addr = mp.start_address().value();
        MAPPINGS.lock().push(mp);
        Ok(start_addr as *mut _)
    }


    match inner(addr, len, prot, flags, fd, offset) {
        Ok(addr) => addr,
        Err(_e) => {
            error!("mmap(): {}", _e);
            MAP_FAILED
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn munmap(addr: *mut c_void, len: size_t) -> c_int {

    fn inner(addr: *mut c_void, _len: size_t) -> Option<c_int> {
        if let Some(mp) = find_mapped_pages(addr as usize).map(|i| MAPPINGS.lock().remove(i)) {
            drop(mp); // unmaps this MappedPages
            Some(0)
        } else {
            None
        }
    }

    match inner(addr, len) {
        Some(res) => res,
        None => {
            error!("munmap() failed, addr: {:#X}, len: {:#X}", addr as usize, len);
            -EINVAL
        }
    }
}



fn entry_flags_from_prot(prot: c_int) -> EntryFlags {
    let mut flags = EntryFlags::empty();

    if prot & PROT_READ != 0 {
        // Theseus currently treats the PRESENT flag as readable.
        flags.insert(EntryFlags::PRESENT);
    }
    if prot & PROT_WRITE != 0 {
        flags.insert(EntryFlags::WRITABLE);
    }
    // Don't set the NO_EXECUTE flag if the protection flags were executable.
    if prot & PROT_READ == 0 {
        flags.insert(EntryFlags::NO_EXECUTE);
    }

    flags
}


/// Returns the index of the MappedPages object in [`MAPPINGS`]
/// that contains the given `base` address, if any.
fn find_mapped_pages(base: usize) -> Option<usize> {
    unsafe {
        for (i, mp) in MAPPINGS.lock().iter().enumerate() {
            if base >= mp.start_address().value() && base < (mp.start_address().value() + mp.size_in_bytes()) {
                return Some(i);
            }
        }
    }
    None
}
  


// pub unsafe fn protect(base: *const (), _size: usize, protection: Protection) -> Result<()> {
//     if let Some(mp) = find_mapped_pages(base).map(|i| unsafe { &mut MAPPINGS[i] }) {
//         let kernel_mmi_ref = memory::get_kernel_mmi_ref().expect("Theseus memory subsystem not yet initialized.");
//         let mut kernel_mmi = kernel_mmi_ref.lock();
//         mp.remap(&mut kernel_mmi.page_table, protection.to_native())
//             .map_err(|_| Error::UnmappedRegion)
//     } else {
//         Err(Error::UnmappedRegion)
//     }
// }
