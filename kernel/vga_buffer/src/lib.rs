//! The vga buffer that implements basic printing in VGA text mode.

#![no_std]
#![feature(alloc)]
#![feature(const_fn)]
#![feature(unique)]
#![feature(ptr_internals)]

// #[macro_use] extern crate log;
extern crate spin;
extern crate volatile;
extern crate alloc;
extern crate serial_port;
extern crate kernel_config;
extern crate port_io;
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
use port_io::Port;

/// defined by x86's physical memory maps
const VGA_BUFFER_VIRTUAL_ADDR: usize = 0xb8000 + KERNEL_OFFSET;




/// height of the VGA text window
const BUFFER_HEIGHT: usize = 25;
/// width of the VGA text window
const BUFFER_WIDTH: usize = 80;


// #[macro_export] pub mod raw;
pub mod raw;


/// Specifies where we want to scroll the VGA display, and by how much.
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
    static CURSOR_PORT_START: Mutex<Port<u8>> = Mutex::new( Port::new(0x3D4) );
    static CURSOR_PORT_END: Mutex<Port<u8>> = Mutex::new( Port::new(0x3D5) );
    static AUXILLARY_ADDR: Mutex<Port<u8>> = Mutex::new( Port::new(0x3E0) );

const UNLOCK_SEQ_1:u8  = 0x0A;
const UNLOCK_SEQ_2:u8 = 0x0B;
const UNLOCK_SEQ_3:u8 = 0xC0;
const UNLOCK_SEQ_4:u8 = 0xE0;
const UPDATE_SEQ_1: u8 = 0x0E;
const UPDATE_SEQ_2: u8 = 0x0F;
const UPDATE_SEQ_3: u16 = 0xFF;
const CURSOR_START:u8 =  0b00000001;
const CURSOR_END:u8 = 0b00010000;
const RIGHT_BIT_SHIFT: u8 = 8;
const DISABLE_SEQ_1: u8 = 0x0A;
const DISABLE_SEQ_2: u8 = 0x20;



pub fn init_cursor() {
    unsafe {
        CURSOR_PORT_START.lock().write(UNLOCK_SEQ_1);
        let temp_read: u8 = (CURSOR_PORT_END.lock().read() & UNLOCK_SEQ_3) | CURSOR_START;
        CURSOR_PORT_END.lock().write(temp_read);
        CURSOR_PORT_START.lock().write(UNLOCK_SEQ_2);
        let temp_read2 = (AUXILLARY_ADDR.lock().read() & UNLOCK_SEQ_4) | CURSOR_END;
        CURSOR_PORT_END.lock().write(temp_read2);
    }
}

pub fn update_cursor (x: u16, y:u16) { 
    let pos: u16 =  y*BUFFER_WIDTH as u16  + x;
    unsafe {
        CURSOR_PORT_START.lock().write(UPDATE_SEQ_2);
        CURSOR_PORT_END.lock().write((pos & UPDATE_SEQ_3) as u8);
        CURSOR_PORT_START.lock().write(UPDATE_SEQ_1);
        CURSOR_PORT_END.lock().write(((pos>>RIGHT_BIT_SHIFT) & UPDATE_SEQ_3) as u8);
    }
}

pub fn disable_cursor () {
    unsafe {
        CURSOR_PORT_START.lock().write(DISABLE_SEQ_1);
        CURSOR_PORT_END.lock().write(DISABLE_SEQ_2);
    }
}

/// An instance of a VGA text buffer which can be displayed to the screen.
pub struct VgaBuffer {
    /// the index of the line that is currently being displayed
    pub display_line: usize,
    /// whether the display is locked to scroll with the end and show new lines as printed
    pub display_scroll_end: bool,
    /// the column position in the last line where the next character will go
    pub column: usize,
    /// the actual buffer memory that can be written to the VGA memory
    lines: Vec<Line>,
}

impl VgaBuffer {
    /// Create a new VgaBuffer.
    pub fn new() -> VgaBuffer {
        VgaBuffer::with_capacity(1000)
    }

    /// Create a new VgaBuffer with the given initial capacity, specified in number of lines. 
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


