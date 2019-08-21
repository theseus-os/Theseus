//! This crate is a frame buffer for display on the screen in 2D mode

#![no_std]
#![feature(const_fn)]
#![feature(ptr_internals)]
#![feature(asm)]

extern crate spin;
extern crate multicore_bringup;

extern crate volatile;
extern crate serial_port;
extern crate kernel_config;
extern crate memory;
#[macro_use] extern crate log;
extern crate util;
extern crate alloc;


use spin::{Mutex};
use memory::{FRAME_ALLOCATOR, FrameRange, PhysicalAddress, 
    EntryFlags, allocate_pages_by_bytes, MappedPages, get_kernel_mmi_ref};
use core::ops::DerefMut;
use alloc::boxed::Box;

//The buffer for text printing
pub mod text_buffer;
//The font for text printing
pub mod font;

const PIXEL_BYTES:usize = 4;

/// Init the frame buffer. Allocate a block of memory and map it to the frame buffer frames.
pub fn init() -> Result<(), &'static str > {
    
    //Get the graphic mode information
    let VESA_DISPLAY_PHYS_START:PhysicalAddress;
    let VESA_DISPLAY_PHYS_SIZE: usize;
    let BUFFER_WIDTH:usize;
    let BUFFER_HEIGHT:usize;
    {
        let graphic_info = multicore_bringup::GRAPHIC_INFO.lock();
        if graphic_info.physical_address == 0 {
            return Err("Fail to get graphic mode infomation!");
        }
        VESA_DISPLAY_PHYS_START = PhysicalAddress::new(graphic_info.physical_address as usize)?;
        BUFFER_WIDTH = graphic_info.width as usize;
        BUFFER_HEIGHT = graphic_info.height as usize;
        VESA_DISPLAY_PHYS_SIZE= BUFFER_WIDTH * BUFFER_HEIGHT * PIXEL_BYTES;
    };

    // init the font for text printing
    font::init()?;

    // get a reference to the kernel's memory mapping information
    let kernel_mmi_ref = get_kernel_mmi_ref().expect("KERNEL_MMI was not yet initialized!");
    let mut kernel_mmi_locked = kernel_mmi_ref.lock();
    let pages = allocate_pages_by_bytes(VESA_DISPLAY_PHYS_SIZE).ok_or("couldn't allocate framebuffer pages")?;
    let vesa_display_flags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;
    let allocator = FRAME_ALLOCATOR.try().ok_or("Couldn't get frame allocator")?;
    let mapped_frame_buffer = kernel_mmi_locked.page_table.map_allocated_pages_to(
        pages, 
        FrameRange::from_phys_addr(VESA_DISPLAY_PHYS_START, VESA_DISPLAY_PHYS_SIZE), 
        vesa_display_flags, 
        allocator.lock().deref_mut()
    )?;

    FRAME_DRAWER.lock().set_mode_info(BUFFER_WIDTH, BUFFER_HEIGHT, mapped_frame_buffer);
    Ok(())
}

// Window manager uses this drawer to draw/print to the screen
static FRAME_DRAWER: Mutex<Drawer> = {
    Mutex::new(Drawer {
        width:0,
        height:0,
        pages:None,
    })
};

/// draw a pixel with coordinates and color
pub fn draw_pixel(x:usize, y:usize, color:u32) {
    FRAME_DRAWER.lock().draw_pixel(x, y, color);
}

/// draw a line with start and end coordinates and color
pub fn draw_line(start_x:usize, start_y:usize, end_x:usize, end_y:usize, color:u32) {
    FRAME_DRAWER.lock().draw_line(start_x as i32, start_y as i32, end_x as i32, end_y as i32, color);
}

/// draw a rectangle with upper left coordinates, width, height and color
pub fn draw_rectangle(start_x:usize, start_y:usize, width:usize, height:usize, color:u32) {
    FRAME_DRAWER.lock().draw_rectangle(start_x, start_y, width, height, color)
}

/// fill a rectangle with upper left coordinates, width, height and color
pub fn fill_rectangle(start_x:usize, start_y:usize, width:usize, height:usize, color:u32) {
    FRAME_DRAWER.lock().fill_rectangle(start_x, start_y, width, height, color)
}

/*pub struct Point {
    pub x: usize,
    pub y: usize,
    pub color: usize,
}*/

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
    fn get_resolution(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn draw_pixel(&mut self, x:usize, y:usize, color:u32) {
        let index = self.get_index_fn();
        let buffer;
        match self.buffer() {
            Ok(rs) => {buffer = rs;},
            Err(err) => { error!("Failed to get frame buffer: {}", err); return; },
        }
        buffer[index(x, y)] = color;
    }

    fn draw_line(&mut self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, color:u32){
        let width:i32 = end_x-start_x;
        let height:i32 = end_y-start_y;
        let (buffer_width, buffer_height) = {self.get_resolution()};
        let index = self.get_index_fn();

        let buffer;
        match self.buffer() {
            Ok(rs) => {buffer = rs;},
            Err(err) => {
                error!("Failed to get frame buffer: {}", err); return;},
        }

        if width.abs() > height.abs() {
            let mut y;
            let mut x = start_x;
            let step = if width > 0 {1} else {-1};

            loop {
                if x == end_x {
                    break;
                }          
                y = (x - start_x) * height / width + start_y;
                if check_in_range(x as usize,y as usize, buffer_width, buffer_height) {
                    buffer[index(x as usize, y as usize)] = color;
                }
                x += step;
            }
        } else {
            let mut x;
            let mut y = start_y;
            let step = if height > 0 {1} else {-1};
            loop {
                if y == end_y {
                    break;
                }
                x = (y - start_y) * width / height + start_x;
                if check_in_range(x as usize,y as usize, buffer_width, buffer_height) {
                    buffer[index(x as usize, y as usize)] = color;
                }
                y += step;   
            }
        }

    }

    fn draw_rectangle(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
        let end_x:usize = {if start_x + width < self.width { start_x + width } 
            else { self.width }};
        let end_y:usize = {if start_y + height < self.height { start_y + height } 
            else { self.height }};  
        let index = self.get_index_fn();

        let buffer;
        match self.buffer() {
            Ok(rs) => {buffer = rs;},
            Err(err) => { error!("Fail to get frame buffer: {}", err); return;},
        }
  
        let mut x = start_x;
        loop {
            if x == end_x {
                break;
            }
            buffer[index(x, start_y)] = color;
            buffer[index(x, end_y-1)] = color;
            x += 1;
        }

        let mut y = start_y;
        loop {
            if y == end_y {
                break;
            }
            buffer[index(start_x, y)] = color;
            buffer[index(end_x-1, y)] = color;
            y += 1;
        }
    }

    fn fill_rectangle(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
        let end_x:usize = {if start_x + width < self.width { start_x + width } 
            else { self.width }};
        let end_y:usize = {if start_y + height < self.height { start_y + height } 
            else { self.height }};  

        let mut x = start_x;
        let mut y = start_y;

        let index = self.get_index_fn();

        let buffer;
        match self.buffer() {
            Ok(rs) => {buffer = rs;},
            Err(err) => { error!("Fail to get frame buffer: {}", err); return;},
        }
        
        loop {
            if x == end_x {
                y += 1;
                if y == end_y {
                    break;
                }
                x = start_x;
            }

            buffer[index(x, y)] = color;
            x += 1;
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

// Get the resolution of the screen
pub fn get_resolution() -> (usize, usize) {
    FRAME_DRAWER.lock().get_resolution()
}

// Check if a point is in the screen
fn check_in_range(x:usize, y:usize, width:usize, height:usize)  -> bool {
    x < width && y < height
}