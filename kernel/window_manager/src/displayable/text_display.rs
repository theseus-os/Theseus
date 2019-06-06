use super::super::{tsc_ticks, TscTicks, CHARACTER_WIDTH, CHARACTER_HEIGHT};
const DEFAULT_CURSOR_FREQ:u64 = 400000000;

/// A displayable component for text display
pub struct TextDisplay {
    width:usize,
    height:usize,
}

impl TextDisplay
{
    /// create a new displayable of size (width, height)
    pub fn new(width:usize, height:usize) -> Result <TextDisplay, &'static str> {
        Ok(TextDisplay{
            width:width,
            height:height,
        })
    }

    // /// takes in a str slice and display as much as it can to the text area
    // pub fn display_string(&self, buffer:&mut FrameBuffer, slice:&str, x:usize, y:usize, font_color:u32, bg_color:u32) -> Result<(), &'static str>{       
    //     buffer.print_by_bytes(x, y, self.width, self.height,
    //         slice, font_color, bg_color)
    // }

    /// Gets the dimensions of the text area to display
    pub fn get_dimensions(&self) -> (usize, usize){
        (self.width / CHARACTER_WIDTH, self.height / CHARACTER_HEIGHT)
    }

    /// Gets the size of the text area
    pub fn get_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// resize the text displayable area
    pub fn resize(&mut self, width:usize, height:usize) {
        self.width = width;
        self.height = height;
    }
}

/// A cursor struct. It contains whether it is enabled, 
/// the frequency it blinks, the last time it blinks, and the current blink state show/hidden
pub struct Cursor {
    enabled:bool,
    freq:u64,
    time:TscTicks,
    show:bool,
}

impl Cursor {
    /// create a new cursor struct
    pub fn new() -> Cursor {
        Cursor {
            enabled:true,
            freq:DEFAULT_CURSOR_FREQ,
            time:tsc_ticks(),
            show:true,
        }
    }

    /// reset the cursor
    pub fn reset(&mut self) {
        self.show = true;
        self.time = tsc_ticks();
    }

    /// enable a cursor
    pub fn enable(&mut self) {
        self.enabled = true;
        self.reset();
    }

    /// disable a cursor
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// change the blink state show/hidden of a cursor. The terminal calls this function in a loop
    pub fn blink(&mut self) -> bool{
        if self.enabled {
            let time = tsc_ticks();
            if let Some(duration) = time.sub(&(self.time)) {
                if let Some(ns) = duration.to_ns() {
                    if ns >= self.freq {
                        self.time = time;
                        self.show = !self.show;
                        return true;
                    }
                }
            }
        }
        false
    }

    /// check if the cursor should be displayed
    pub fn show(&self) -> bool {
        self.enabled && self.show
    }
}

