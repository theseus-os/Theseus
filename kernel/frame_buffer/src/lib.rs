//! The frame buffer for display on the screen


#![no_std]
#![feature(alloc)]
#![feature(const_fn)]
#![feature(unique)]
#![feature(unique)]
#![feature(asm)]

extern crate spin;

extern crate volatile;
extern crate serial_port;
extern crate memory;
extern crate irq_safety;

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;


use core::ptr::Unique;
use spin::Mutex;
use volatile::Volatile;
use alloc::string::String;
use alloc::vec::Vec;
use memory::{FRAME_ALLOCATOR, Frame, ActivePageTable, PageTable, PhysicalAddress, 
    EntryFlags, allocate_pages_by_bytes, allocate_pages, MappedPages, MemoryManagementInfo,
    get_kernel_mmi_ref};
use irq_safety::MutexIrqSafe;
use core::ops::DerefMut;


const VGA_BUFFER_ADDR: usize = 0xa0000;

//Size of VESA mode 0x4112
pub const FRAME_BUFFER_WIDTH:usize = 640*3;
pub const FRAME_BUFFER_HEIGHT:usize = 480;

pub static mut frame_buffer_direction:Direction = Direction::Right;

pub static mut frame_buffer_pages:Option<MappedPages> = None;


pub fn init() -> Result<(), &'static str > {

    //Wenqiu Allocate VESA frame buffer
    const VESA_DISPLAY_PHYS_START: PhysicalAddress = 0xFD00_0000;
    const VESA_DISPLAY_PHYS_SIZE: usize = FRAME_BUFFER_WIDTH*FRAME_BUFFER_HEIGHT;

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
            let pages = allocate_pages_by_bytes(VESA_DISPLAY_PHYS_SIZE).unwrap();
            let vesa_display_flags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;
            let allocator_mutex = FRAME_ALLOCATOR.try();
            if allocator_mutex.is_none(){
                return Err("framebuffer::init() Couldn't get frame allocator");
            } 

            FRAME_DRAWER.lock().init_frame_buffer(pages.start_address());
            let mut allocator = try!(allocator_mutex.ok_or("asdfasdf")).lock();
            let mapped_frame_buffer = try!(active_table.map_allocated_pages_to(
                pages, 
                Frame::range_inclusive_addr(VESA_DISPLAY_PHYS_START, VESA_DISPLAY_PHYS_SIZE), 
                vesa_display_flags, 
                allocator.deref_mut())
            );

            unsafe { frame_buffer_pages = Some(mapped_frame_buffer); }

            Ok(())
        }
        _ => { 
            return Err("framebuffer::init() Couldn't get kernel's active_table");
        }
    }
}


#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

pub static FRAME_DRAWER: Mutex<Drawer> = {
    Mutex::new(Drawer {
        start_address:0,
        buffer: unsafe {Unique::new_unchecked((VGA_BUFFER_ADDR) as *mut _) },
        depth: [[core::usize::MAX;FRAME_BUFFER_WIDTH/3];FRAME_BUFFER_HEIGHT],
    })
};




#[macro_export]
macro_rules! draw_pixel {
    ($x:expr, $y:expr, $color:expr) => ({
        $crate::draw_pixel($x, $y, $color);
    });
}


#[doc(hidden)]
pub fn draw_pixel(x:usize, y:usize, z:usize, color:usize) {
    unsafe{ FRAME_DRAWER.force_unlock();}
    FRAME_DRAWER.lock().draw_pixel(x, y, z, color)
}

#[macro_export]
macro_rules! draw_line {
    ($start_x:expr, $start_y:expr, $end_x:expr, $end_y:expr, $color:expr) => ({
        $crate::draw_line($start_x, $start_y, $end_x, $end_y, $color);
    });
}


#[doc(hidden)]
pub fn draw_line(start_x:usize, start_y:usize, end_x:usize, end_y:usize, color:usize) {
    unsafe{ FRAME_DRAWER.force_unlock();}
    FRAME_DRAWER.lock().draw_line(start_x as i32, start_y as i32, end_x as i32, end_y as i32, color)
}

#[macro_export]
macro_rules! draw_square {
    ($start_x:expr, $start_y:expr, $width:expr, $height:expr, $color:expr) => ({
        $crate::draw_square($start_x, $start_y, $width, $height, $color);
    });
}


#[doc(hidden)]
pub fn draw_square(start_x:usize, start_y:usize, width:usize, height:usize, color:usize) {
    unsafe{ FRAME_DRAWER.force_unlock();}
    FRAME_DRAWER.lock().draw_square(start_x, start_y, width, height, color)
}

