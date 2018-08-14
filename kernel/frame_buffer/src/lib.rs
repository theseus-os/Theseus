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


use core::ptr::Unique;
use spin::{Mutex};
use memory::{FRAME_ALLOCATOR, Frame, PageTable, PhysicalAddress, 
    EntryFlags, allocate_pages_by_bytes, MappedPages, MemoryManagementInfo,
    get_kernel_mmi_ref};
use core::ops::DerefMut;
use kernel_config::memory::KERNEL_OFFSET;
use alloc::boxed::Box;

pub mod text_buffer;
pub mod font;

//const VGA_BUFFER_ADDR: usize = 0xa0000;
const VGA_BUFFER_ADDR: usize = 0xb8000 + KERNEL_OFFSET;

//Size of VESA mode 0x4112

///The width of the screen
//pub const FRAME_BUFFER_WIDTH:usize = 640;

///The height of the screen
//pub const FRAME_BUFFER_HEIGHT:usize = 400;

const PIXEL_BYTES:usize = 4;

static FRAME_BUFFER_PAGES:Mutex<Option<MappedPages>> = Mutex::new(None);

static GRAPHIC_MODE_PAGE:Mutex<Option<MappedPages>> = Mutex::new(None);



/// Init the frame buffer. Allocate a block of memory and map it to the frame buffer frames.
pub fn init() -> Result<(), &'static str > {
    
    let VESA_DISPLAY_PHYS_START:PhysicalAddress;
    let VESA_DISPLAY_PHYS_SIZE: usize;
    let (width, height) = {
        let graphic_info = acpi::madt::GRAPHIC_INFO.lock();
        if graphic_info.address == 0 {
            return Err("Fail to get graphic mode infomation!");
        }
        VESA_DISPLAY_PHYS_START = graphic_info.address as usize;
        let mut drawer = FRAME_DRAWER.lock();
        //drawer.set_resolution(graphic_info.x as usize, graphic_info.y as usize);
        VESA_DISPLAY_PHYS_SIZE= (graphic_info.x*graphic_info.y) as usize * PIXEL_BYTES;
        (graphic_info.x as usize, graphic_info.y as usize)
    };

    let rs = font::init();
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
            let pages = match allocate_pages_by_bytes(VESA_DISPLAY_PHYS_SIZE) {
                Some(pages) => { pages },
                None => { return Err("frame_buffer_3d::init() couldn't allocate pages."); }
            };
            
            let vesa_display_flags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;
            let allocator_mutex = FRAME_ALLOCATOR.try();
            match allocator_mutex {
                Some(_) => { },
                None => { return Err("framebuffer::init() Couldn't get frame allocator"); }
            }

            let mut allocator = try!(allocator_mutex.ok_or("allocate frame buffer")).lock();
            let mapped_frame_buffer = try!(active_table.map_allocated_pages_to(
                pages, 
                Frame::range_inclusive_addr(VESA_DISPLAY_PHYS_START, VESA_DISPLAY_PHYS_SIZE), 
                vesa_display_flags, 
                allocator.deref_mut())
            );

            let rs = FRAME_DRAWER.lock().set_mode_info(width, height, mapped_frame_buffer);

            Ok(())
        }
        _ => { 
            return Err("framebuffer::init() Couldn't get kernel's active_table");
        }
    }
}

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
    FRAME_DRAWER.lock().draw_line(start_x as i32, start_y as i32, end_x as i32, end_y as i32, color)
}

/// draw a rectangle with upper left coordinates, width, height and color
pub fn draw_rectangle(start_x:usize, start_y:usize, width:usize, height:usize, color:u32) {
    FRAME_DRAWER.lock().draw_rectangle(start_x, start_y, width, height, color)
}

/// fill a rectangle with upper left coordinates, width, height and color
pub fn fill_rectangle(start_x:usize, start_y:usize, width:usize, height:usize, color:u32) {
    FRAME_DRAWER.lock().fill_rectangle(start_x, start_y, width, height, color)
}

pub struct Point {
    pub x: usize,
    pub y: usize,
    pub color: usize,
}

pub struct Drawer {
    width:usize,
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
    fn set_mode_info(&mut self, width:usize, height:usize, pages:MappedPages) {
        self.width = width;
        self.height = height;
        self.pages = Some(pages);
    }

    pub fn get_resolution(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn draw_pixel(&mut self, x:usize, y:usize, color:u32) {
        let index = get_index_fn(self.width);
        let buffer;
        match self.buffer() {
            Ok(rs) => {buffer = rs;},
            Err(err) => { debug!("Fail to get frame buffer"); return; },
        }
        buffer[index(x, y)] = color;
    }

    fn draw_line(&mut self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, color:u32){
        let width:i32 = end_x-start_x;
        let height:i32 = end_y-start_y;
        let (buffer_width, buffer_height) = {self.get_resolution()};
        let index = get_index_fn(buffer_width);

        let buffer;
        match self.buffer() {
            Ok(rs) => {buffer = rs;},
            Err(err) => { debug!("Fail to get frame buffer"); return;},
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
        }
        else {
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
        let index = get_index_fn(self.width);

        let buffer;
        match self.buffer() {
            Ok(rs) => {buffer = rs;},
            Err(err) => { debug!("Fail to get frame buffer"); return;},
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
            if y == end_x {
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

        let index = get_index_fn(self.width);

        let buffer;
        match self.buffer() {
            Ok(rs) => {buffer = rs;},
            Err(err) => { debug!("Fail to get frame buffer"); return;},
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

    fn buffer(&mut self) -> Result<&mut[u32], &'static str> {
        match self.pages {
            Some(ref mut pages) => {
                let buffer = try!(pages.as_slice_mut(0, 640*400));
                return Ok(buffer);
            },
            None => { return Err("no allocated pages in framebuffer") }
        }
    }
}

pub struct Buffer {
    //chars: [Volatile<[u8; FRAME_BUFFER_WIDTH]>;FRAME_BUFFER_HEIGHT],
    pub chars: [[u32; 640];400],
}

pub fn get_resolution() -> (usize, usize) {
    FRAME_DRAWER.lock().get_resolution()
}

fn check_in_range(x:usize, y:usize, width:usize, height:usize)  -> bool {
        x + 2 < width && y < height
}

pub fn get_index_fn(width:usize) -> Box<Fn(usize, usize)->usize>{
    Box::new(move |x:usize, y:usize| y * width + x )
}