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
#[macro_use] extern crate alloc;
extern crate owning_ref;

use owning_ref::BoxRefMut;
use spin::{Mutex, Once};
use memory::{FRAME_ALLOCATOR, Frame, PageTable, PhysicalAddress, 
    EntryFlags, allocate_pages_by_bytes, MappedPages, MemoryManagementInfo,
    get_kernel_mmi_ref};
use core::ops::DerefMut;
use alloc::boxed::Box;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;


static COMPOSITOR:Once<Mutex<Compositor>> = Once::new();

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
            COMPOSITOR.call_once(|| 
                Mutex::new(Compositor {
                    width:buffer_width,
                    height:buffer_height,
                    buffer: buffer,
                    frames: Vec::new(),
                })
            );

            Ok(())
        }
        _ => { 
            return Err("framebuffer::init() Couldn't get kernel's active_table");
        }
    }
}

pub struct Compositor {
    width:usize,
    height:usize,
    buffer:BoxRefMut<MappedPages, [u32]>,
    frames:Vec<DisplayFrame>
}

impl Compositor {
    fn get_resolution(&self) -> (usize, usize){
        (self.width, self.height)
    }

    fn add_frame(&mut self, frame:DisplayFrame) {
        self.frames.push(frame);
    }

    
    fn display(&mut self, vfb:&Arc<Mutex<VirtualFrameBuffer>>) {

        trace!("WEnqiu valid false" );        
        for item in self.frames.iter() {
            let reference = item.vframebuffer.upgrade();
            if let Some(vfb_rf) = reference {
                if Arc::ptr_eq(&vfb_rf, &vfb) {
                    trace!("Wenqiu frame buffer location {} {}", item.x, item.y);
                    self.buffer_copy(item.x, item.y, vfb);
                    return;
                }
            }
        }

        
    }

    fn get_position(&mut self, vfb:&Arc<Mutex<VirtualFrameBuffer>>) -> Result<(usize, usize), &'static str>{

        trace!("WEnqiu valid false" );        
        for item in self.frames.iter() {
            let reference = item.vframebuffer.upgrade();
            if let Some(vfb_rf) = reference {
                if Arc::ptr_eq(&vfb_rf, &vfb) {
                    return Ok((item.x, item.y));
                }
            }
        }

        Err("The virtual framebuffer hasn't been mapped")        
    }

    fn buffer_copy(&mut self, x:usize, y:usize, vfb:&Arc<Mutex<VirtualFrameBuffer>>) {
        let src_buffer = vfb.lock();
       
        for i in 0..src_buffer.height {
            let dest_start = self.width * ( i + y ) + x;
            let dest_end = dest_start + src_buffer.width;
            let src_start = src_buffer.width * i;
            let src_end = src_start + src_buffer.width;

            self.buffer[dest_start..dest_end].copy_from_slice(
                &(src_buffer.buffer[src_start..src_end])
            );
        }
    }
}

pub struct DisplayFrame {
    x:usize,
    y:usize,
    vframebuffer:Weak<Mutex<VirtualFrameBuffer>>,
}

pub struct VirtualFrameBuffer {
    width:usize,
    height:usize,
    buffer:Vec<u32>
}

impl VirtualFrameBuffer {
    pub fn new(width:usize, height:usize) -> Result<VirtualFrameBuffer, &'static str>{
        let buffer = vec![0;width * height];
        let vf = VirtualFrameBuffer {
            width:width,
            height:height,
            buffer:buffer
        };
        Ok(vf)
    }

    pub fn buffer(&mut self) -> &mut Vec<u32> {
        return &mut self.buffer
    }

    //get the resolution of the screen
    pub fn get_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    pub fn get_index_fn(&self) -> Box<Fn(usize, usize)->usize>{
         let width = self.width;
         Box::new(move |x:usize, y:usize| y * width + x )
    }

    pub fn check_in_range(&self, x:usize, y:usize)  -> bool {
        x < self.width && y < self.height
    }
}


// impl PhysicalFrameBuffer {
//         // set the graphic mode information of the buffer
//     // fn set_mode_info(&mut self, width:usize, height:usize, pages:MappedPages) {
//     //     self.width = width;
//     //     self.height = height;
//     //     self.pages = Some(pages);
//     // }

//     /// Get the resolution of the screen
//     pub fn get_resolution(&self) -> (usize, usize) {
//         (self.width, self.height)
//     }

//     // Check if a point is in the screen
//     pub fn check_in_range(&self, x:usize, y:usize)  -> bool {
//         x < self.width && y < self.height
//     }

//     pub fn get_index_fn(&self) -> Box<Fn(usize, usize)->usize>{
//         let width = self.width;
//         Box::new(move |x:usize, y:usize| y * width + x )
//     }

// }

// pub fn draw_in_buffer(index:usize, color:u32,  buffer:&mut[u32]) {
//     buffer[index] = color;
// }

pub fn map(x:usize, y:usize, vf:VirtualFrameBuffer) -> Result<Arc<Mutex<VirtualFrameBuffer>>, &'static str>{
    let vf_ref = Arc::new(Mutex::new(vf));
    map_ref(x, y, &vf_ref)?;
    Ok(vf_ref)
}

pub fn map_ref(x:usize, y:usize, vf_ref:&Arc<Mutex<VirtualFrameBuffer>>) -> Result<(), &'static str>{
    let vf_weak = Arc::downgrade(vf_ref);
    let frame = DisplayFrame {
        x:x,
        y:y,
        vframebuffer:vf_weak
    };
    let mut compositor = try!(COMPOSITOR.try().ok_or("Fail to get the compositor")).lock();
    
    compositor.add_frame(frame);
    Ok(())
}

pub fn get_resolution() -> Result<(usize, usize), &'static str> {
    let compositor = try!(COMPOSITOR.try().ok_or("Fail to get the physical frame buffer"));
    Ok(compositor.lock().get_resolution())
}

pub fn display(vfb:&Arc<Mutex<VirtualFrameBuffer>>) -> Result<(), &'static str>{
        trace!("WEnqiu flush");
    let compositor = try!(COMPOSITOR.try().ok_or("Fail to get the physical frame buffer"));
    compositor.lock().display(vfb);
    Ok(())
}


pub fn get_position(vfb:&Arc<Mutex<VirtualFrameBuffer>>) ->  Result<(usize, usize), &'static str> {
    let compositor = try!(COMPOSITOR.try().ok_or("Fail to get the physical frame buffer"));
    compositor.lock().get_position(vfb)
}
// // Check if a point is in the screen
// pub fn check_in_range(x:usize, y:usize, width:usize, height:usize)  -> bool {
//     x < width && y < height
// }