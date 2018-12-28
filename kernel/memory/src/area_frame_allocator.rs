// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use super::{Frame, FrameAllocator, FrameIter, PhysicalMemoryArea};
use alloc::vec::Vec;
use kernel_config::memory::PAGE_SIZE;


/// A stand-in for a Union
pub enum VectorArray<T: Clone> {
    Array((usize, [T; 32])),
    Vector(Vec<T>),
}
impl<T: Clone> VectorArray<T> {
    pub fn upgrade_to_vector(&mut self) {
        let new_val = { 
            match *self {
                VectorArray::Array((count, ref arr)) => { 
                    Some(VectorArray::Vector(arr[0..count].to_vec()))
                }
                _ => { 
                    None // no-op, it's already a Vector
                }
            }
        };
        if let Some(nv) = new_val {
            *self = nv;
        }
    }

    // pub fn iter(&self) -> ::core::slice::Iter<T> {
    //     match self {
    //         &VectorArray::Array((_count, arr)) => arr.iter(),
    //         &VectorArray::Vector(v) => v[0..v.len()].iter(),
    //     }
    // }

}




/// A frame allocator that uses the memory areas from the multiboot information structure as
/// source. The {kernel, multiboot}_{start, end} fields are used to avoid returning memory that is
/// already in use.
///
/// `kernel_end` and `multiboot_end` are _inclusive_ bounds.
pub struct AreaFrameAllocator {
    next_free_frame: Frame,
    current_area: Option<PhysicalMemoryArea>,
    available: VectorArray<PhysicalMemoryArea>,
    occupied: VectorArray<PhysicalMemoryArea>,
}

impl AreaFrameAllocator {
    pub fn new(available: [PhysicalMemoryArea; 32], avail_len: usize, occupied: [PhysicalMemoryArea; 32], occ_len: usize) 
               -> Result<AreaFrameAllocator, &'static str> {

        let mut allocator = AreaFrameAllocator {
            next_free_frame: Frame::containing_address(0),
            current_area: None,
            available: VectorArray::Array((avail_len, available)),
            occupied: VectorArray::Array((occ_len, occupied)),
        };
        allocator.select_next_area();
        Ok(allocator)
    }

    /// `available`: specifies whether the given `area` is an available or occupied memory area.
    pub fn add_area(&mut self, area: PhysicalMemoryArea, available: bool) -> Result<(), &'static str> {
        // match self.available {
        match if available { &mut self.available } else { &mut self.occupied } {
            &mut VectorArray::Array((ref mut count, ref mut arr)) => {
                if *count < arr.len() {
                    arr[*count] = area;
                    *count += 1;
                }
                else {
                    error!("AreaFrameAllocator::add_area(): {} array is already full!", if available { "available" } else { "occupied" } );
                    return Err("array is already full");
                }
            }
            &mut VectorArray::Vector(ref mut v) => {
                v.push(area);
            }
        }

        // debugging stuff below
        trace!("AreaFrameAllocator: updated {} area: =======================================", if available { "available" } else { "occupied" });
        match if available { &self.available } else { &self.occupied } {
            &VectorArray::Array((ref count, ref arr)) => {
                trace!("   Array[{}]: {:?}", count, arr);
            }
            & VectorArray::Vector(ref v) => {
                trace!("   Vector: {:?}", v);
            }
        }


        Ok(())
    }

    fn select_next_area(&mut self) {
        self.current_area = match self.available {
            VectorArray::Array((len, ref arr)) => {
                arr.iter().take(len)
                    .filter(|area| {
                        let address = area.base_addr + area.size_in_bytes - 1;
                        area.typ == 1 && Frame::containing_address(address as usize) >= self.next_free_frame
                    })
                    .min_by_key(|area| area.base_addr).cloned()
            }
            VectorArray::Vector(ref v) => {
                v.iter()
                    .filter(|area| {
                        let address = area.base_addr + area.size_in_bytes - 1;
                        area.typ == 1 && Frame::containing_address(address as usize) >= self.next_free_frame
                    })
                    .min_by_key(|area| area.base_addr).cloned()
            }
        };
        
            
        trace!("AreaFrameAllocator: selected next area {:?}", self.current_area);

        if let Some(area) = self.current_area {
            let start_frame = Frame::containing_address(area.base_addr as usize);
            if self.next_free_frame < start_frame {
                self.next_free_frame = start_frame;
            }
        }
    }

    /// Determines whether or not the current `next_free_frame` is within any occupied memory area,
    /// and advances it to the start of the next free region after the occupied area.
    fn skip_occupied_frames(&mut self) {
        let orig_frame: usize = self.next_free_frame.number;
        match self.occupied {
            VectorArray::Array((len, ref arr)) => {
                for area in arr.iter().take(len) {
                    let start = Frame::containing_address(area.base_addr);
                    let end = Frame::containing_address(area.base_addr + area.size_in_bytes);
                    if self.next_free_frame >= start && self.next_free_frame <= end {
                        self.next_free_frame = end + 1; 
                        trace!("AreaFrameAllocator: skipping frame {:?} to next frame {:?}", orig_frame, self.next_free_frame);
                        return;
                    }
                }
            }
            VectorArray::Vector(ref v) => {
                for area in v.iter() {
                    let start = Frame::containing_address(area.base_addr);
                    let end = Frame::containing_address(area.base_addr + area.size_in_bytes);
                    if self.next_free_frame >= start && self.next_free_frame <= end {
                        self.next_free_frame = end + 1; 
                        trace!("AreaFrameAllocator: skipping frame {:?} to next frame {:?}", orig_frame, self.next_free_frame);
                        return;
                    }
                }
            }
        };
    }
}

