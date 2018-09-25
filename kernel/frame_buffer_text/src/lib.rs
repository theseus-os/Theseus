
#![no_std]
#![feature(alloc)]
#![feature(const_fn)]
#![feature(unique)]
#![feature(ptr_internals)]
#![feature(asm)]

extern crate tsc;
extern crate frame_buffer;
extern crate spin;
extern crate alloc;

pub mod font;

use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH, FONT_PIXEL};
use spin::{Mutex};
use alloc::boxed::Box;
use core::ops::DerefMut;
use frame_buffer::{FRAME_DRAWER};

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


//type Line = [u8; BUFFER_WIDTH];

//const BLANK_LINE: Line = [b' '; BUFFER_WIDTH];

/// An instance of a frame text buffer which can be displayed to the screen.
pub struct FrameTextBuffer {
    pub cursor:Mutex<Cursor>,
}

impl FrameTextBuffer {
    pub fn new() -> FrameTextBuffer {
        FrameTextBuffer {
            cursor:Mutex::new(Cursor::new(0, 0, true)),
        }
    }

    ///print a string by bytes
    pub fn print_by_bytes(&self, x:usize, y:usize, width:usize, height:usize, 
        slice: &str, font_color:u32, bg_color:u32) -> Result<(), &'static str> {
    
        let mut curr_line = 0;
        let mut curr_column = 0;
        //let mut cursor_pos = 0;

        let buffer_width = width/CHARACTER_WIDTH;
        let buffer_height = height/CHARACTER_HEIGHT;
        
        let mut drawer = FRAME_DRAWER.lock();

        //get the index computation function
        let index = drawer.get_index_fn();

        let mut buffer = match drawer.buffer() {
            Ok(rs) => { rs },
            Err(err) => {return Err(err);}
        };

        for byte in slice.bytes() {
            if byte == b'\n' {
                self.fill_blank (buffer, 
                    x + curr_column * CHARACTER_WIDTH,
                    y + curr_line * CHARACTER_HEIGHT,
                    x + width, 
                    y + (curr_line + 1 )* CHARACTER_HEIGHT, 
                    bg_color, &index);
                //cursor_pos += buffer_width - curr_column;
                curr_column = 0;
                curr_line += 1;
                if curr_line == buffer_height {
                    break;
                }
            } else {
                if curr_column == buffer_width {
                    curr_column = 0;
                    curr_line += 1;
                    if curr_line == buffer_height {
                        break;
                    }
                }
                self.print_byte(buffer, byte, font_color, bg_color, x, y, 
                    curr_line, curr_column, &index);
                curr_column += 1;
                //cursor_pos += 1;
            }
        }

        // Fill the blank of the last line
        self.fill_blank (buffer, 
            x + curr_column * CHARACTER_WIDTH,
            y + curr_line * CHARACTER_HEIGHT,
            x + width, 
            y + (curr_line + 1 )* CHARACTER_HEIGHT, 
            bg_color, &index);

        // Fill the blank of remaining lines
        self.fill_blank (buffer, 
            x, y + (curr_line + 1 )* CHARACTER_HEIGHT, x + width, y + height, 
            bg_color, &index);

        Ok(())
    }

    fn print_byte (&self, buffer:&mut[u32], byte:u8, font_color:u32, bg_color:u32,
            left:usize, top:usize, line:usize, column:usize, index:&Box<Fn(usize, usize)->usize>) {
        let x = left + column * CHARACTER_WIDTH;
        let y = top + line * CHARACTER_HEIGHT;
        let mut i = 0;
        let mut j = 0;

        let fonts = FONT_PIXEL.lock();
   
        loop {
            let mask:u32 = fonts[byte as usize][i][j];
            buffer[index(x + j, y + i)] = font_color & mask | bg_color & (!mask);
            j += 1;
            if j == CHARACTER_WIDTH {
                i += 1;
                if i == CHARACTER_HEIGHT {
                    break;
                }
                j = 0;
            }
        }
    }

    fn fill_blank(&self, buffer:&mut[u32], left:usize, top:usize, right:usize,
             bottom:usize, color:u32, index:&Box<Fn(usize, usize)->usize>){
        let mut x = left;
        let mut y = top;
        if left > right || top > bottom {
            return
        }
        loop {
            if x == right {
                y += 1;
                x = left;
            }
            if y == bottom {
                break;
            }
            buffer[index(x, y)] = color;
            x += 1;
        }
    }
    
}

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

    pub fn get_info(&self) -> (usize, usize, bool) {
        (self.line, self.column, self.show)
    }
}



