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


use spin::{Mutex};
use memory::{FRAME_ALLOCATOR, Frame, PageTable, PhysicalAddress, 
    EntryFlags, allocate_pages_by_bytes, MappedPages, MemoryManagementInfo,
    get_kernel_mmi_ref};
use core::ops::DerefMut;
use alloc::boxed::Box;


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

            FRAME_BUFFER.lock().set_mode_info(buffer_width, buffer_height, mapped_frame_buffer);

            Ok(())
        }
        _ => { 
            return Err("framebuffer::init() Couldn't get kernel's active_table");
        }
    }
}

//The only instance of the drawer structure
pub static FRAME_BUFFER: Mutex<Buffer> = {
    Mutex::new(Buffer {
        width:0,
        height:0,
        pages:None,
    })
};

pub struct Buffer {
    width:usize,
    height:usize,
    pages:Option<MappedPages>
}

impl Buffer {
        // set the graphic mode information of the buffer
    fn set_mode_info(&mut self, width:usize, height:usize, pages:MappedPages) {
        self.width = width;
        self.height = height;
        self.pages = Some(pages);
    }

    /// Get the resolution of the screen
    pub fn get_resolution(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    // Check if a point is in the screen
    pub fn check_in_range(&self, x:usize, y:usize)  -> bool {
        x < self.width && y < self.height
    }

    fn draw(&mut self, index:usize, color:u32) {
        match self.pages {
            Some(ref mut pages) => {
                let buffer = match (pages.as_slice_mut(0, self.width*self.height)) {
                    Ok( bf) => { bf },
                    Err(_) => { error!("Fail to transmute buffer"); return; }
                };
                buffer[index] = color;
            },
            None => { error!("Fail to get buffer") }
        }
    }

    // return the framebuffer
    pub fn buffer(&mut self) -> Result<&mut[u32], &'static str> {
        match self.pages {
            Some(ref mut pages) => {
                let buffer = try!(pages.as_slice_mut(0, self.width*self.height));
                return Ok(buffer);
            },
            None => { return Err("no allocated pages in framebuffer") }
        }
    }

    // get the index computation function according to the width of the buffer
    // call it to get the index function before locking the buffer
    pub fn get_index_fn(&self) -> Box<Fn(usize, usize)->usize>{
        let width = self.width;
        Box::new(move |x:usize, y:usize| y * width + x )
    }

}


pub fn draw_in_buffer(index:usize, color:u32,  buffer:&mut[u32]) {
    buffer[index] = color;
}

// Check if a point is in the screen
pub fn check_in_range(x:usize, y:usize, width:usize, height:usize)  -> bool {
    x < width && y < height
}