impl FrameAllocator for AreaFrameAllocator {

    fn allocate_frames(&mut self, num_frames: usize) -> Option<FrameIter> {
        // this is just a shitty way to get contiguous frames, since right now it's really easy to get them
        // it wastes the frames that are allocated 

        if let Some(first_frame) = self.allocate_frame() {
            let first_frame_paddr = first_frame.start_address();

            // here, we successfully got the first frame, so try to allocate the rest
            for i in 1..num_frames {
                if let Some(f) = self.allocate_frame() {
                    if f.start_address() == (first_frame_paddr + (i * PAGE_SIZE)) {
                        // still getting contiguous frames, so we're good
                        continue;
                    }
                    else {
                        // didn't get a contiguous frame, so let's try again
                        warn!("AreaFrameAllocator::allocate_frames(): could only alloc {}/{} contiguous frames (those are wasted), trying again!", i, num_frames);
                        return self.allocate_frames(num_frames);
                    }
                }
                else {
                    error!("Error: AreaFrameAllocator::allocate_frames(): couldn't allocate {} contiguous frames, out of memory!", num_frames);
                    return None;
                }
            }

            // here, we have allocated enough frames, and checked that they're all contiguous
            let last_frame = first_frame.clone() + num_frames - 1; // -1 because FrameIter is inclusive
            return Some(Frame::range_inclusive(first_frame, last_frame));
        }

        error!("Error: AreaFrameAllocator::allocate_frames(): couldn't allocate {} contiguous frames, out of memory!", num_frames);
        None
    }


    fn allocate_frame(&mut self) -> Option<Frame> {
        if let Some(area) = self.current_area {
            // first, see if we need to skip beyond the current area (it may be already occupied)
            self.skip_occupied_frames();

            // "clone" the frame to return it if it's free. Frame doesn't
            // implement Clone, but we can construct an identical frame.
            let frame = Frame { number: self.next_free_frame.number };

            // the last frame of the current area
            let last_frame_in_current_area = {
                let address = area.base_addr + area.size_in_bytes - 1;
                Frame::containing_address(address as usize)
            };

            if frame > last_frame_in_current_area {
                // all frames of current area are used, switch to next area
                self.select_next_area();
            } else {
                // frame is unused, increment `next_free_frame` and return it
                self.next_free_frame += 1;
                // trace!("AreaFrameAllocator: allocated frame {:?}", frame);
                return Some(frame);
            }
            // `frame` was not valid, try it again with the updated `next_free_frame`
            self.allocate_frame()
        } else {
            error!("FATAL ERROR: AreaFrameAllocator: out of physical memory!!!");
            None // no free frames left
        }
    }

    
    fn deallocate_frame(&mut self, _frame: Frame) {
        unimplemented!()
    }


    /// Call this when the kernel heap has been set up
    fn alloc_ready(&mut self) {
        self.available.upgrade_to_vector();
        self.occupied.upgrade_to_vector();
    }
}
