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

use core::ptr::Unique;
use spin::Mutex;
use memory::{FRAME_ALLOCATOR, Frame, PageTable, PhysicalAddress, 
    EntryFlags, allocate_pages_by_bytes, MappedPages, MemoryManagementInfo,
    get_kernel_mmi_ref};
use core::ops::DerefMut;
use kernel_config::memory::KERNEL_OFFSET;

pub mod text_buffer;
pub mod font;


//const VGA_BUFFER_ADDR: usize = 0xa0000;
const VGA_BUFFER_ADDR: usize = 0xb8000 + KERNEL_OFFSET;

//Size of VESA mode 0x4112

///The width of the screen
pub const FRAME_BUFFER_WIDTH:usize = 640;

///The height of the screen
pub const FRAME_BUFFER_HEIGHT:usize = 400;

static FRAME_BUFFER_PAGES:Mutex<Option<MappedPages>> = Mutex::new(None);

/// try to unwrap an option. return error result if fails. 
#[macro_export]
macro_rules! try_opt_err {
    ($e:expr, $s:expr) =>(
        match $e {
            Some(v) => v,
            None => return Err($s),
        }
    )
}

/// Init the frame buffer. Allocate a block of memory and map it to the frame buffer frames.
pub fn init() -> Result<(), &'static str > {
    
    let rs = font::init();
    if rs.is_ok() {
         trace!("frame_buffer text initialized.");
    } else {
         debug!("frame_buffer::init(): {}", rs.unwrap_err());
    }

    //Allocate VESA frame buffer
    const VESA_DISPLAY_PHYS_START: PhysicalAddress = 0xFD00_0000;
    const VESA_DISPLAY_PHYS_SIZE: usize = FRAME_BUFFER_WIDTH*FRAME_BUFFER_HEIGHT*4;

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
            let pages = try_opt_err!(allocate_pages_by_bytes(VESA_DISPLAY_PHYS_SIZE), "framebuffer::init() couldn't allocate pages.");
            let vesa_display_flags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;
            let allocator_mutex = FRAME_ALLOCATOR.try();
            if allocator_mutex.is_none(){
                return Err("framebuffer::init() Couldn't get frame allocator");
            } 

            let err = FRAME_DRAWER.lock().init_frame_buffer(pages.start_address());
            if err.is_err() {
                debug!("Fail to init frame buffer");
                return err;
            }
            let mut allocator = try!(allocator_mutex.ok_or("allocate frame buffer")).lock();
            let mapped_frame_buffer = try!(active_table.map_allocated_pages_to(
                pages, 
                Frame::range_inclusive_addr(VESA_DISPLAY_PHYS_START, VESA_DISPLAY_PHYS_SIZE), 
                vesa_display_flags, 
                allocator.deref_mut())
            );

            let mut pages = FRAME_BUFFER_PAGES.lock();
            *pages = Some(mapped_frame_buffer);

            Ok(())
        }
        _ => { 
            return Err("framebuffer::init() Couldn't get kernel's active_table");
        }
    }
}

static FRAME_DRAWER: Mutex<Drawer> = {
    Mutex::new(Drawer {
        start_address:0,
        buffer: unsafe {Unique::new_unchecked((VGA_BUFFER_ADDR) as *mut _) },
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
    start_address: usize,
    buffer: Unique<Buffer> ,
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

    fn draw_pixel(&mut self, x:usize, y:usize, color:u32) {
        self.buffer().chars[y][x] = color;
    }

    fn check_in_range(&mut self, x:usize, y:usize) -> bool {
        x + 2 < FRAME_BUFFER_WIDTH && y < FRAME_BUFFER_HEIGHT
    }

    fn draw_line(&mut self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, color:u32){
        let width:i32 = end_x-start_x;
        let height:i32 = end_y-start_y;
        if width.abs() > height.abs() {
            let mut y;
            let mut x = start_x;
            let step = if width > 0 {1} else {-1};
            loop {
                if x == end_x {
                    break;
                }
                y = (x - start_x) * height / width + start_y;
                if self.check_in_range(x as usize,y as usize) {
                    self.buffer().chars[y as usize][x as usize] = color;
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
                if self.check_in_range(x as usize,y as usize) {
                    self.buffer().chars[y as usize][x as usize] = color;
                }
                y += step;   
            }
        }
    }

    fn draw_rectangle(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
        let end_x:usize = {if start_x + width < FRAME_BUFFER_WIDTH { start_x + width } 
            else { FRAME_BUFFER_WIDTH }};
        let end_y:usize = {if start_y + height < FRAME_BUFFER_HEIGHT { start_y + height } 
            else { FRAME_BUFFER_HEIGHT }};  

        let mut x = start_x;
        loop {
            if x == end_x {
                break;
            }
            self.buffer().chars[start_y][x] = color;
            self.buffer().chars[end_y-1][x] = color;
            x += 1;
        }

        let mut y = start_y;
        loop {
            if y == end_x {
                break;
            }
            self.buffer().chars[y][start_x] = color;
            self.buffer().chars[y][end_x-1] = color;
            y += 1;
        }
    }

    fn fill_rectangle(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
        let end_x:usize = {if start_x + width < FRAME_BUFFER_WIDTH { start_x + width } 
            else { FRAME_BUFFER_WIDTH }};
        let end_y:usize = {if start_y + height < FRAME_BUFFER_HEIGHT { start_y + height } 
            else { FRAME_BUFFER_HEIGHT }};  

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
            self.buffer().chars[y][x] = color;
            x += 1;
        }
    }

    fn buffer(&mut self) -> &mut Buffer {
         unsafe { self.buffer.as_mut() }
    } 

    fn init_frame_buffer(&mut self, virtual_address:usize) -> Result<(), &'static str>{
        if self.start_address == 0 {
            self.start_address = virtual_address;
            self.buffer = try_opt_err!(Unique::new((virtual_address) as *mut _), "Fail to init frame buffer"); 
            trace!("Set frame buffer address {:#x}", virtual_address);
        }

        Ok(())
    }  
}

pub struct Buffer {
    //chars: [Volatile<[u8; FRAME_BUFFER_WIDTH]>;FRAME_BUFFER_HEIGHT],
    pub chars: [[u32; FRAME_BUFFER_WIDTH];FRAME_BUFFER_HEIGHT],
}
