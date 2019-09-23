//! This crate is a frame buffer manager.
//! * It defines a FrameBuffer structure and creates new framebuffers for applications

#![no_std]

extern crate multicore_bringup;
extern crate spin;
extern crate alloc;
extern crate memory;
extern crate owning_ref;
extern crate frame_buffer;

use alloc::boxed::Box;
use core::ops::DerefMut;
use memory::{EntryFlags, FrameRange, MappedPages,PhysicalAddress, FRAME_ALLOCATOR};
use core::hash::{Hash, Hasher, SipHasher};
use owning_ref::BoxRefMut;
use spin::{Mutex, Once};
use frame_buffer::{Pixel, FrameBuffer, FINAL_FRAME_BUFFER};

// Every pixel is of u32 type
const PIXEL_BYTES: usize = 4;

/// Initialize the final frame buffer.
/// Allocate a block of memory and map it to the physical framebuffer frames.
/// Init the frame buffer. Allocate a block of memory and map it to the frame buffer frames.
pub fn init() -> Result<(), &'static str> {
    // Get the graphic mode information
    let vesa_display_phys_start: PhysicalAddress;
    let buffer_width: usize;
    let buffer_height: usize;
    {
        let graphic_info = multicore_bringup::GRAPHIC_INFO.lock();
        if graphic_info.physical_address == 0 {
            return Err("Fail to get graphic mode infomation!");
        }
        vesa_display_phys_start = PhysicalAddress::new(graphic_info.physical_address as usize)?;
        buffer_width = graphic_info.width as usize;
        buffer_height = graphic_info.height as usize;
    };
    // Initialize the final framebuffer
    let framebuffer = FrameBufferRGB::new(buffer_width, buffer_height, Some(vesa_display_phys_start))?;
    FINAL_FRAME_BUFFER.call_once(|| Mutex::new(Box::new(framebuffer)));

    Ok(())
}

/// The virtual frame buffer struct. It contains the size of the buffer and a buffer array
#[derive(Hash)]
pub struct FrameBufferRGB {
    width: usize,
    height: usize,
    buffer: BoxRefMut<MappedPages, [Pixel]>,
}

impl FrameBufferRGB {
    /// Create a new virtual frame buffer with specified size.
    /// If the physical_address is specified, the new virtual frame buffer will be mapped to hardware's physical memory at that address.
    /// If the physical_address is none, the new function will allocate a block of physical memory at a random address and map the new frame buffer to that memory.
    pub fn new(
        width: usize,
        height: usize,
        physical_address: Option<PhysicalAddress>,
    ) -> Result<FrameBufferRGB, &'static str> {
        // get a reference to the kernel's memory mapping information
        let kernel_mmi_ref =
            memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
        let allocator = FRAME_ALLOCATOR
            .try()
            .ok_or("Couldn't get Frame Allocator")?;

        let vesa_display_flags: EntryFlags =
            EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::GLOBAL | EntryFlags::NO_CACHE;

        let size = width * height * PIXEL_BYTES;
        let pages = memory::allocate_pages_by_bytes(size).ok_or("could not allocate pages")?;

        let mapped_frame_buffer = if let Some(address) = physical_address {
            let frame = FrameRange::from_phys_addr(address, size);
            try!(kernel_mmi_ref.lock().page_table.map_allocated_pages_to(
                pages,
                frame,
                vesa_display_flags,
                allocator.lock().deref_mut()
            ))
        } else {
            try!(kernel_mmi_ref.lock().page_table.map_allocated_pages(
                pages,
                vesa_display_flags,
                allocator.lock().deref_mut()
            ))
        };

        // create a refence to transmute the mapped frame buffer pages as a slice
        let buffer = BoxRefMut::new(Box::new(mapped_frame_buffer))
            .try_map_mut(|mp| mp.as_slice_mut(0, width * height))?;

        Ok(FrameBufferRGB {
            width: width,
            height: height,
            buffer: buffer,
        })
    }

    /// return a mutable reference to the buffer
    pub fn buffer_mut(&mut self) -> &mut BoxRefMut<MappedPages, [Pixel]> {
        return &mut self.buffer;
    }


}

impl FrameBuffer for FrameBufferRGB {
    /// return a reference to the buffer
    fn buffer(&self) -> &BoxRefMut<MappedPages, [Pixel]> {
        return &self.buffer;
    }

    /// get the size of the frame buffer. Return (width, height).
    fn get_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    // ///get a function to compute the index of a pixel in the buffer array. The returned function is (x:usize, y:usize) -> index:usize
    // pub fn get_index_fn(&self) -> Box<Fn(usize, usize)->usize>{
    //     let width = self.width;
    //     Box::new(move |x:usize, y:usize| y * width + x )
    // }

    /// check if a pixel (x,y) is within the framebuffer
    fn check_in_buffer(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.height
    }

    fn buffer_copy(&mut self, src:&[Pixel], dest_start:usize){
        let len = src.len();
        let dest_end = dest_start + len;
        self.buffer_mut()[dest_start..dest_end].copy_from_slice(src);
    }

    fn get_hash(&self) -> u64 {
        let mut s = SipHasher::new();
        self.hash(&mut s);
        s.finish()
    }

    fn draw_pixel(&mut self, x: usize, y: usize, color: Pixel) {
        let index = self.index(x, y);
        self.buffer[index] = color;
    }

    /// compute the index of pixel (x, y) in the buffer array
    fn index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

}