#![no_std]

extern crate input_event_types;

use input_event_types::Event;
use super::super::{FrameTextBuffer,Once, CHARACTER_WIDTH, CHARACTER_HEIGHT};

static TEXT_BUFFER:Once<FrameTextBuffer> = Once::new();


/// Defines the methods that a TextDisplay must have
pub struct TextDisplay {
    x:usize,
    y:usize,
    width:usize,
    height:usize,
}

impl TextDisplay
{
    pub fn new(x:usize, y:usize, width:usize, height:usize) -> TextDisplay {
        TextDisplay{
            x:x,
            y:y,
            width:width,
            height:height,
        }
    }

    /// takes in a str slice and display as much as it can to the screen
    pub fn display_string(&self, x:usize, y:usize, slice: &str, font_color:u32, bg_color:u32) -> Result<(), &'static str>{       
        let text_buffer = TEXT_BUFFER.call_once(|| {
            FrameTextBuffer::new()
        });
        text_buffer.print_by_bytes(x + self.x, y + self.y, 
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
    pub fn set_cursor(&mut self, line: u16, column: u16, reset:bool){

    } 
    /// Take the cursor off the display
    pub fn disable_cursor(&mut self){

    }
    /// Display the cursor and let it blinks
    pub fn cursor_blink(&mut self){

    }
}

