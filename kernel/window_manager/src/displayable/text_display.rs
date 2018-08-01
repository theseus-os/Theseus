#![no_std]

extern crate event_types;

use event_types::Event;
use super::super::{FrameTextBuffer,Once, CHARACTER_WIDTH, CHARACTER_HEIGHT, WindowObj, frame_buffer};
use core::ops::DerefMut;

/// Defines the methods that a TextDisplay must have
pub struct TextDisplay {
    x:usize,
    y:usize,
    width:usize,
    height:usize,
    textbuffer:FrameTextBuffer,
}

impl TextDisplay
{
    pub fn new(x:usize, y:usize, width:usize, height:usize) -> TextDisplay {
        TextDisplay{
            x:x,
            y:y,
            width:width,
            height:height,
            textbuffer:FrameTextBuffer::new(),
        }
    }

    /// takes in a str slice and display as much as it can to the screen
    pub fn display_string(&self, window:&WindowObj, slice: &str, font_color:u32, bg_color:u32) -> Result<(), &'static str>{       
        let (x, y) = window.get_content_position();
        self.textbuffer.print_by_bytes(x + self.x, y + self.y, 
            self.width, self.height, 
            slice, font_color, bg_color)
    }

    /// Gets the dimensions of the text area to display
    pub fn get_dimensions(&self) -> (usize, usize){
        (self.width/CHARACTER_WIDTH, self.height/CHARACTER_HEIGHT)
    }

    pub fn get_size(&self) -> (usize, usize, usize, usize) {
        (self.x, self.y, self.width, self.height)
    }

    pub fn resize(&mut self, x:usize, y:usize, width:usize, height:usize) {
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height;
    }

    /// Function to set a cursor on the display at an (x,y) position. 
    pub fn set_cursor(&self, window:&WindowObj, line: u16, column: u16, color:u32, reset:bool){
        let mut cursor = self.textbuffer.cursor.lock();
        cursor.enable();
        cursor.update(line as usize, column as usize, reset);
        let (x, y) = window.get_content_position();
        frame_buffer::fill_rectangle(x + self.x + (column as usize) * CHARACTER_WIDTH, 
                        y + self.y + (line as usize) * CHARACTER_HEIGHT, 
                        CHARACTER_WIDTH, CHARACTER_HEIGHT, color);
    } 
    /// Take the cursor off the display
    pub fn disable_cursor(&mut self){
        let mut cursor = self.textbuffer.cursor.lock();
        cursor.disable();
    }
    /// Display the cursor and let it blinks
    pub fn cursor_blink(&self, window:&WindowObj, font_color:u32, bg_color:u32){
        let mut cursor = self.textbuffer.cursor.lock();
        if cursor.blink() {
            let (line, column, show) = cursor.get_info();
            let (x, y) = window.get_content_position();
            let color = if show { font_color } else { bg_color };
            frame_buffer::fill_rectangle(x + self.x + (column as usize) * CHARACTER_WIDTH, 
                        y + self.y + (line as usize) * CHARACTER_HEIGHT, 
                        CHARACTER_WIDTH, CHARACTER_HEIGHT, color);
        }
    }
}

