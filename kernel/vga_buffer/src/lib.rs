//! The vga buffer that implements basic printing in VGA text mode.

#![no_std]
#![feature(alloc)]
#![feature(const_fn)]
#![feature(unique)]

extern crate spin;
extern crate volatile;
extern crate alloc;
extern crate serial_port;
extern crate kernel_config;
// #[macro_use] extern crate log;

use core::ptr::Unique;
use core::cmp::min;
use core::fmt;
// use core::slice;
use core::mem;
use spin::Mutex;
use volatile::Volatile;
use alloc::string::String;
use alloc::Vec;
use kernel_config::memory::KERNEL_OFFSET;

/// defined by x86's physical memory maps
const VGA_BUFFER_VIRTUAL_ADDR: usize = 0xb8000 + KERNEL_OFFSET;

/// height of the VGA text window
const BUFFER_HEIGHT: usize = 25;
/// width of the VGA text window
const BUFFER_WIDTH: usize = 80;


// #[macro_export] pub mod raw;
pub mod raw;


/// Specifies where we want to scroll the display, and by how much
#[derive(Debug)]
pub enum DisplayPosition {
    /// Move the display to the very top of the VgaBuffer
    Start,
    /// Refresh the display without scrolling it
    Same, 
    /// Move the display down by the specified number of lines
    Down(usize),
    /// Move the display up by the specified number of lines
    Up(usize),
    /// Move the display to the very end of the VgaBuffer
    End
}


type Line = [ScreenChar; BUFFER_WIDTH];

const BLANK_LINE: Line = [ScreenChar::new(b' ', ColorCode::new(Color::LightGreen, Color::Black)); BUFFER_WIDTH];


/// An instance of a VGA text buffer which can be displayed to the screen.
pub struct VgaBuffer {
    /// the index of the line that is currently being displayed
    display_line: usize,
    /// whether the display is locked to scroll with the end and show new lines as printed
    display_scroll_end: bool,
    /// the column position in the last line where the next character will go
    column: usize,
    /// the actual buffer memory that can be written to the VGA memory
    lines: Vec<Line>,
}

impl VgaBuffer {
    /// Create a new VgaBuffer.
    pub fn new() -> VgaBuffer {
        VgaBuffer::with_capacity(1000)
    }

    // Create a new VgaBuffer with the given capacity, specified in number of lines. 
    pub fn with_capacity(num_initial_lines: usize) -> VgaBuffer {
        let first_line = BLANK_LINE;
        let mut lines = Vec::with_capacity(num_initial_lines);
        lines.push(first_line);

        VgaBuffer {
            display_line: 0,
            display_scroll_end: true,
            column: 0,
            lines: lines,
        }
    }
    

    pub fn write_str_with_color(&mut self, s: &str, color: ColorCode) {
        for byte in s.bytes() {
            match byte {
                // handle new line
                b'\n' => {
                    self.new_line(color);
                }

                // handle backspace
                // 0x08 => { }

                // all other regular bytes
                byte => {
                    {
                        let mut curr_line = self.lines.last_mut().unwrap();
                        curr_line[self.column] = ScreenChar::new(byte, color);
                    }
                    self.column += 1;

                    if self.column == BUFFER_WIDTH { // wrap to a new line
                        self.new_line(color); 
                    }
                }
            }
        }

        // refresh the VGA text display if the changes would be visible on screen
        // i.e., if the end of the vga buffer is visible
        let last_line = self.lines.len() - 1;;
        if  self.display_scroll_end || 
            (last_line >= self.display_line && last_line <= (self.display_line + BUFFER_HEIGHT))
        {
            self.display(DisplayPosition::End);
        }
        
        // // refresh the VGA text display if the changes would be visible on screen
        // // keep in mind the latest write to the VGA buffer is always written into the last element of self.lines
        // let display_line = self.display_line;
        // let written_line = self.lines.len();
        // if written_line >= display_line && written_line <= display_line + BUFFER_HEIGHT {
        //     // the recent write will be visible
        //     self.display(DisplayPosition::Same);
        // }

    }


    pub fn write_string_with_color(&mut self, s: &String, color: ColorCode) {
        self.write_str_with_color(s.as_str(), color);
    }


    pub fn write_args(&mut self, args: fmt::Arguments) -> fmt::Result {
        use core::fmt::Write;
        self.write_fmt(args)
    }

    /// To create a new line, this function does the following:
    /// 1) Clears the rest of the current line.
    /// 2) Resets the column index to 0 (beginning of next line).
    /// 3) Allocates a new Line and pushes it to the `lines` Vec.
    fn new_line(&mut self, color: ColorCode) {
        // clear out the rest of the current line
        let ref mut lines = self.lines;
        for c in self.column .. BUFFER_WIDTH {
            lines.last_mut().unwrap()[c] = ScreenChar::new(b' ', color);
        }
        
        self.column = 0; 
        lines.push([ScreenChar::new(b' ', color); BUFFER_WIDTH]);
    }



