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
#[macro_use] extern crate acpi;



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
const BACKGROUD_COLOR:usize = 0x000000;


//Size of VESA mode 0x4112
pub const FRAME_BUFFER_WIDTH:usize = 640*3;
pub const FRAME_BUFFER_HEIGHT:usize = 480;


pub static mut frame_buffer_pages:Option<MappedPages> = None;

#[macro_export]
macro_rules! try_opt_err {
    ($e:expr, $s:expr) =>(
        match $e {
            Some(v) => v,
            None => return Err($s),
        }
    )
}

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
            let pages = try_opt_err!(allocate_pages_by_bytes(VESA_DISPLAY_PHYS_SIZE), "frame_buffer_3d::init() couldn't allocate pages.");
            let vesa_display_flags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;
            let allocator_mutex = FRAME_ALLOCATOR.try();
            if allocator_mutex.is_none(){
                return Err("framebuffer::init() Couldn't get frame allocator");
            } 

            let err = FRAME_DRAWER.lock().init_frame_buffer(pages.start_address());
            if err.is_err(){
                debug!("Fail to init frame buffer");
                return err;
            }
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


pub static FRAME_DRAWER: Mutex<Drawer> = {
    Mutex::new(Drawer {
        start_address:0,
        buffer: unsafe {Unique::new_unchecked((VGA_BUFFER_ADDR) as *mut _) },
        depth: [[core::usize::MAX;FRAME_BUFFER_WIDTH/3];FRAME_BUFFER_HEIGHT],
    })
};




/*#[macro_export]
macro_rules! draw_pixel {
    ($x:expr, $y:expr, $color:expr) => ({
        $crate::draw_pixel($x, $y, 0, $color);
    });
}*/


#[doc(hidden)]
pub fn draw_pixel(x:usize, y:usize, color:usize) {
    FRAME_DRAWER.lock().draw_pixel(x, y, 0, color, true)
}

pub fn draw_pixel_3d(x:usize, y:usize, z:usize, color:usize, show:bool) {
    FRAME_DRAWER.lock().draw_pixel(x, y, z, color, show)
}

#[macro_export]
macro_rules! draw_line {
    ($start_x:expr, $start_y:expr, $end_x:expr, $end_y:expr, $color:expr) => ({
        $crate::draw_line($start_x, $start_y, $end_x, $end_y, 0, $color, true);
    });
}

#[doc(hidden)]
pub fn draw_line(start_x:usize, start_y:usize, end_x:usize, end_y:usize,
    color:usize) {
    FRAME_DRAWER.lock().draw_line(start_x as i32, start_y as i32, end_x as i32, 
        end_y as i32, 0, color, true)
}

#[doc(hidden)]
pub fn draw_line_3d(start_x:usize, start_y:usize, end_x:usize, end_y:usize, z:usize, 
    color:usize, show:bool) {
    FRAME_DRAWER.lock().draw_line(start_x as i32, start_y as i32, end_x as i32, 
        end_y as i32, z, color, show)
}

/*#[macro_export]
macro_rules! draw_square {
    ($start_x:expr, $start_y:expr, $width:expr, $height:expr, $color:expr) => ({
        $crate::draw_square($start_x, $start_y, $width, $height, 0, $color, true);
    });
}*/


#[doc(hidden)]
pub fn draw_square(start_x:usize, start_y:usize, width:usize, height:usize,
     color:usize) {
    FRAME_DRAWER.lock().draw_square(start_x, start_y, width, height, 0, color, true)
}

#[doc(hidden)]
pub fn draw_square_3d(start_x:usize, start_y:usize, width:usize, height:usize, z:usize,
     color:usize, show:bool) {
    FRAME_DRAWER.lock().draw_square(start_x, start_y, width, height, z, color, show)
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


//If z-depth is less than 0, clean this point with the color
impl Drawer {
    pub fn draw_pixel(&mut self, x:usize, y:usize, z:usize, color:usize, show:bool){
        if x*3+2 >= FRAME_BUFFER_WIDTH || y >= FRAME_BUFFER_HEIGHT {
            return
        }
        
        if z > self.depth[y][x] {
            return
        }

        self.depth[y][x] = if show {z} else {core::usize::MAX};
        let color = if show {color} else {BACKGROUD_COLOR};

        self.buffer().chars[y][x*3] = (color & 255) as u8;//.write((color & 255) as u8);
        self.buffer().chars[y][x*3 + 1] = (color >> 8 & 255) as u8;//.write((color >> 8 & 255) as u8);
        self.buffer().chars[y][x*3 + 2] = (color >> 16 & 255) as u8;//.write((color >> 16 & 255) as u8); 
    
    }

    pub fn draw_points(&mut self, points:Vec<Point>, show:bool){
        for p in points{
            self.draw_pixel(p.x, p.y, p.z, p.color,show);
        }
      
    }

    pub fn check_in_range(&mut self, x:usize, y:usize) -> bool {
        x + 2 < FRAME_BUFFER_WIDTH && y < FRAME_BUFFER_HEIGHT
    }

    pub fn draw_line(&mut self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, 
        z:usize, color:usize, show:bool){
        let width:i32 = end_x-start_x;
        let height:i32 = end_y-start_y;
        let mut points = Vec::new();
       
        let mut s;
        let mut e;
        if width.abs() > height.abs() {
            let mut y;
            s = core::cmp::min(start_x, end_x);
            e = core::cmp::max(start_x, end_x);
            for x in s..e {
                y = ((x-start_x)*height/width+start_y);
                if(self.check_in_range(x as usize,y as usize)){
                    points.push(Point{x:x as usize, y:y as usize, z:z, color:color});
                }
            }
        } else {
            let mut x;
            s = core::cmp::min(start_y, end_y);
            e = core::cmp::max(start_y, end_y);

            for y in s..e {
                x = (y-start_y)*width/height+start_x;

                if(self.check_in_range(x as usize,y as usize)){
                    points.push(Point{x:x as usize, y:y as usize, z:z, color:color});
                }            
            }
        }
        self.draw_points(points, show);
    }

    pub fn draw_square(&mut self, start_x:usize, start_y:usize, width:usize, 
        height:usize, z:usize, color:usize, show:bool){
        let end_x:usize = if start_x + width < FRAME_BUFFER_WIDTH { start_x + width } 
            else { FRAME_BUFFER_WIDTH };
        let end_y:usize = if start_y + height < FRAME_BUFFER_HEIGHT { start_y + height } 
            else { FRAME_BUFFER_HEIGHT };  
        let mut points = Vec::new();

        for x in start_x..end_x{
            for y in start_y..end_y{
                points.push(Point{x:x as usize, y:y as usize, z:z, color:color});
              // draw_pixel(x, y, color);
            }
        }
        self.draw_points(points, show);

    }


    fn buffer(&mut self) -> &mut Buffer {
        unsafe { self.buffer.as_mut() }
    } 

    pub fn init_frame_buffer(&mut self, virtual_address:usize) -> Result<(), &'static str>{
        if(self.start_address == 0){
            self.start_address = virtual_address;
            self.buffer = try_opt_err!(Unique::new((virtual_address) as *mut _), "Error in init frame buffer"); 
            trace!("Set frame buffer address {:#x}", virtual_address);
        }

        Ok(())
    }  
}



struct Buffer {
    //chars: [Volatile<[u8; FRAME_BUFFER_WIDTH]>;FRAME_BUFFER_HEIGHT],
    chars: [[u8; FRAME_BUFFER_WIDTH];FRAME_BUFFER_HEIGHT],
}


