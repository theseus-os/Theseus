//! This crate is a frame buffer for display on the screen in 2D mode

#![no_std]
#![feature(alloc)]
#![feature(const_fn)]
#![feature(unique)]
#![feature(ptr_internals)]
#![feature(asm)]

extern crate spin;
extern crate acpi;

extern crate volatile;
extern crate serial_port;
extern crate kernel_config;
extern crate memory;
#[macro_use] extern crate log;
extern crate util;
extern crate alloc;
#[macro_use] extern crate vga_buffer;


use core::ptr::Unique;
use spin::{Mutex};
use memory::{FRAME_ALLOCATOR, Frame, PageTable, PhysicalAddress, 
    EntryFlags, allocate_pages_by_bytes, MappedPages, MemoryManagementInfo,
    get_kernel_mmi_ref};
use core::ops::DerefMut;
use kernel_config::memory::KERNEL_OFFSET;
use alloc::boxed::Box;

const PIXEL_BYTES:usize = 4;

static FRAME_BUFFER_PAGES:Mutex<Option<MappedPages>> = Mutex::new(None);

/// Init the frame buffer. Allocate a block of memory and map it to the frame buffer frames.
pub fn init() -> Result<(), &'static str > {
    
    //Get the graphic mode information
    let VESA_DISPLAY_PHYS_START:PhysicalAddress;
    let VESA_DISPLAY_PHYS_SIZE: usize;
    let BUFFER_WIDTH:usize;
    let BUFFER_HEIGHT:usize;
    {
        let graphic_info = acpi::madt::GRAPHIC_INFO.lock();
        if graphic_info.physical_address == 0 {
            return Err("Fail to get graphic mode infomation!");
        }
        VESA_DISPLAY_PHYS_START = graphic_info.physical_address as usize;
        BUFFER_WIDTH = graphic_info.width as usize;
        BUFFER_HEIGHT = graphic_info.height as usize;
        VESA_DISPLAY_PHYS_SIZE= BUFFER_WIDTH * BUFFER_HEIGHT * PIXEL_BYTES;
    };

    // init the font for text printing
    /*let rs = font::init();
    match font::init() {
        Ok(_) => { trace!("frame_buffer text initialized."); },
        Err(err) => { return Err(err); }
    }*/   

    // get a reference to the kernel's memory mapping information
    let kernel_mmi_ref = get_kernel_mmi_ref().expect("KERNEL_MMI was not yet initialized!");
    let mut kernel_mmi_locked = kernel_mmi_ref.lock();

    // destructure the kernel's MMI so we can access its page table
    let MemoryManagementInfo { 
        page_table: ref mut kernel_page_table, 
        .. // don't need to access other stuff in kernel_mmi
    } = *kernel_mmi_locked;
    trace!("Wenqiu::the swapping is done 3");
    match kernel_page_table {
        &mut PageTable::Active(ref mut active_table) => {
            let pages = match allocate_pages_by_bytes(VESA_DISPLAY_PHYS_SIZE) {
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
            let mut mapped_frame_buffer = try!(active_table.map_allocated_pages_to(
                pages, 
                Frame::range_inclusive_addr(VESA_DISPLAY_PHYS_START, VESA_DISPLAY_PHYS_SIZE), 
                vesa_display_flags, 
                allocator.deref_mut())
            );

            FRAME_DRAWER.lock().set_mode_info(BUFFER_WIDTH, BUFFER_HEIGHT, mapped_frame_buffer);

            Ok(())
        }
        _ => { 
            return Err("framebuffer::init() Couldn't get kernel's active_table");
        }
    }
}

// Window manager uses this drawer to draw/print to the screen
pub static FRAME_DRAWER: Mutex<Drawer> = {
    Mutex::new(Drawer {
        width:0,
        height:0,
        pages:None,
    })
};


// The drawer is responsible for drawing/printing to the screen
pub struct Drawer {
    pub width:usize,
    height:usize,
    pages:Option<MappedPages>,
}

impl Drawer {
    // set the graphic mode information of the buffer
    fn set_mode_info(&mut self, width:usize, height:usize, pages:MappedPages) {
        self.width = width;
        self.height = height;
        self.pages = Some(pages);
    }

    //get the resolution of the screen
    pub fn get_resolution(&self) -> (usize, usize) {
        (self.width, self.height)
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