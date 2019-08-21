#![no_std]

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
            height:height
        }
    }

    /// takes in a str slice and display as much as it can to the screen
    pub fn display_string(&self, _slice: &str) -> Result<(), &'static str>{
        Ok(())
    }

    /// Function to set a cursor on the display at an (x,y) position. 
    /// set_cursor() should accept coordinates within those specified by get_dimensions() and display to window
    pub fn set_cursor(&mut self, _line: u16, _column: u16, _reset: bool){

    } 
    /// Take the cursor off the display
    pub fn disable_cursor(&mut self){

    }
    /// Display the cursor and let it blinks
    pub fn cursor_blink(&mut self){

    }

    pub fn get_size(&self) -> (usize, usize, usize, usize){
        (self.x, self.y, self.width, self.height)
    }
}

