//! This crate is frame buffer for display on screen in 3D mode


#![no_std]
#![feature(const_fn)]
#![feature(ptr_internals)]
#![feature(asm)]

extern crate spin;

extern crate volatile;
extern crate serial_port;
extern crate memory;
extern crate irq_safety;
extern crate alloc;

#[macro_use] extern crate log;
//#[macro_use] extern crate acpi;

use core::ptr::Unique;
use spin::{Mutex, Once};
use alloc::vec::Vec;
use memory::{FRAME_ALLOCATOR, FrameRange, VirtualAddress, PhysicalAddress, 
    EntryFlags, allocate_pages_by_bytes, MappedPages, get_kernel_mmi_ref};
use core::ops::DerefMut;


const VGA_BUFFER_ADDR: usize = 0xa0000;
const BACKGROUD_COLOR:usize = 0x000000;


//Size of VESA mode 0x4112

///The width of the screen
pub const FRAME_BUFFER_WIDTH:usize = 640*3;
///The height of the screen
pub const FRAME_BUFFER_HEIGHT:usize = 480;


static FRAME_BUFFER_PAGES: Once<MappedPages> = Once::new();


///Init the frame buffer in 3D mode. Allocate a block of memory and map it to the physical frame buffer.
pub fn init() -> Result<(), &'static str > {
    const VESA_DISPLAY_PHYS_START: usize = 0xFD00_0000;
    const VESA_DISPLAY_PHYS_SIZE: usize = FRAME_BUFFER_WIDTH * FRAME_BUFFER_HEIGHT;

    // get a reference to the kernel's memory mapping information
    let kernel_mmi_ref = get_kernel_mmi_ref().expect("KERNEL_MMI was not yet initialized!");
    let mut kernel_mmi_locked = kernel_mmi_ref.lock();

    let pages = allocate_pages_by_bytes(VESA_DISPLAY_PHYS_SIZE).ok_or("frame_buffer_3d::init() couldn't allocate pages")?;
    let vesa_display_flags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;
    let allocator = FRAME_ALLOCATOR.try().ok_or("Couldn't get frame allocator")?;

    FRAME_DRAWER.lock().init_frame_buffer(pages.start_address())?;
    let mapped_frame_buffer = kernel_mmi_locked.page_table.map_allocated_pages_to(
        pages, 
        FrameRange::from_phys_addr(PhysicalAddress::new_canonical(VESA_DISPLAY_PHYS_START), VESA_DISPLAY_PHYS_SIZE), 
        vesa_display_flags, 
        allocator.lock().deref_mut()
    )?;

    FRAME_BUFFER_PAGES.call_once(|| mapped_frame_buffer);

    Ok(())
}


static FRAME_DRAWER: Mutex<Drawer> = {
    Mutex::new(Drawer {
        start_address: VirtualAddress::zero(),
        buffer: unsafe {Unique::new_unchecked((VGA_BUFFER_ADDR) as *mut _) },
        depth: [[core::usize::MAX;FRAME_BUFFER_WIDTH/3];FRAME_BUFFER_HEIGHT],
    })
};

///draw a pixel in 2D compatible mode with coordinates and color.
pub fn draw_pixel(x:usize, y:usize, color:usize) {
    FRAME_DRAWER.lock().draw_pixel(x, y, 0, color, true)
}

///draw a pixel in 3D mode with coordinates and color.
pub fn draw_pixel_3d(x:usize, y:usize, z:usize, color:usize, show:bool) {
    FRAME_DRAWER.lock().draw_pixel(x, y, z, color, show)
}

///draw a line in 2D compatible mode with start and end coordinates and color.
pub fn draw_line(start_x:usize, start_y:usize, end_x:usize, end_y:usize,
    color:usize) {
    FRAME_DRAWER.lock().draw_line(start_x as i32, start_y as i32, end_x as i32, 
        end_y as i32, 0, color, true)
}

///draw a line in 3D mode with coordinates and color.
pub fn draw_line_3d(start_x:usize, start_y:usize, end_x:usize, end_y:usize, z:usize, 
    color:usize, show:bool) {
    FRAME_DRAWER.lock().draw_line(start_x as i32, start_y as i32, end_x as i32, 
        end_y as i32, z, color, show)
}

///draw a square in 2D compatible mode with upper left coordinates, width, height and color.
pub fn draw_square(start_x:usize, start_y:usize, width:usize, height:usize,
     color:usize) {
    FRAME_DRAWER.lock().draw_square(start_x, start_y, width, height, 0, color, true)
}

///draw a square in 3D mode with upper left coordinates, width, height and color.
pub fn draw_square_3d(start_x:usize, start_y:usize, width:usize, height:usize, z:usize,
     color:usize, show:bool) {
    FRAME_DRAWER.lock().draw_square(start_x, start_y, width, height, z, color, show)
}

struct Point {
    pub x: usize,
    pub y: usize,
    pub z: usize,
    pub color: usize,
}

struct Drawer {
    start_address: VirtualAddress,
    buffer: Unique<Buffer>,
    depth : [[usize; FRAME_BUFFER_WIDTH/3]; FRAME_BUFFER_HEIGHT], 
}

//If z-depth is less than 0, clean this point with the color
impl Drawer {
    fn draw_pixel(&mut self, x:usize, y:usize, z:usize, color:usize, show:bool){
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

    fn draw_points(&mut self, points:Vec<Point>, show:bool){
        for p in points{
            self.draw_pixel(p.x, p.y, p.z, p.color,show);
        }
      
    }

    fn check_in_range(&mut self, x:usize, y:usize) -> bool {
        x + 2 < FRAME_BUFFER_WIDTH && y < FRAME_BUFFER_HEIGHT
    }

    fn draw_line(&mut self, start_x:i32, start_y:i32, end_x:i32, end_y:i32, 
        z:usize, color:usize, show:bool){
        let width:i32 = end_x-start_x;
        let height:i32 = end_y-start_y;
        let mut points = Vec::new();
       
        if width.abs() > height.abs() {
            let mut y;
            let s = core::cmp::min(start_x, end_x);
            let e = core::cmp::max(start_x, end_x);
            for x in s..e {
                y = (x-start_x)*height/width+start_y;
                if self.check_in_range(x as usize,y as usize) {
                    points.push(Point{x:x as usize, y:y as usize, z:z, color:color});
                }
            }
        } else {
            let mut x;
            let s = core::cmp::min(start_y, end_y);
            let e = core::cmp::max(start_y, end_y);

            for y in s..e {
                x = (y-start_y)*width/height+start_x;

                if self.check_in_range(x as usize,y as usize) {
                    points.push(Point{x:x as usize, y:y as usize, z:z, color:color});
                }            
            }
        }
        self.draw_points(points, show);
    }

    fn draw_square(&mut self, start_x:usize, start_y:usize, width:usize, 
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

    fn init_frame_buffer(&mut self, virtual_address: VirtualAddress) -> Result<(), &'static str>{
        if self.start_address.value() == 0 {
            self.start_address = virtual_address;
            match Unique::new(virtual_address.value() as *mut _) {
                Some(buffer) => { 
                    self.buffer = buffer; 
                    trace!("Set frame buffer address {:#x}", virtual_address);
                },
                None => { return Err("Failed to init new frame buffer"); }
            }
        }
        Ok(())
    }  
}

struct Buffer {
    //chars: [Volatile<[u8; FRAME_BUFFER_WIDTH]>;FRAME_BUFFER_HEIGHT],
    chars: [[u8; FRAME_BUFFER_WIDTH];FRAME_BUFFER_HEIGHT],
}


