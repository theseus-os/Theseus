#![no_std]

extern crate input_event_types;

use input_event_types::Event;
use super::super::{FrameTextBuffer,Once};

static TEXT_BUFFER:Once<FrameTextBuffer> = Once::new();


/// Defines the methods that a TextDisplay must have
pub struct TextDisplay {
    x:usize,
    y:usize,
    pub width:usize,
    pub height:usize,
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
    pub fn display_string(&self, x:usize, y:usize, slice: &str) -> Result<(), &'static str>{       
        let text_buffer = TEXT_BUFFER.call_once(|| {
            FrameTextBuffer::new()
        });
        text_buffer.print_by_bytes(x + self.x, y + self.y, 
            self.width, self.height, 
            slice)
    }

    /// Gets the dimensions of the text area to display
    pub fn get_dimensions(&self) -> (usize, usize){
        return (0, 0)

    }
    /// Function to set a cursor on the display at an (x,y) position. 
    /// set_cursor() should accept coordinates within those specified by get_dimensions() and display to window
    pub fn set_cursor(&mut self, line: u16, column: u16, reset:bool){

    } 
    /// Take the cursor off the display
    pub fn disable_cursor(&mut self){

    }
    /// Display the cursor and let it blinks
    pub fn cursor_blink(&mut self){

    }
}

