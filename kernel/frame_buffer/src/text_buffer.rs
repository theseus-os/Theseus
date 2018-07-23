extern crate tsc;
extern crate text_display;

// andrew: fix
extern crate input_event_types;
// use input_event_types::Event;
use text_buffer::input_event_types::Event;

use super::font::{CHARACTER_HEIGHT, CHARACTER_WIDTH, FONT_PIXEL};
use super::{Mutex, Buffer, FRAME_BUFFER_WIDTH, FRAME_BUFFER_HEIGHT, FRAME_DRAWER, fill_rectangle};

use self::tsc::{tsc_ticks, TscTicks};
use self::text_display::TextDisplay;


const BUFFER_WIDTH:usize = FRAME_BUFFER_WIDTH / CHARACTER_WIDTH;
const BUFFER_HEIGHT:usize = FRAME_BUFFER_HEIGHT / CHARACTER_HEIGHT;

pub const FONT_COLOR:u32 = 0x93ee90;
pub const BACKGROUND_COLOR:u32 = 0x000000;

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
/// An instance of a VGA text buffer which can be displayed to the screen.
pub struct FrameTextBuffer {
    pub cursor:Cursor,
}

impl FrameTextBuffer {
    pub fn new() -> FrameTextBuffer {
        FrameTextBuffer {
            cursor:Cursor::new(0, 0, true),
        }
    }

    ///print a string by bytes
    pub fn print_by_bytes(&self, x:usize, y:usize, width:usize, height:usize, slice: &str) -> Result<(), &'static str> {
        let mut curr_line = 0;
        let mut curr_column = 0;
        let mut cursor_pos = 0;

        let buffer_width = width/CHARACTER_WIDTH;
        let buffer_height = height/CHARACTER_HEIGHT;
        
        let mut drawer = FRAME_DRAWER.lock();
        let buffer = drawer.buffer();
        for byte in slice.bytes() {
            if byte == b'\n' {
                self.fill_blank (buffer, 
                    x + curr_column * CHARACTER_WIDTH,
                    y + curr_line * CHARACTER_HEIGHT,
                    x + width, 
                    y + (curr_line + 1 )* CHARACTER_HEIGHT, 
                    BACKGROUND_COLOR);
                cursor_pos += buffer_width - curr_column;
                curr_column = 0;
                curr_line += 1;
            } else {
                if curr_column == buffer_width {
                    curr_column = 0;
                    curr_line += 1;
                    if curr_line == buffer_height {
                        break;
                    }
                }
                self.print_byte(buffer, byte, FONT_COLOR, x, y, curr_line, curr_column);
                curr_column += 1;
                cursor_pos += 1;
            }
        }
        self.fill_blank (buffer, 
            x, y + (curr_line + 1 )* CHARACTER_HEIGHT, x + width, y + height, 
            BACKGROUND_COLOR);

        Ok(())
    }

    fn print_byte (&self, buffer:&mut Buffer, byte:u8, color:u32, left:usize, top:usize, line:usize, column:usize) {
        let x = left + column * CHARACTER_WIDTH;
        let y = top + line * CHARACTER_HEIGHT;
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

    fn fill_blank(&self, buffer:&mut Buffer, left:usize, top:usize, right:usize, bottom:usize, color:u32){
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
            buffer.chars[y][x] = color;
            x += 1;
        }
    }
    
}


/// Implements TextDisplay trait for vga buffer.
/// set_cursor() should accept coordinates within those specified by get_dimensions() and display to window
impl TextDisplay for FrameTextBuffer {

    fn disable_cursor(&mut self) {
        self.cursor.disable();
        fill_rectangle(self.cursor.column * CHARACTER_WIDTH, self.cursor.line * CHARACTER_HEIGHT,  
                    CHARACTER_WIDTH, CHARACTER_HEIGHT, BACKGROUND_COLOR);

    }

    fn set_cursor(&mut self, line:u16, column:u16, reset:bool) {
        self.cursor.enabled = true;
        self.cursor.update(line as usize, column as usize, reset);
        fill_rectangle(self.cursor.column * CHARACTER_WIDTH, self.cursor.line * CHARACTER_HEIGHT, 
                    CHARACTER_WIDTH, CHARACTER_HEIGHT, FONT_COLOR); 
    }

    fn cursor_blink(&mut self) {
        if self.cursor.blink() {
            let color = if self.cursor.show { FONT_COLOR } else { BACKGROUND_COLOR };
            fill_rectangle(self.cursor.column * CHARACTER_WIDTH, self.cursor.line * CHARACTER_HEIGHT, 
                    CHARACTER_WIDTH, CHARACTER_HEIGHT, color); 
        }

    }

    /// Returns a tuple containing (buffer height, buffer width)
    fn get_dimensions(&self) -> (usize, usize) {
        (BUFFER_WIDTH, BUFFER_HEIGHT)
    }

    /// Requires that a str slice that will exactly fit the frame buffer
    /// The calculation is done inside the console crate by the print_by_bytes function and associated methods
    /// Print every byte and fill the blank with background color
    fn display_string(&mut self, slice: &str) -> Result<(), &'static str> {
        self.print_by_bytes (0, 0, FRAME_BUFFER_WIDTH, FRAME_BUFFER_HEIGHT, slice)     
    }

    fn draw_border(&self) -> (usize, usize, usize) {
        (0, 0, 0)
    }


    fn get_key_event(&self) -> Option<Event> {
        // Andrew: fix instead of using as trait
        None
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
            freq:500000000,
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
            if time.sub(&(self.time)).unwrap().to_ns().unwrap() >= self.freq {
                self.time = time;
                self.show = !self.show;
                return true
            }
        }
        false
    }

    pub fn get_info(&self) -> (usize, usize, bool) {
        (self.line, self.column, self.show)
    }
}



