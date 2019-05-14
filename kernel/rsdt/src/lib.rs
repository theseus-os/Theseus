//! Definitions for the ACPI RSDT and XSDT system tables.
//!
//! RSDT is the Root System Descriptor Table, whereas
//! XSDT is the Extended System Descriptor Table. 
//! They are identical except that the XSDT uses 64-bit physical addresses
//! to point to other ACPI SDTs, while the RSDT uses 32-bit physical addresses.

#![no_std]

extern crate alloc;
extern crate memory;
extern crate sdt;
extern crate owning_ref;

use core::mem::size_of;
use core::ops::DerefMut;
use alloc::boxed::Box;
use memory::{MappedPages, allocate_pages, EntryFlags, Frame, PageTable, FRAME_ALLOCATOR, PhysicalAddress, PhysicalMemoryArea};
use sdt::{Sdt, SDT_SIZE_IN_BYTES};
use owning_ref::BoxRef;


const RSDT_SIGNATURE: &'static [u8; 4] = b"RSDT";
const XSDT_SIGNATURE: &'static [u8; 4] = b"XSDT";


/// The Root/Extended System Descriptor Table,
/// which primarily contains an array of physical addresses
/// (32-bit if the regular RSDT, 64-bit if the extended XSDT)
/// where other ACPI SDTs can be found.
pub struct RsdtXsdt {
    which: RsdtOrXsdt,
    /// The offset into the MappedPages where the SDT header is.
    sdt_offset: usize,
}

impl RsdtXsdt {
    /// Creates a new `Rsdt` or `Xsdt` object based on the given MappedPages object
    /// that contains its `Sdt`. 
    pub fn create_and_map(rxsdt_phys_addr: PhysicalAddress, page_table: &mut PageTable) -> Result<RsdtXsdt, &'static str> {
        // First, we map the SDT header so we can obtain the `signature` field to determine if it's an RSDT or XSDT, 
        // and the `length` field to determine how many SDT entries there are in this RSDT/XSDT.
        let first_page = allocate_pages(1).ok_or("couldn't allocate_pages")?;
        let first_frame = Frame::containing_address(rxsdt_phys_addr);
        let allocator = FRAME_ALLOCATOR.try().ok_or("couldn't get Frame Allocator")?;
        let mut mapped_pages = page_table.map_allocated_pages_to(
            first_page, 
            Frame::range_inclusive(first_frame.clone(), first_frame.clone()),
            EntryFlags::PRESENT | EntryFlags::NO_EXECUTE,
            allocator.lock().deref_mut(),
        )?;
        
        // the offset into the mapped_pages where the SDT header is
        let sdt_offset = rxsdt_phys_addr.frame_offset();
        // the offset into the mapped_pages where the array of physical addresses is
        let addrs_offset = sdt_offset + size_of::<Sdt>();
        let (is_rsdt, total_length) = {
            let sdt: &Sdt = mapped_pages.as_type(sdt_offset)?;
            let is_rsdt = match &sdt.signature {
                RSDT_SIGNATURE => true,
                XSDT_SIGNATURE => false,
                _ => return Err("couldn't find RSDT or XSDT signature"),
            };
            (is_rsdt, sdt.length as usize)
        };

        // Here, if the initial mapped_pages is insufficient to cover the full Rsdt/Xsdt length,
        // then we need to create a new mapping to cover it and the length of all of its entries.
        let last_frame = Frame::containing_address(rxsdt_phys_addr + total_length);
        if last_frame > first_frame {
            let frames = Frame::range_inclusive(first_frame, last_frame);
            let num_pages = frames.size_in_frames();
            let pages = allocate_pages(num_pages).ok_or("couldn't allocate_pages")?;
            mapped_pages = page_table.map_allocated_pages_to(
                pages,
                frames,
                EntryFlags::PRESENT | EntryFlags::NO_EXECUTE,
                allocator.lock().deref_mut(),
            )?;
        }

