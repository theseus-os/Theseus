
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

const BLANK_LINE: Line = [ScreenChar::new(' ', 0); BUFFER_WIDTH];
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
    pub display_line: usize,
    /// whether the display is locked to scroll with the end and show new lines as printed
    display_scroll_end: bool,
    /// the column position in the last line where the next character will go
    pub column: usize,
    /// the actual buffer memory that can be written to the frame memory
    lines: Vec<Line>,
}

impl FrameTextBuffer {
    /// Create a new FrameBuffer.
    pub fn new() -> FrameTextBuffer {
        FrameTextBuffer::with_capacity(1000)
    }

    // Create a new FrameBuffer with the given capacity, specified in number of lines. 
    fn with_capacity(num_initial_lines: usize) -> FrameTextBuffer {
        let first_line = BLANK_LINE;
        let mut lines = Vec::with_capacity(num_initial_lines);
        lines.push(first_line);

        FrameTextBuffer {
            display_line: 0,
            display_scroll_end: true,
            column: 0,
            lines: lines,
        }
    }
    

    fn write_str_with_color(&mut self, s: &str, color: usize)-> fmt::Result {
        for byte in s.chars() {
            //trace!("Wenqiu:print chars {}", byte);
            match byte {
                // handle new line
                '\n' => {
                    self.new_line(color);
                }

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

        // refresh the Frame text display if the changes would be visible on screen
        // i.e., if the end of the frame buffer is visible
        let last_line = self.lines.len() - 1;;
        if  self.display_scroll_end || 
            (last_line >= self.display_line && last_line <= (self.display_line + BUFFER_HEIGHT))
        {
            self.display(DisplayPosition::End);
        }
        
        // // refresh the Frame text display if the changes would be visible on screen
        // // keep in mind the latest write to the frame buffer is always written into the last element of self.lines
        // let display_line = self.display_line;
        // let written_line = self.lines.len();
        // if written_line >= display_line && written_line <= display_line + BUFFER_HEIGHT {
        //     // the recent write will be visible
        //     self.display(DisplayPosition::Same);
        // }

        Ok(())

    }

    ///Write string to console with color
    pub fn write_string_with_color(&mut self, s: &String, color: usize)-> fmt::Result {
        self.write_str_with_color(s.as_str(), color)
    }


    /// To create a new line, this function does the following:
    /// 1) Clears the rest of the current line.
    /// 2) Resets the column index to 0 (beginning of next line).
    /// 3) Allocates a new Line and pushes it to the `lines` Vec.
    fn new_line(&mut self, color: usize) {
        // clear out the rest of the current line
        let ref mut lines = self.lines;
        for c in self.column .. BUFFER_WIDTH {
            lines.last_mut().unwrap()[c] = ScreenChar::new(' ', color);
        }
        
        self.column = 0; 
        lines.push([ScreenChar::new(' ', color); BUFFER_WIDTH]);
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
        return
    }

    /// Update the cursor based on the given x and y coordinates,
    /// which correspond to the column and row (line) respectively
    /// Note that the coordinates must correspond to the absolute coordinates the cursor should be 
    /// displayed onto the buffer, not the coordinates relative to the 80x24 grid
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

    /// Disables the cursor 
    /// Still maintains the cursor's position
    pub fn disable_cursor (&self) {
        unsafe {
            CURSOR_PORT_START.lock().write(DISABLE_SEQ_1);
            CURSOR_PORT_END.lock().write(DISABLE_SEQ_2);
        }
    }   

    /// Returns a bool that indicates whether the vga buffer has the ability to scroll 
    /// i.e. if there are more lines than the vga buffer can display at one time
    pub fn can_scroll(&self) -> bool {
        if self.lines.len() > BUFFER_HEIGHT {
            return true
        } else {
            return false
        }
    }

    /// Displays this FrameBuffer at the given string offset by flushing it to the screen.
    pub fn display(&mut self, position: DisplayPosition) {
        //trace!("FrameBuffer::display(): position {:?}", position);
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
        // if we're displaying the end of the FrameBuffer, the range of characters displayed needs to start before that
        let start = if start == (self.lines.len() - 1) {
            start.saturating_sub(BUFFER_HEIGHT - 1)
        } else {
            start
        };
        let end = min(end, self.lines.len());       // ending line must be within the bounds of the buffer (exclusive)
        //let num_lines = end - start;
        
        // trace!("   adjusted start {}, end {}, num_lines {}", start, end, num_lines);

        // use volatile memory to ensure the writes happen every time
        //use core::ptr::write_volatile;
        // write the lines that we *can* get from the buffer
        for (i, line) in (start .. end).enumerate() {
            printline(i, self.lines[line]);
        }

        // fill the rest of the space, if any, with blank lines
        /*    if num_lines < BUFFER_HEIGHT {
            for i in num_lines .. BUFFER_HEIGHT {
                //trace!(self.lines[line]);

                //  write_volatile(addr, BLANK_LINE);
            }
            }
        */

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

impl fmt::Write for FrameTextBuffer {
    fn write_str(&mut self, s: &str) -> ::core::fmt::Result {
        try!(self.write_str_with_color(s, FONT_COLOR));
        serial_port::write_str(s)
    }
}



#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct ScreenChar {
    ascii_character: char,
    color_code: usize,
}

impl ScreenChar {
    const fn new(ascii: char, color: usize) -> ScreenChar {
        ScreenChar {
            ascii_character: ascii,
            color_code: color,
        }
    }
}

fn printline(line_num:usize, line:Line){
    //trace!("printline");
    let mut linebuffer = [[0 as u8; frame_buffer::FRAME_BUFFER_WIDTH]; CHARACTER_HEIGHT];
    
/*    for i in 0..BUFFER_WIDTH{
        parsechar(line[i].ascii_character, line_num, i, line[i].color_code, &mut linebuffer);
    }
*/

     //trace!("printline");
    let mut linebuffer = [[0 as u8; frame_buffer::FRAME_BUFFER_WIDTH]; CHARACTER_HEIGHT];

    let font_color = [0x90 as u8, 0xee as u8, 0x90 as u8];
    //let bg_color = [(BACKGROUND_COLOR as usize & 255) as u8,(BACKGROUND_COLOR as usize & 255) as u8,(BACKGROUND_COLOR as usize & 255) as u8]

unsafe {// TODO
    for y in 0..CHARACTER_HEIGHT {
        for i in 0..BUFFER_WIDTH{
            let character = line[i].ascii_character;
            if character != ' ' {
                //trace!{"WEnqiu:{}", character};

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

fn parsechar(character:char, line:usize, col:usize, color:usize, linebuffer:&mut [[u8;frame_buffer::FRAME_BUFFER_WIDTH]; 16]){
    if col >= BUFFER_WIDTH {
        debug!("frame_buffer_text::print(): The col is out of bound");
        return
    }
    if line >= BUFFER_HEIGHT {
        debug!("frame_buffer_text::print(): The line is out of bound");
        return
    }

    if character == ' ' {
        return
    }
    for y in 0..CHARACTER_HEIGHT{
        trace!("start to draw {}", character as usize);

        let ascii = character as usize;
        let x = (col * CHARACTER_WIDTH) + 1;//leave 1 pixel left margin for every character
        let num = FONT_BASIC[ascii][y];
        for i in 0..8 {
            if num & (0x80 >> i) !=0 {
                //frame_buffer::draw_pixel(x + i, y, color);
                linebuffer[y][(x+i)*3] = (color as usize & 255) as u8;
                linebuffer[y][(x+i)*3+1] = (color as usize >> 8 & 255) as u8; 
                linebuffer[y][(x+i)*3+2] = (color as usize >> 16 & 255) as u8;
            } else {
                //frame_buffer::draw_pixel(x + i, y, BACKGROUND_COLOR);
               /* linebuffer[y][(x+i)*3] = (BACKGROUND_COLOR as usize & 255) as u8;
                linebuffer[y][(x+i)*3+1] = (BACKGROUND_COLOR as usize >> 8 & 255) as u8; 
                linebuffer[y][(x+i)*3+2] = (BACKGROUND_COLOR as usize >> 16 & 255) as u8;
            */}
        }
    }  
}

