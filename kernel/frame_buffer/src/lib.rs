//! This crate is a frame buffer for display on the screen in 2D mode

#![no_std]
#![feature(const_fn)]
#![feature(ptr_internals)]
#![feature(asm)]

extern crate spin;
extern crate acpi;

extern crate volatile;
extern crate serial_port;
extern crate memory;
#[macro_use] extern crate log;
extern crate util;
extern crate alloc;
extern crate owning_ref;

use owning_ref::BoxRefMut;
use spin::{Mutex, Once};
use memory::{FRAME_ALLOCATOR, Frame, PageTable, PhysicalAddress, 
    EntryFlags, allocate_pages_by_bytes, MappedPages, MemoryManagementInfo,
    get_kernel_mmi_ref};
use core::ops::DerefMut;
use alloc::boxed::Box;
use alloc::sync::Arc;


static FRAME_BUFFER: Once<Buffer> = Once::new();


const PIXEL_BYTES:usize = 4;

// #[cfg(framebuffer3d)]
// const COLOR_BITS:usize = 24;

/// Init the frame buffer. Allocate a block of memory and map it to the frame buffer frames.
pub fn init() -> Result<(), &'static str > {
    
    //Get the graphic mode information
    let vesa_display_phys_start:PhysicalAddress;
    let vesa_display_phys_size: usize;
    let buffer_width:usize;
    let buffer_height:usize;
    {
        let graphic_info = acpi::madt::GRAPHIC_INFO.lock();
        if graphic_info.physical_address == 0 {
            return Err("Fail to get graphic mode infomation!");
        }
        vesa_display_phys_start = PhysicalAddress::new(graphic_info.physical_address as usize)?;
        buffer_width = graphic_info.width as usize;
        buffer_height = graphic_info.height as usize;
        vesa_display_phys_size= buffer_width * buffer_height * PIXEL_BYTES;
    };

    // get a reference to the kernel's memory mapping information
    let kernel_mmi_ref = get_kernel_mmi_ref().expect("KERNEL_MMI was not yet initialized!");
    let mut kernel_mmi_locked = kernel_mmi_ref.lock();

    // destructure the kernel's MMI so we can access its page table
    let MemoryManagementInfo { 
        page_table: ref mut kernel_page_table, 
        .. // don't need to access other stuff in kernel_mmi
    } = *kernel_mmi_locked;
    
    match kernel_page_table {
        &mut PageTable::Active(ref mut active_table) => {
            let pages = match allocate_pages_by_bytes(vesa_display_phys_size) {
                Some(pages) => { pages },
                None => { return Err("frame_buffer::init() couldn't allocate pages."); }
            };
            
            let vesa_display_flags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;
            let allocator_mutex = FRAME_ALLOCATOR.try();
            match allocator_mutex {
                Some(_) => { },
                None => { return Err("framebuffer::init() couldn't get frame allocator"); }
            }

            let mut allocator = try!(allocator_mutex.ok_or("allocate frame buffer")).lock();
            let mapped_frame_buffer = try!(active_table.map_allocated_pages_to(
                pages, 
                Frame::range_inclusive_addr(vesa_display_phys_start, vesa_display_phys_size), 
                vesa_display_flags, 
                allocator.deref_mut())
            );

            // let mut hpet = BoxRefMut::new(Box::new(hpet_page))
            //     .try_map_mut(|mp| mp.as_type_mut::<Hpet>(address_page_offset(phys_addr.value())))?;

            let mut buffer = BoxRefMut::new(Box::new(mapped_frame_buffer)).
                try_map_mut(|mp| mp.as_slice_mut(0, buffer_width * buffer_height))?;
            let buffer_ref = Arc::new(Mutex::new(buffer));
            FRAME_BUFFER.call_once(|| 
                Buffer {
                    width:buffer_width,
                    height:buffer_height,
                    buffer_ref: buffer_ref
                }
            );

            Ok(())
        }
        _ => { 
            return Err("framebuffer::init() Couldn't get kernel's active_table");
        }
    }
}

pub struct Buffer {
    width:usize,
    height:usize,
    buffer_ref:Arc<Mutex<BoxRefMut<MappedPages, [u32]>>>,
}

pub struct PhysicalFrameBuffer{
    width:usize, 
    height:usize,
    pub buffer_ref:Arc<Mutex<BoxRefMut<MappedPages, [u32]>>>,
}

impl PhysicalFrameBuffer {
        // set the graphic mode information of the buffer
    // fn set_mode_info(&mut self, width:usize, height:usize, pages:MappedPages) {
    //     self.width = width;
    //     self.height = height;
    //     self.pages = Some(pages);
    // }

    /// Get the resolution of the screen
    pub fn get_resolution(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    // Check if a point is in the screen
    pub fn check_in_range(&self, x:usize, y:usize)  -> bool {
        x < self.width && y < self.height
    }

    // fn draw(&mut self, index:usize, color:u32) -> Result<(), &'static str>{
        
    //     // match self.pages {
    //     //     Some(ref mut pages) => {
    //     //         let buffer = match (pages.as_slice_mut(0, self.width*self.height)) {
    //     //             Ok( bf) => { bf },
    //     //             Err(_) => { error!("Fail to transmute buffer"); return; }
    //     //         };
    //     //         buffer[index] = color;
    //     //     },
    //     //     None => { error!("Fail to get buffer") }
    //     // }
    //     let mut fb = try!(FRAME_BUFFER.try().ok_or("Fail to get access to the framebuffer")).lock();
    //     fb.buffer_ref[index] = color;
    //     Ok(())
    // }

    // return the framebuffer
    // pub fn buffer(&mut self) -> Result<&mut[u32], &'static str> {
    //     match self.pages {
    //         Some(ref mut pages) => {
    //             let buffer = try!(pages.as_slice_mut(0, self.width*self.height));
    //             return Ok(buffer);
    //         },
    //         None => { return Err("no allocated pages in framebuffer") }
    //     }
    // }

    // get the index computation function according to the width of the buffer
    // call it to get the index function before locking the buffer
    pub fn get_index_fn(&self) -> Box<Fn(usize, usize)->usize>{
        let width = self.width;
        Box::new(move |x:usize, y:usize| y * width + x )
    }

}

pub fn get_buffer_ref() ->  Result<PhysicalFrameBuffer, &'static str> {
    let mut fb = try!(FRAME_BUFFER.try().ok_or("Fail to get access to the framebuffer"));
    let (width, height) = (fb.width, fb.height);
//    let buffer_ref = fb.buffer.deref_mut();
    Ok(PhysicalFrameBuffer{
        width:width,
        height:height,
        buffer_ref:Arc::clone(&fb.buffer_ref),
    })
}


pub fn draw_in_buffer(index:usize, color:u32,  buffer:&mut[u32]) {
    buffer[index] = color;
}

pub fn get_resolution() -> Result<(usize, usize), &'static str> {
    let framebuffer = try!(FRAME_BUFFER.try().ok_or("Fail to get the physical frame buffer"));
    Ok((framebuffer.width, framebuffer.height))
}


// Check if a point is in the screen
pub fn check_in_range(x:usize, y:usize, width:usize, height:usize)  -> bool {
    x < width && y < height
}