        // Inform the frame allocator that the physical frame(s) where the RSDT/XSDT exists are now in use.
        {
            let rxsdt_area = PhysicalMemoryArea::new(rxsdt_phys_addr, total_length, 1, 3);
            allocator.lock().add_area(rxsdt_area, false)?;
        }
        
        // Now we can convert the mapped_pages into either an RSDT or XSDT
        if is_rsdt {
            let num_addrs = (total_length - SDT_SIZE_IN_BYTES) / size_of::<u32>();
            Ok( RsdtXsdt {
                which: RsdtOrXsdt::Regular(
                    BoxRef::new(Box::new(mapped_pages)).try_map(|mp| mp.as_slice::<u32>(addrs_offset, num_addrs))?
                ),
                sdt_offset: sdt_offset,
            })
        } else {
            let num_addrs = (total_length - SDT_SIZE_IN_BYTES) / size_of::<u64>();
            Ok( RsdtXsdt {
                which: RsdtOrXsdt::Extended(
                    BoxRef::new(Box::new(mapped_pages)).try_map(|mp| mp.as_slice::<u64>(addrs_offset, num_addrs))?
                ),
                sdt_offset: sdt_offset,
            })
        }
    }

    /// Returns the number of SDTs that are included in the RSDT or XSDT.
    pub fn num_sdts(&self) -> usize {
        match self.which {
            RsdtOrXsdt::Regular(ref r)  => r.len(),
            RsdtOrXsdt::Extended(ref x) => x.len(),
        }
    }

    /// Returns an iterator over the `PhysicalAddress`es of the SDT entries
    /// included in the RSDT or XSDT.
    pub fn sdt_addresses<'r>(&'r self) -> impl Iterator<Item = PhysicalAddress> + 'r {
        // Ideally, we would do something like this, but Rust doesn't allow match arms to have different types (iterator map closures are types...)
        // match &self.which {
        //     RsdtOrXsdt::Regular(ref r)  => r.iter().map(|paddr| PhysicalAddress::new_canonical(*paddr as usize)),
        //     RsdtOrXsdt::Extended(ref x) => x.iter().map(|paddr| PhysicalAddress::new_canonical(*paddr as usize)),
        // }
        //
        // So, instead, we use a little trick inspired by this post:
        // https://stackoverflow.com/questions/29760668/conditionally-iterate-over-one-of-several-possible-iterators

        let r_iter = if let RsdtOrXsdt::Regular(ref r) = self.which {
            Some(r.iter().map(|paddr| PhysicalAddress::new_canonical(*paddr as usize)))
        } else {
            None
        };
        let x_iter = if let RsdtOrXsdt::Extended(ref x) = self.which {
            Some(x.iter().map(|paddr| PhysicalAddress::new_canonical(*paddr as usize)))
        } else {
            None
        };
        r_iter.into_iter().flatten().chain(x_iter.into_iter().flatten())
    }

    /// Returns a reference to the SDT header of this RSDT or XSDT.
    pub fn sdt(&self) -> Result<&Sdt, &'static str> {
        match &self.which {
            RsdtOrXsdt::Regular(ref r)  => r.as_owner().as_type(self.sdt_offset),
            RsdtOrXsdt::Extended(ref x) => x.as_owner().as_type(self.sdt_offset),
        }
    }
}


/// Either an RSDT or an XSDT. 
/// The RSDT specifies that there are a variable number of 
/// 32-bit physical addresses following the SDT header,
/// while the XSDT is the same but with 64-bit physical addresses.
enum RsdtOrXsdt {
    /// RSDT
    Regular( BoxRef<MappedPages, [u32]>),
    // Regular( BoxRef<MappedPages, (Sdt, [u32])>),
    /// XSDT
    Extended(BoxRef<MappedPages, [u64]>),
    // Extended(BoxRef<MappedPages, (Sdt, [u64])>),
}
