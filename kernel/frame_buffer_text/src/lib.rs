
#![no_std]
#![feature(alloc)]
#![feature(const_fn)]
#![feature(unique)]
#![feature(asm)]

extern crate frame_buffer;
#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate serial_port;
extern crate spin;
extern crate port_io;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::cmp::min;
use spin::Mutex;
use port_io::Port;
use core::mem;



use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH, FONT_BASIC};

const BUFFER_WIDTH:usize = (frame_buffer::FRAME_BUFFER_WIDTH/3)/CHARACTER_WIDTH;
const BUFFER_HEIGHT:usize = frame_buffer::FRAME_BUFFER_HEIGHT/CHARACTER_HEIGHT;

pub const FONT_COLOR:usize = 0x90ee90;
const BACKGROUND_COLOR:usize = 0x000000;

pub mod font;




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


type Line = [ScreenChar; BUFFER_WIDTH];

const BLANK_LINE: Line = [ScreenChar::new(b' ', 0); BUFFER_WIDTH];
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

pub fn enable_cursor() {
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

/// An instance of a frame text buffer which can be displayed to the screen.
pub struct FrameTextBuffer {
    /// the index of the line that is currently being displayed
    //pub display_line: usize,
    /// whether the display is locked to scroll with the end and show new lines as printed
    //display_scroll_end: bool,
    /// the column position in the last line where the next character will go
    //pub column: usize,
    /// the actual buffer memory that can be written to the frame memory
    display_lines: Vec<Line>,
}

impl FrameTextBuffer {
    /// Create a new FrameBuffer.
    pub fn new() -> FrameTextBuffer {
        FrameTextBuffer::with_capacity(1000)
    }

    /// Create a new VgaBuffer with the given initial capacity, specified in number of lines.
    pub fn with_capacity(num_initial_lines: usize) -> FrameTextBuffer {
        let first_line = BLANK_LINE;
        let mut lines = Vec::with_capacity(num_initial_lines);
        lines.push(first_line);
        let display_lines = Vec::with_capacity(num_initial_lines);
        FrameTextBuffer {
            display_lines: display_lines,
        }
    }


 /// Enables the cursor by writing to four ports
    pub fn enable_cursor(&self) {
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
        return;
    }


    pub fn update_cursor(&self, x: u16, y: u16) {
        let pos: u16 = y * BUFFER_WIDTH as u16 + x;
        unsafe {
            CURSOR_PORT_START.lock().write(UPDATE_SEQ_2);
            CURSOR_PORT_END.lock().write((pos & UPDATE_SEQ_3) as u8);
            CURSOR_PORT_START.lock().write(UPDATE_SEQ_1);
            CURSOR_PORT_END
                .lock()
                .write(((pos >> RIGHT_BIT_SHIFT) & UPDATE_SEQ_3) as u8);
        }
        return;
    }


    /// Disables the cursor
    /// Still maintains the cursor's position
    pub fn disable_cursor(&self) {
        unsafe {
            CURSOR_PORT_START.lock().write(DISABLE_SEQ_1);
            CURSOR_PORT_END.lock().write(DISABLE_SEQ_2);
        }
    }

    /// Returns a tuple containing (buffer height, buffer width)
    pub fn get_dimensions(&self) -> (usize, usize) {
        (BUFFER_WIDTH, BUFFER_HEIGHT)
    }

    /// Requires that a str slice that will exactly fit the vga buffer
    /// The calculation is done inside the console crate by the print_to_vga function and associated methods
    /// Parses the string into line objects and then prints them onto the vga buffer
    pub fn display_string(&mut self, slice: &str) -> Result<usize, &'static str> {
        let mut curr_column = 0;
        let mut new_line = BLANK_LINE;
        let mut cursor_pos = 0;
        // iterates through the string slice and puts it into lines that will fit on the vga buffer
        for byte in slice.bytes() {
            if byte == b'\n' {
                self.display_lines.push(new_line);
                new_line = BLANK_LINE;
                cursor_pos += BUFFER_WIDTH - curr_column;
                curr_column = 0;
            } else {
                if curr_column == BUFFER_WIDTH {
                    curr_column = 0;
                    self.display_lines.push(new_line);
                    new_line = BLANK_LINE;
                }
                new_line[curr_column] = ScreenChar::new(byte, FONT_COLOR as usize);
                curr_column += 1;
                cursor_pos += 1;
            }
        }
        self.display_lines.push(new_line);

        let iterator = self.display_lines.len();
        // Writes the lines to the vga buffer
        unsafe {
            use core::ptr::write_volatile;
            //for (i, line) in (0..iterator).enumerate() {
                //let addr = (VGA_BUFFER_VIRTUAL_ADDR + i * mem::size_of::<Line>()) as *mut Line;
                //write_volatile(addr, self.display_lines[line]);
            for i in 0..iterator {
                self.printline(i, self.display_lines[i]);
            }

            // fill the rest of the space, if any, with blank lines
            if iterator < BUFFER_HEIGHT {
                for i in iterator..BUFFER_HEIGHT {
                    //let addr = (VGA_BUFFER_VIRTUAL_ADDR + i * mem::size_of::<Line>()) as *mut Line;
                    // trace!("   writing BLANK ({}) at addr {:#X}", i, addr as usize);
                    //write_volatile(addr, BLANK_LINE);
                    self.printline(i, BLANK_LINE);
                }
            }
        }
        self.display_lines = Vec::with_capacity(1000);
        Ok(cursor_pos)
    }  


    fn printline(&self, line_num:usize, line:Line){
        
        let mut linebuffer = [[0 as u8; frame_buffer::FRAME_BUFFER_WIDTH]; CHARACTER_HEIGHT];

        let font_color = parsecolor(FONT_COLOR);

        unsafe {// TODO
            for y in 0..CHARACTER_HEIGHT {
                for i in 0..BUFFER_WIDTH{
                    let character = line[i].ascii_character;
                    if character != b' ' {
                        let ascii_code = line[i].ascii_character as usize;
                        for x in 0..8 {
                            if (font::FONT_PIXEL[ascii_code][y][x])!= 0 {
                                linebuffer[y][(i*CHARACTER_WIDTH+x+1)*3..(i*CHARACTER_WIDTH+x+1)*3+3].clone_from_slice(&font_color);                    
                            }
                        }
                    }
                }
            }
        }
        frame_buffer::display(line_num * CHARACTER_HEIGHT, CHARACTER_HEIGHT, &linebuffer);
    
    }



}



#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct ScreenChar {
    ascii_character: u8,
    color_code: usize,
}

impl ScreenChar {
    const fn new(ascii: u8, color: usize) -> ScreenChar {
        ScreenChar {
            ascii_character: ascii,
            color_code: color,
        }
    }
}

fn parsecolor(color:usize) -> [u8;3] {
    let red = (color >> 16) as u8;
    let green = ((color >> 8) & 0xff) as u8;
    let blue = (color & 0xff) as u8;
    [red, green, blue]
}