//! This crate is a frame buffer manager. 
//! * It defines a FrameBuffer structure and creates new framebuffers for applications

#![no_std]

extern crate spin;
extern crate acpi;
extern crate memory;
extern crate alloc;
extern crate owning_ref;
#[macro_use] extern crate log;

use owning_ref::BoxRefMut;
use spin::{Mutex, Once};
use memory::{FRAME_ALLOCATOR, Frame, PageTable, PhysicalAddress, 
    EntryFlags, MappedPages, MemoryManagementInfo,
    get_kernel_mmi_ref};
use core::ops::DerefMut;
use alloc::boxed::Box;

/// A Pixel is a u32 interger. The lower 24 bits of a Pixel specifies the RGB color of a pixel
pub type Pixel = u32;

/// The final framebuffer instance. It contains the pages which are mapped to the physical framebuffer
pub static FINAL_FRAME_BUFFER: Once<Mutex<FrameBuffer>> = Once::new();

// Every pixel is of u32 type
const PIXEL_BYTES: usize = 4;

/// Init the final frame buffer. 
/// Allocate a block of memory and map it to the physical framebuffer frames.
pub fn init() -> Result<(), &'static str > {
    // Get the graphic mode information
    let vesa_display_phys_start: PhysicalAddress;
    let buffer_width: usize;
    let buffer_height: usize;
    {
        let graphic_info = acpi::madt::GRAPHIC_INFO.lock();
        if graphic_info.physical_address == 0 {
            return Err("Fail to get graphic mode infomation!");
        }
        vesa_display_phys_start = PhysicalAddress::new(graphic_info.physical_address as usize)?;
        buffer_width = graphic_info.width as usize;
        buffer_height = graphic_info.height as usize;
    };

    // Initialize the final framebuffer
    let framebuffer = FrameBuffer::new(buffer_width, buffer_height, Some(vesa_display_phys_start))?;
    FINAL_FRAME_BUFFER.call_once(|| {
        Mutex::new(framebuffer)}
    );
    
    Ok(())
}

/// The virtual frame buffer struct. It contains the size of the buffer and a buffer array
pub struct FrameBuffer {
    width: usize,
    height: usize,
    buffer: BoxRefMut<MappedPages, [Pixel]>
}

impl FrameBuffer {
    /// Create a new virtual frame buffer with specified size
    /// If the physical_address is specified, the new virtual frame buffer will be mapped to hardware's physical memory at that address
    /// If the physical_address is none, the new function will allocate a block of physical memory at a random address and map the new frame buffer to that memory
    pub fn new(width: usize, height: usize, physical_address: Option<PhysicalAddress>) -> Result<FrameBuffer, &'static str>{       
        // get a reference to the kernel's memory mapping information
        let kernel_mmi_ref = get_kernel_mmi_ref().expect("KERNEL_MMI was not yet initialized!");
        let mut kernel_mmi_locked = kernel_mmi_ref.lock();

        // destructure the kernel's MMI so we can access its page table
        let MemoryManagementInfo { 
            page_table: ref mut kernel_page_table, 
            .. // don't need to access other stuff in kernel_mmi
        } = *kernel_mmi_locked;
        let size = width * height * PIXEL_BYTES;

        match kernel_page_table {
            &mut PageTable::Active(ref mut active_table) => {
                //Map the physical frame buffer memory
                let pages = match memory::allocate_pages_by_bytes(size) {
                    Some(pages) => { pages },
                    None => { return Err("FrameBuffer::new() couldn't allocate pages."); }
                };
                
                let allocator_mutex = FRAME_ALLOCATOR.try();
                match allocator_mutex {
                    Some(_) => { },
                    None => { return Err("FrameBuffer::new() couldn't get frame allocator"); }
                }

                let mut allocator = try!(allocator_mutex.ok_or("allocate frame buffer")).lock();
                let vesa_display_flags: EntryFlags = EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;
                let mapped_frame_buffer = if let Some(address) = physical_address {                
                    try!(active_table.map_allocated_pages_to(
                        pages, 
                        Frame::range_inclusive_addr(address, size), 
                        vesa_display_flags, 
                        allocator.deref_mut())
                    )
                } else {
                    try!(active_table.map_allocated_pages(
                        pages, 
                        vesa_display_flags, 
                        allocator.deref_mut())
                    )
                };

                // Create a reference to the mapped frame buffer pages as slice
                let buffer = BoxRefMut::new(Box::new(mapped_frame_buffer)).
                    try_map_mut(|mp| mp.as_slice_mut(0, width * height))?;
            
                return Ok(FrameBuffer{
                    width: width,
                    height: height,
                    buffer: buffer
                });             
            },
            _ => { 
                return Err("FrameBuffer::new()  Couldn't get kernel's active_table");
            }
        }
    }

    /// return a reference to the buffer
    pub fn buffer_mut(&mut self) -> &mut BoxRefMut<MappedPages, [Pixel]> {
        return &mut self.buffer
    }

    /// return a reference to the buffer
    pub fn buffer(&self) -> &BoxRefMut<MappedPages, [Pixel]> {
        return &self.buffer
    }

    /// get the size of the frame buffer. Return (width, height).
    pub fn get_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    // ///get a function to compute the index of a pixel in the buffer array. The returned function is (x:usize, y:usize) -> index:usize
    // pub fn get_index_fn(&self) -> Box<Fn(usize, usize)->usize>{
    //     let width = self.width;
    //     Box::new(move |x:usize, y:usize| y * width + x )
    // }

    /// compute the index of pixel (x, y) in the buffer array
    pub fn index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    /// check if a pixel (x,y) is within the framebuffer
    pub fn check_in_range(&self, x: usize, y: usize)  -> bool {
        x < self.width && y < self.height
    }
}

/// Get the size of the final framebuffer. Return (width, height)
pub fn get_screen_size() -> Result<(usize, usize), &'static str> {
    let final_buffer = FINAL_FRAME_BUFFER.try().
        ok_or("The final frame buffer was not yet initialized")?.lock();
    Ok((final_buffer.width, final_buffer.height))
}