#[macro_export]
macro_rules! set_direction {
    ($direction:expr) => ({
        $crate::draw_square($direction);
    });
}


#[doc(hidden)]
pub fn set_direction(direction:Direction) {
    unsafe { frame_buffer_direction = direction; }
}

/*#[macro_export]
 macro_rules! init_frame_buffer {
     ($v_add:expr) => (
         {
             $crate::init_frame_buffer($v_add);
         }
     )
 }


#[doc(hidden)]
pub fn init_frame_buffer(virtual_address:usize) {
    FRAME_DRAWER.lock().init_frame_buffer(virtual_address);
}
*/

pub struct Point {
    pub x: usize,
    pub y: usize,
    pub z: usize,
    pub color: usize,
}



pub struct Drawer {
    start_address: usize,
    buffer: Unique<Buffer>,
    depth : [[usize; FRAME_BUFFER_WIDTH/3]; FRAME_BUFFER_HEIGHT], 
}



impl Drawer {
    pub fn draw_pixel(&mut self, x:usize, y:usize, z:usize, color:usize){
        if x*3+2 >= FRAME_BUFFER_WIDTH || y >= FRAME_BUFFER_HEIGHT {
            return
        }
        if z > self.depth[y][x] {
            return
        }

        trace!("x:{}, y:{}, z:{} color:{}", x, y, z, color);

        self.depth[y][x] = z;

        self.buffer().chars[y][x*3] = (color & 255) as u8;//.write((color & 255) as u8);
        self.buffer().chars[y][x*3 + 1] = (color >> 8 & 255) as u8;//.write((color >> 8 & 255) as u8);
        self.buffer().chars[y][x*3 + 2] = (color >> 16 & 255) as u8;//.write((color >> 16 & 255) as u8); 
    
    }

    pub fn draw_points(&mut self, points:Vec<Point>){
        for p in points{
            draw_pixel(p.x, p.y, p.z, p.color);
        }
      
    }

    pub fn check_in_range(&mut self, x:usize, y:usize) -> bool {
        x + 2 < FRAME_BUFFER_WIDTH && y < FRAME_BUFFER_HEIGHT
    }

    pub fn draw_line(&mut self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, color:usize){
        let width:i32 = end_x-start_x;
        let height:i32 = end_y-start_y;
        let mut points = Vec::new();
        if width.abs() > height.abs() {
            let mut y;
            for x in start_x..end_x {
                y = ((x-start_x)*height/width+start_y);
                if(self.check_in_range(x as usize,y as usize)){
                    points.push(Point{x:x as usize, y:y as usize, z:0, color:color});
                }
            }
        }
        else {
            let mut x;
            for y in start_y..end_y {
                x = (y-start_y)*width/height+start_x;
                if(self.check_in_range(x as usize,y as usize)){
                    points.push(Point{x:x as usize, y:y as usize, z:0, color:color});
                }            
            }
        }
        self.draw_points(points);
    }

    pub fn draw_square(&mut self, start_x:usize, start_y:usize, width:usize, height:usize, color:usize){
        let end_x:usize = if start_x + width < FRAME_BUFFER_WIDTH { start_x + width } 
            else { FRAME_BUFFER_WIDTH };
        let end_y:usize = if start_y + height < FRAME_BUFFER_HEIGHT { start_y + height } 
            else { FRAME_BUFFER_HEIGHT };  
        let mut points = Vec::new();

        for x in start_x..end_x{
            for y in start_y..end_y{
                points.push(Point{x:x as usize, y:y as usize, z:0, color:color});
              // draw_pixel(x, y, color);
            }
        }
        self.draw_points(points);

    }


    fn buffer(&mut self) -> &mut Buffer {
        unsafe { self.buffer.as_mut() }
    } 

    pub fn init_frame_buffer(&mut self, virtual_address:usize) {
        if(self.start_address == 0){
            unsafe {
                self.start_address = virtual_address;
                self.buffer = Unique::new_unchecked((virtual_address) as *mut _); 
            }
            trace!("Set frame buffer address {:#x}", virtual_address);
        }
    }  
}



struct Buffer {
    //chars: [Volatile<[u8; FRAME_BUFFER_WIDTH]>;FRAME_BUFFER_HEIGHT],
    chars: [[u8; FRAME_BUFFER_WIDTH];FRAME_BUFFER_HEIGHT],
}


