extern crate tsc;

use super::font::{CHARACTER_HEIGHT, CHARACTER_WIDTH, FONT_PIXEL};
use super::{Buffer, FRAME_BUFFER_WIDTH, FRAME_BUFFER_HEIGHT, FRAME_DRAWER, fill_rectangle};

use self::tsc::{tsc_ticks, TscTicks};

const BUFFER_WIDTH:usize = FRAME_BUFFER_WIDTH / CHARACTER_WIDTH;
const BUFFER_HEIGHT:usize = FRAME_BUFFER_HEIGHT / CHARACTER_HEIGHT;

pub const FONT_COLOR:u32 = 0x93ee90;
const BACKGROUND_COLOR:u32 = 0x000000;

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
    //display_lines: Vec<Line>,
    //The cursor of the terminal. Contaning the position and enabled flag.
    //cursor: Cursor,
}

impl FrameTextBuffer {
    /// Create a new FrameBuffer. 
    pub fn new() -> FrameTextBuffer {
        FrameTextBuffer{}
    }

    /*// Enables the cursor by writing to four ports
    pub fn enable_cursor(&self) {
        /*unsafe {
            let cursor_start = 0b00000001;
            let cursor_end = 0b00010000;
            CURSOR_PORT_START.lock().write(UNLOCK_SEQ_1);
            let temp_read: u8 = (CURSOR_PORT_END.lock().read() & UNLOCK_SEQ_3) | cursor_start;
            CURSOR_PORT_END.lock().write(temp_read);
            CURSOR_PORT_START.lock().write(UNLOCK_SEQ_2);
            let temp_read2 = (AUXILLARY_ADDR.lock().read() & UNLOCK_SEQ_4) | cursor_end;
            CURSOR_PORT_END.lock().write(temp_read2);
        }
        return;*/
    }

    /// Update the cursor
    pub fn update_cursor(&self, x: u16, y: u16) {
        /*let pos: u16 = y * BUFFER_WIDTH as u16 + x;
        unsafe {
            CURSOR_PORT_START.lock().write(UPDATE_SEQ_2);
            CURSOR_PORT_END.lock().write((pos & UPDATE_SEQ_3) as u8);
            CURSOR_PORT_START.lock().write(UPDATE_SEQ_1);
            CURSOR_PORT_END
                .lock()
                .write(((pos >> RIGHT_BIT_SHIFT) & UPDATE_SEQ_3) as u8);
        }
        return;*/
    }


    /// Disables the cursor
    /// Still maintains the cursor's position
    pub fn disable_cursor(&self) {
        /*unsafe {
            CURSOR_PORT_START.lock().write(DISABLE_SEQ_1);
            CURSOR_PORT_END.lock().write(DISABLE_SEQ_2);
        }*/
    }*/

    /// Returns a tuple containing (buffer height, buffer width)
    pub fn get_dimensions(&self) -> (usize, usize) {
        (BUFFER_WIDTH, BUFFER_HEIGHT)
    }

    /// Requires that a str slice that will exactly fit the frame buffer
    /// The calculation is done inside the console crate by the print_by_bytes function and associated methods
    /// Print every byte and fill the blank with background color
    pub fn display_string(&self, slice: &str) -> Result<usize, &'static str> {
        self.print_by_bytes (slice)     
    }
    
    ///print a string by lines. This is no longer used
    /*fn print_by_lines (&self, slice: &str) -> Result<usize, &'static str> {
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

    }*/

    ///print a string by bytes
    fn print_by_bytes(&self, slice: &str) -> Result<usize, &'static str> {
        let mut curr_line = 0;
        let mut curr_column = 0;
        let mut cursor_pos = 0;
        
        let mut drawer = FRAME_DRAWER.lock();
        let buffer = drawer.buffer();
        for byte in slice.bytes() {
            if byte == b'\n' {
                let bottom = (curr_line + 1) * CHARACTER_HEIGHT;
                self.fill_blank (buffer, curr_line, curr_column, bottom, BACKGROUND_COLOR);
                cursor_pos += BUFFER_WIDTH - curr_column;
                curr_column = 0;
                curr_line += 1;
            } else {
                if curr_column == BUFFER_WIDTH {
                    curr_column = 0;
                    curr_line += 1;
                }
                self.print_byte(buffer, byte, FONT_COLOR, curr_line, curr_column);
                curr_column += 1;
                cursor_pos += 1;
            }
        }
        self.fill_blank (buffer, curr_line + 1, 0, FRAME_BUFFER_HEIGHT, BACKGROUND_COLOR);

        Ok(cursor_pos)
    }

    fn print_byte (&self, buffer:&mut Buffer, byte:u8, color:u32, line:usize, column:usize) {
        let x = column * CHARACTER_WIDTH;
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

    fn fill_blank(&self, buffer:&mut Buffer, line:usize, column:usize, bottom:usize, color:u32){
        let mut x = column * CHARACTER_WIDTH;
        let mut y = line * CHARACTER_HEIGHT;
        loop {
            if x == FRAME_BUFFER_WIDTH {
                y += 1;
                x = column * CHARACTER_WIDTH;
            }
            if y == bottom {
                break;
            }
            buffer.chars[y][x] = color;
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
            freq:500000000,
            time:tsc_ticks(),
            show:true,
        }
    }

    ///update the cursor position
    pub fn update(&mut self, line:usize, column:usize) {
        self.line = line;
        self.column = column;
        self.show = true;
    }

    ///enable a cursor
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    ///disable a cursor
    pub fn disable(&mut self) {
        self.enabled = false;
        //fill_rectangle(column * CHARACTER_WIDTH, line * CHARACTER_HEIGHT, CHARACTER_WIDTH, CHARACTER_HEIGHT, BACKGROUND_COLOR);
    }

    ///change the blink state show/hidden of a cursor. The terminal calls this function in a loop
    pub fn display(&mut self) {
        let time = tsc_ticks();
        
        if time.sub(&(self.time)).unwrap().to_ns().unwrap() >= self.freq {
            self.time = time;
            self.show = !self.show;
        }

        if self.enabled  {
            if self.show {
                fill_rectangle(self.column * CHARACTER_WIDTH, self.line * CHARACTER_HEIGHT, 
                    CHARACTER_WIDTH, CHARACTER_HEIGHT, FONT_COLOR);    
            } else {
                fill_rectangle(self.column * CHARACTER_WIDTH, self.line * CHARACTER_HEIGHT, 
                    CHARACTER_WIDTH, CHARACTER_HEIGHT, BACKGROUND_COLOR);
            }
        }
    }
}