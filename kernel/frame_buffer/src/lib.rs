//! This crate is a frame buffer for display on the screen in 2D mode

#![no_std]
#![feature(const_fn)]
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


use spin::{Mutex};
use memory::{FRAME_ALLOCATOR, Frame, PageTable, PhysicalAddress, 
    EntryFlags, allocate_pages_by_bytes, MappedPages, MemoryManagementInfo,
    get_kernel_mmi_ref};
use core::ops::DerefMut;
use alloc::boxed::Box;

//The buffer for text printing
pub mod text_buffer;
//The font for text printing
pub mod font;

const PIXEL_BYTES:usize = 4;

#[cfg(framebuffer3d)]
const COLOR_BITS:usize = 24;

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

    // init the font for text printing
    match font::init() {
        Ok(_) => { trace!("frame_buffer text initialized."); },
        Err(err) => { return Err(err); }
    }    

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

            FRAME_DRAWER.lock().set_mode_info(buffer_width, buffer_height, mapped_frame_buffer);

            Ok(())
        }
        _ => { 
            return Err("framebuffer::init() Couldn't get kernel's active_table");
        }
    }
}

//The only instance of the drawer structure
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


///draw a pixel in 3D mode with coordinates and color.
#[cfg(framebuffer3d)]
pub fn draw_pixel_3d(x:usize, y:usize, z:u8, color:u32) {
    FRAME_DRAWER.lock().draw_pixel_3d(x, y, z, color)
}

/// draw a line with start and end coordinates and color
pub fn draw_line(start_x:usize, start_y:usize, end_x:usize, end_y:usize, color:u32) {
    FRAME_DRAWER.lock().draw_line(start_x as i32, start_y as i32, end_x as i32, end_y as i32, color);
}

/// draw a line with start and end coordinates and color
#[cfg(framebuffer3d)]
pub fn draw_line_3d(start_x:usize, start_y:usize, end_x:usize, end_y:usize, z:u8, color:u32) {
    FRAME_DRAWER.lock().draw_line_3d(start_x as i32, start_y as i32, end_x as i32, end_y as i32, 
        z, color);
}

/// draw a rectangle with upper left coordinates, width, height and color
pub fn draw_rectangle(start_x:usize, start_y:usize, width:usize, height:usize, color:u32) {
    FRAME_DRAWER.lock().draw_rectangle(start_x, start_y, width, height, color)
}

/// draw a rectangle with upper left coordinates, width, height and color
#[cfg(framebuffer3d)]
pub fn draw_rectangle_3d(start_x:usize, start_y:usize, width:usize, height:usize, z:u8, color:u32) {
    FRAME_DRAWER.lock().draw_rectangle_3d(start_x, start_y, width, height, z, color)
}

/// fill a rectangle with upper left coordinates, width, height and color
pub fn fill_rectangle(start_x:usize, start_y:usize, width:usize, height:usize, color:u32) {
    FRAME_DRAWER.lock().fill_rectangle(start_x, start_y, width, height, color)
}

/// fill a rectangle with upper left coordinates, width, height and color in 3D mode
#[cfg(framebuffer3d)]
pub fn fill_rectangle_3d(start_x:usize, start_y:usize, width:usize, height:usize, z:u8, color:u32) {
    FRAME_DRAWER.lock().fill_rectangle_3d(start_x, start_y, width, height, z, color)
}

// The drawer is responsible for drawing/printing to the screen
pub struct Drawer {
    pub width:usize,
    height:usize,
    pages:Option<MappedPages>,
}

