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

///The default font file

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



/// An instance of a text virtual frame buffer which can be displayed to the screen.
// pub struct TextVFrameBuffer {
//     //The cursor in the text frame buffer
//     //pub cursor:Mutex<Cursor>, Cursor should belong to the terminal
//     ///The virtual frame buffer to be displayed in
//     pub vbuffer:Arc<Mutex<FrameBuffer>>
// }




///Dropped   code. Cursor should belong to the terminal
///A cursor struct. It contains the position of a cursor, whether it is enabled, 
///the frequency it blinks, the last time it blinks, and the current blink state show/hidden
pub struct Cursor {
    line:usize,
    column:usize,
    enabled:bool,
    freq:u64,
    time:TscTicks,
    show:bool,
}

impl Cursor {
    ///create a new cursor struct
    pub fn new(li:usize, col:usize, ena:bool) -> Cursor {
        Cursor {
            line:li,
            column:col,
            enabled:ena,
            freq:400000000,
            time:tsc_ticks(),
            show:true,
        }
    }

    ///update the cursor position
    pub fn update(&mut self, line:usize, column:usize, reset:bool) {
        self.line = line;
        self.column = column;
        if reset {
            self.show = true;
            self.time = tsc_ticks();
        }      
    }

    ///enable a cursor
    pub fn enable(&mut self) {
        self.enabled = true;
        self.time = tsc_ticks();
        self.show = true;
    }

    ///disable a cursor
    pub fn disable(&mut self) {
        self.enabled = false;
     }

    ///change the blink state show/hidden of a cursor. The terminal calls this function in a loop
    pub fn blink(&mut self) -> bool{
        if self.enabled {
            let time = tsc_ticks();
            if let Some(duration) = time.sub(&(self.time)) {
                if let Some(ns) = duration.to_ns() {
                    if ns >= self.freq {
                        self.time = time;
                        self.show = !self.show;
                        return true;
                    }
                }
            }
        }
        false
    }

    ///The the fields of the cursor object
    pub fn get_info(&self) -> (usize, usize, bool) {
        (self.line, self.column, self.show)
    }
}