    /// Displays this VgaBuffer at the given string offset by flushing it to the screen.
    pub fn display(&mut self, position: DisplayPosition) {
        // trace!("VgaBuffer::display(): position {:?}", position);
        let (start, end) = match position {
            DisplayPosition::Start => {
                self.display_scroll_end = false;
                self.display_line = 0;
                (0, BUFFER_HEIGHT)
            }
            DisplayPosition::Up(u) => {
                if self.display_scroll_end {
                    // handle the case when it was previously at the end, but then scrolled up
                    self.display_line = self.display_line.saturating_sub(BUFFER_HEIGHT);
                }
                self.display_scroll_end = false;
                self.display_line = self.display_line.saturating_sub(u);
                (self.display_line, self.display_line.saturating_add(BUFFER_HEIGHT))
            }
            DisplayPosition::Down(d) => {
                if self.display_scroll_end {
                    // do nothing if we're already locked to the end
                }
                else {
                    self.display_line = self.display_line.saturating_add(d);
                    if self.display_line + BUFFER_HEIGHT >= self.lines.len() {
                        self.display_scroll_end = true;
                        self.display_line = self.lines.len() - 1;
                    }
                }
                (self.display_line, self.display_line.saturating_add(BUFFER_HEIGHT))
            }
            DisplayPosition::Same => {
                (self.display_line, self.display_line.saturating_add(BUFFER_HEIGHT))
            }
            DisplayPosition::End => {
                self.display_scroll_end = true;
                self.display_line = self.lines.len() - 1;
                (self.display_line, self.display_line.saturating_add(BUFFER_HEIGHT))
            }
        };

        // trace!("   initial start {}, end {}", start, end);
        // if we're displaying the end of the VgaBuffer, the range of characters displayed needs to start before that
        let start = if start == (self.lines.len() - 1) {
            start.saturating_sub(BUFFER_HEIGHT - 1)
        } else {
            start
        };
        let end = min(end, self.lines.len());       // ending line must be within the bounds of the buffer (exclusive)
        let num_lines = end - start;
        
        // trace!("   adjusted start {}, end {}, num_lines {}", start, end, num_lines);

        // use volatile memory to ensure the writes happen every time
        use core::ptr::write_volatile;
        unsafe {
            // write the lines that we *can* get from the buffer
            for (i, line) in (start .. end).enumerate() {
                let addr = (VGA_BUFFER_VIRTUAL_ADDR + i * mem::size_of::<Line>()) as *mut Line;
                // trace!("   writing line ({}, {}) at addr {:#X}", i, line, addr as usize);
                write_volatile(addr, self.lines[line]);
            }

            // fill the rest of the space, if any, with blank lines
             if num_lines < BUFFER_HEIGHT {
                for i in num_lines .. BUFFER_HEIGHT {
                    let addr = (VGA_BUFFER_VIRTUAL_ADDR + i * mem::size_of::<Line>()) as *mut Line;
                    // trace!("   writing BLANK ({}) at addr {:#X}", i, addr as usize);
                    write_volatile(addr, BLANK_LINE);
                }
             }
        }


        // // here, we do the actual writing of the VGA memory
        // unsafe {
        //     // copy lines from the our VgaBuffer to the VGA text memory
        //     let dest = slice::from_raw_parts_mut((VGA_BUFFER_VIRTUAL_ADDR as *mut Line), num_lines);
        //     dest.copy_from_slice(&self.lines[start .. end]);
            
        //     // if the buffer is too small, fill in the rest of the lines
        //     if num_lines < BUFFER_HEIGHT {
        //         let start = BUFFER_HEIGHT - num_lines;
        //         for line in start .. BUFFER_HEIGHT {
        //             let dest = slice::from_raw_parts_mut((VGA_BUFFER_VIRTUAL_ADDR + (line * mem::size_of::<Line>())) as *mut ScreenChar, BUFFER_WIDTH); // copy 1 line at a time
        //             dest.copy_from_slice(&BLANK_LINE);
        //         }
        //     }
        // }
    }

}

impl fmt::Write for VgaBuffer {
    fn write_str(&mut self, s: &str) -> ::core::fmt::Result {
        let ret = serial_port::write_str(s); // mirror to serial port
        self.write_str_with_color(s, ColorCode::default());
        ret
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

#[derive(Debug, Clone, Copy)]
pub struct ColorCode(u8);

impl ColorCode {
    pub const fn new(foreground: Color, background: Color) -> ColorCode {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

impl Default for ColorCode {
	fn default() -> ColorCode {
		ColorCode::new(Color::LightGreen, Color::Black)
	}

}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}
impl ScreenChar {
    pub const fn new(ascii: u8, color: ColorCode) -> ScreenChar {
        ScreenChar {
            ascii_character: ascii,
            color_code: color,
        }
    }
}