    /// Writes the given string with the given color to this `VgaBuffer`.
    pub fn write_str_with_color(&mut self, s: &str, color: ColorCode) -> fmt::Result {
        let last_line = self.lines.len() - 1;
        for byte in s.bytes() {
            match byte {
                // handle new line
                b'\n' => {
                    try!(self.new_line(color));
                }

                // handle backspace
                0x08 => { 
                    if self.column != 0 {
                        // goes back one column and writes a blank space over the deleted character
                        self.column -= 1;
                        let mut curr_line = try!(self.lines.last_mut().ok_or(fmt::Error));
                        curr_line[self.column] = ScreenChar::new(b' ', color);
                        
                    }
                    else {
                        // covers the case whenever the user backspaces after the cursor wraps to the next line
                        self.column = BUFFER_WIDTH-1;
                        self.lines.pop();
                        let mut curr_line = try!(self.lines.last_mut().ok_or(fmt::Error));
                        curr_line[self.column] = ScreenChar::new(b' ', color);
                    }
                }

                // all other regular bytes
                byte => {
                    {
                        let mut curr_line = try!(self.lines.last_mut().ok_or(fmt::Error));
                        curr_line[self.column] = ScreenChar::new(byte, color);
                    }
                    self.column += 1;

                    if self.column == BUFFER_WIDTH { // wrap to a new line
                        try!(self.new_line(color)); 
                    }
                }
            }
        }

        // refresh the VGA text display if the changes would be visible on screen
        // i.e., if the end of the vga buffer is visible
        
        if  self.display_scroll_end || 
            (last_line >= self.display_line && last_line <= (self.display_line + BUFFER_HEIGHT))
        {
            self.display(DisplayPosition::End);
        }

        Ok(())
    }


    /// Writes the given string with the given color to this `VgaBuffer`.    
    pub fn write_string_with_color(&mut self, s: &String, color: ColorCode) -> fmt::Result {
        self.write_str_with_color(s.as_str(), color)
    }


    /// Writes the given formatting args to this `VgaBuffer` with the default color scheme.
    #[allow(dead_code)]
    fn write_args(&mut self, args: fmt::Arguments) -> fmt::Result {
        use fmt::Write;
        self.write_fmt(args)
    }

    /// Writes a new line to the VgaBuffer, which does the following:
    /// 
    /// 1. Clears the rest of the current line.
    /// 2. Resets the column index to 0 (beginning of next line).
    /// 3. Allocates a new Line and pushes it to the `lines` Vec.
    fn new_line(&mut self, color: ColorCode) -> fmt::Result {
        // clear out the rest of the current line
        let ref mut lines = self.lines;
        for c in self.column .. BUFFER_WIDTH {
            try!(lines.last_mut().ok_or(fmt::Error))[c] = ScreenChar::new(b' ', color);
        }
        
        self.column = 0; 
        lines.push([ScreenChar::new(b' ', color); BUFFER_WIDTH]);

        Ok(())
    }

    pub fn init_cursor(&self) {
        unsafe {
            let cursor_start = 0b00000001;
            let cursor_end = 0b00010000;
            CURSOR_PORT_START.lock().write(UNLOCK_SEQ_1);
            let temp_read: u8 = (CURSOR_PORT_END.lock().read() & UNLOCK_SEQ_3) | cursor_start;
            CURSOR_PORT_END.lock().write(temp_read);
            CURSOR_PORT_START.lock().write(UNLOCK_SEQ_2);
            let temp_read2 = (AUXILLARY_ADDR.lock().read() & UNLOCK_SEQ_4) | cursor_end;
            CURSOR_PORT_END.lock().write(temp_read2);
        }
        return
    }

    pub fn update_cursor(&self, x: u16, y:u16) { 
        let pos: u16 =  y*BUFFER_WIDTH as u16  + x;
        unsafe {
            CURSOR_PORT_START.lock().write(UPDATE_SEQ_2);
            CURSOR_PORT_END.lock().write((pos & UPDATE_SEQ_3) as u8);
            CURSOR_PORT_START.lock().write(UPDATE_SEQ_1);
            CURSOR_PORT_END.lock().write(((pos>>RIGHT_BIT_SHIFT) & UPDATE_SEQ_3) as u8);
        }
        return
    }

    pub fn disable_cursor (&self) {
        unsafe {
            CURSOR_PORT_START.lock().write(DISABLE_SEQ_1);
            CURSOR_PORT_END.lock().write(DISABLE_SEQ_2);
        }
    }   





    /// Displays (refreshes) this VgaBuffer at the given position.
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
    }

}

impl fmt::Write for VgaBuffer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        try!(self.write_str_with_color(s, ColorCode::default()));
        serial_port::write_str(s) // mirror to serial port
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
