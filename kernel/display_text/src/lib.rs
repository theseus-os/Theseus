#![no_std]

extern crate tsc;
extern crate alloc;
extern crate spin;
extern crate frame_buffer;

use frame_buffer::{FrameBuffer};
use alloc::vec::{Vec};
use alloc::sync::{Arc};
use alloc::boxed::Box;
use spin::{Mutex};
use core::ops::DerefMut;

use tsc::{tsc_ticks, TscTicks};


/// Specifies where we want to scroll the display, and by how much
#[derive(Debug)]
pub enum DisplayPosition {
    /// Move the display to the very top of the FrameBuffer
    Start,
    /// Refresh the display without scrolling it
    Same, 
    /// Move the display down by the specified number of lines
    Down(usize),
    /// Move the display up by the specified number of lines
    Up(usize),
    /// Move the display to the very end of the FrameBuffer
    End
}






