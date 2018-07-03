#![no_std]
#![feature(alloc)]
#![feature(const_fn)]
#![feature(unique)]
#![feature(asm)]

extern crate port_io;

use super::font::{CHARACTER_HEIGHT, CHARACTER_WIDTH, FONT_PIXEL};
use super::{Buffer, FRAME_BUFFER_WIDTH, FRAME_BUFFER_HEIGHT, FRAME_DRAWER};
use super::{Vec, Mutex};

use self::port_io::Port;

const BUFFER_WIDTH:usize = FRAME_BUFFER_WIDTH/CHARACTER_WIDTH;
const BUFFER_HEIGHT:usize = FRAME_BUFFER_HEIGHT/CHARACTER_HEIGHT;

pub const FONT_COLOR:u32 = 0x93ee90;
const BACKGROUND_COLOR:u32 = 0x000000;

static mut buf:[u32;640*400] = [0;640*400];


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


type Line = [u8; BUFFER_WIDTH];

const BLANK_LINE: Line = [b' '; BUFFER_WIDTH];
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
    pub fn display_string(&self, slice: &str) -> Result<usize, &'static str> {
        self.print_by_bytes (slice)       
    }
    
    ///print a string by lines
    fn print_by_lines (&self, slice: &str) -> Result<usize, &'static str> {
        let mut curr_line = 0;
        let mut curr_column = 0;
        let mut cursor_pos = 0;
        let mut new_line = BLANK_LINE;
        
        let mut drawer = FRAME_DRAWER.lock();
        let mut buffer = drawer.buffer();

        for byte in slice.bytes() {

            if byte == b'\n' {
                self.print_line(buffer, curr_line, new_line, FONT_COLOR, BACKGROUND_COLOR);
                new_line = BLANK_LINE;
                cursor_pos += BUFFER_WIDTH - curr_column;
                curr_column = 0;
                curr_line += 1;
            } else {
                if curr_column == BUFFER_WIDTH {
                    curr_column = 0;
                    self.print_line(buffer, curr_line, new_line, FONT_COLOR, BACKGROUND_COLOR);
                    new_line = BLANK_LINE;
                    curr_line += 1;
                }
                new_line[curr_column] = byte;
                curr_column += 1;
                cursor_pos += 1;
            }
 
        }
        self.print_line(buffer, curr_line, new_line, FONT_COLOR, BACKGROUND_COLOR);

        /*loop {
            curr_line += 1;
            if curr_line == BUFFER_HEIGHT {
                break;
            }
            self.print_line(buffer, curr_line, BLANK_LINE, FONT_COLOR, BACKGROUND_COLOR);
        }*/

        Ok(cursor_pos)
    }

    fn print_line(&self, buffer:&mut Buffer, curr_line: usize, line:Line, fg_color:u32 , bg_color: u32) {
        let mut x = 0;
        let mut y = curr_line * CHARACTER_HEIGHT;
        let mut i = 0;
        let mut j = 0;

        let mut index = 0;
        let mut byte = line[index] as usize;

        let fonts = FONT_PIXEL.lock();
        loop {
            let mask = fonts[byte][j][i];            
            buffer.chars[y][x + i] = fg_color & mask | bg_color & (!mask);
            i += 1;
            if i == CHARACTER_WIDTH {
                index += 1;
                x += CHARACTER_WIDTH;
                if index == BUFFER_WIDTH {
                    index = 0;
                    j += 1;
                    if j == CHARACTER_HEIGHT {
                        return
                    }
                    y += 1;
                    x = 0;
                }
                i = 0;
                byte = line[index] as usize;
            }

        }

    }

    ///prints a string by bytes
    fn print_by_bytes(&self, slice: &str) -> Result<usize, &'static str> {
        let mut curr_line = 0;
        let mut curr_column = 0;
        let mut cursor_pos = 0;
        
        let mut drawer = FRAME_DRAWER.lock();
        let mut buffer = drawer.buffer();
        for byte in slice.bytes() {
            if byte == b'\n' {
                loop {
                    if curr_column == BUFFER_WIDTH {
                        break;
                    }
                    self.print_byte(buffer, b' ', FONT_COLOR, curr_column, curr_line);
                    curr_column += 1;
                }
                cursor_pos += BUFFER_WIDTH - curr_column;
                curr_column = 0;
                curr_line += 1;
            } else {
                if curr_column == BUFFER_WIDTH {
                    curr_column = 0;
                    curr_line += 1;
                }
                self.print_byte(buffer, byte, FONT_COLOR, curr_column, curr_line);
                curr_column += 1;
                cursor_pos += 1;
            }
        }
        Ok(cursor_pos)
    }

    fn print_byte (&self, buffer:&mut Buffer, byte:u8, color:u32, column:usize, line:usize) {
        let x = column * CHARACTER_WIDTH + 1;
        let y = line * CHARACTER_HEIGHT;
        let mut i = 0;
        let mut j = 0;

        let fonts = FONT_PIXEL.lock();
   
        loop {
            let mask:u32 = fonts[byte as usize][i][j];
            buffer.chars[i + y][j + x] = color & mask | BACKGROUND_COLOR & (!mask);
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

}