impl Drawer {
    /*unsafe fn set_background(&mut self, offset:usize, len:usize, color:u32) {
        asm!("cld
            rep stosd"
            :
            : "{rdi}"(self.start_address + offset), "{eax}"(color), "{rcx}"(len)
            : "cc", "memory", "rdi", "rcx"
            : "intel", "volatile");
    }*/

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
        let (buffer_width, buffer_height) = {self.get_resolution()};
        let index = self.get_index_fn();
        let mut buffer;
        match self.buffer() {
            Ok(rs) => {buffer = rs;},
            Err(err) => { debug!("Fail to get frame buffer: {}", err); return; },
        }
        if check_in_range(x as usize,y as usize, buffer_width, buffer_height) {
            draw_in_buffer(index(x, y), color, &mut buffer);
        }
    }

    #[cfg(framebuffer3d)]
    fn draw_pixel_3d(&mut self, x:usize, y:usize, z:u8, color:u32) {
        let (buffer_width, buffer_height) = {self.get_resolution()};
        let index = self.get_index_fn();
        let mut buffer;
        match self.buffer() {
            Ok(rs) => {buffer = rs;},
            Err(err) => { debug!("Fail to get frame buffer"); return; },
        }
        if check_in_range(x as usize,y as usize, buffer_width, buffer_height) {
            draw_in_buffer_3d(index(x, y), z, color, &mut buffer);
        }
    }


    fn draw_line(&mut self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, color:u32){
        let width:i32 = end_x-start_x;
        let height:i32 = end_y-start_y;
        let (buffer_width, buffer_height) = {self.get_resolution()};
        let index = self.get_index_fn();

        let mut buffer;
        match self.buffer() {
            Ok(rs) => {buffer = rs;},
            Err(err) => {
                debug!("Fail to get frame buffer: {}", err); 
                return;
            },
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
                    draw_in_buffer(index(x as usize, y as usize), color, &mut buffer);
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
                    draw_in_buffer(index(x as usize, y as usize), color, &mut buffer);
                }
                y += step;   
            }
        }

    }


    #[cfg(framebuffer3d)]
    pub fn draw_line_3d(&mut self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, z:u8, 
        color:u32) {
        let width:i32 = end_x-start_x;
        let height:i32 = end_y-start_y;
        let mut drawer = FRAME_DRAWER.lock();
        let (buffer_width, buffer_height) = {drawer.get_resolution()};
        let index = drawer.get_index_fn();

        let mut buffer = match drawer.buffer() {
            Ok(rs) => {rs},
            Err(err) => { debug!("Fail to get frame buffer"); return; },
        };

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
                    draw_in_buffer_3d(index(x as usize, y as usize), z, color, buffer);
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
                    draw_in_buffer_3d(index(x as usize, y as usize), z, color, buffer);
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

        let mut buffer;
        match self.buffer() {
            Ok(rs) => {buffer = rs;},
            Err(err) => { debug!("Fail to get frame buffer: {}", err); return;},
        }
  
        let mut x = start_x;
        loop {
            if x == end_x {
                break;
            }
            draw_in_buffer(index(x, start_y), color, &mut buffer);
            draw_in_buffer(index(x, end_y-1), color, &mut buffer);
            // buffer[index(x, start_y)] = color;
            // buffer[index(x, end_y-1)] = color;
            x += 1;
        }

        let mut y = start_y;
        loop {
            if y == end_y {
                break;
            }
            draw_in_buffer(index(start_x, y), color, &mut buffer);
            draw_in_buffer(index(end_x-1, y), color, &mut buffer);
            // buffer[index(start_x, y)] = color;
            // buffer[index(end_x-1, y)] = color;
            y += 1;
        }
    }

    #[cfg(framebuffer3d)]
    pub fn draw_rectangle_3d(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, z:u8,
     color:u32) {
        let mut drawer = FRAME_DRAWER.lock();
        let (buffer_width, buffer_height) = {drawer.get_resolution()};
        let index = drawer.get_index_fn();

        let mut buffer = match drawer.buffer() {
            Ok(rs) => {rs},
            Err(err) => { debug!("Fail to get frame buffer"); return; },
        };

        let end_x:usize = {if start_x + width < buffer_width { start_x + width } 
            else { buffer_width }};
        let end_y:usize = {if start_y + height < buffer_height { start_y + height } 
            else { buffer_height }};  

        let mut x = start_x;
        loop {
            if x == end_x {
                break;
            }
            
            draw_in_buffer_3d(index(x, start_y), z, color, buffer);
            draw_in_buffer_3d(index(x, end_y-1), z, color, buffer);
            x += 1;
        }

        let mut y = start_y;
        loop {
            if y == end_y {
                break;
            }

            draw_in_buffer_3d(index(start_x, y), z, color, buffer);
            draw_in_buffer_3d(index(end_x-1, y), z, color, buffer);
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

        let mut buffer;
        match self.buffer() {
            Ok(rs) => {buffer = rs;},
            Err(err) => { debug!("Fail to get frame buffer: {}", err); return;},
        }
        
        loop {
            if x == end_x {
                y += 1;
                if y == end_y {
                    break;
                }
                x = start_x;
            }

            draw_in_buffer(index(x, y), color, &mut buffer);
            x += 1;
        }
    }

    ///draw a square in 2D compatible mode with upper left coordinates, width, height and color.
    #[cfg(framebuffer3d)]
    pub fn fill_rectangle_3d(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, z:u8,
        color:u32) {
        let mut drawer = FRAME_DRAWER.lock();
        let (buffer_width, buffer_height) = {drawer.get_resolution()};
        let index = drawer.get_index_fn();

        let buffer = match drawer.buffer() {
            Ok(rs) => {rs},
            Err(err) => { debug!("Fail to get frame buffer"); return; },
        };
        
        let end_x:usize = {if start_x + width < buffer_width { start_x + width } 
            else { buffer_width }};
        let end_y:usize = {if start_y + height < buffer_height { start_y + height } 
            else { buffer_height }};  

        let mut x = start_x;
        let mut y = start_y;
        loop {
            if x == end_x {
                y += 1;
                if y == end_y {
                    break;
                }
                x = start_x;
            }

            draw_in_buffer_3d(index(x, y), z, color, buffer);
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

/// Get the resolution of the screen
pub fn get_resolution() -> (usize, usize) {
    FRAME_DRAWER.lock().get_resolution()
}

// Check if a point is in the screen
fn check_in_range(x:usize, y:usize, width:usize, height:usize)  -> bool {
    x < width && y < height
}

#[cfg(not(framebuffer3d))]
fn draw_in_buffer(index:usize, color:u32, buffer:&mut[u32]) {
    buffer[index] = color;
}

#[cfg(framebuffer3d)]
fn draw_in_buffer(index:usize, color:u32, buffer:&mut[u32]) {
    draw_in_buffer_3d(index, 0, color, buffer);
}

#[cfg(framebuffer3d)]
fn draw_in_buffer_3d(index:usize, z:u8, color:u32,  buffer:&mut[u32]) {
    if  (buffer[index] >> COLOR_BITS) <= z as u32 {
        buffer[index] = ((z as u32) << COLOR_BITS) + color;
    }
}