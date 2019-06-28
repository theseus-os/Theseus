//! This crate defines text displayable
//! A text displayable profiles a block of text to be displayed on the screen. It specifies the width and height of the text block.
//! This crate also defines a Cursor structure. The structure specifies the size and blink frequency and implement the blink method.
//! The owner of a framebuffer can use a text displayable to display a string in the framebuffer. It can also use a text displayable to display a cursor and let it blink.

#![no_std]

extern crate tsc;
extern crate font;
extern crate frame_buffer;
extern crate frame_buffer_drawer;
extern crate frame_buffer_printer;

extern crate displayable;

use tsc::{tsc_ticks, TscTicks};
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH};
use frame_buffer::{FrameBuffer, Pixel};
use displayable::Displayable;

const DEFAULT_CURSOR_FREQ:u64 = 400000000;

/// A displayable component for text display.
/// The owner of a framebuffer can use this text displayable to display a string. The displayable specifies the size of the text block to be displayable in the framebuffer.
pub struct TextDisplay {
    width: usize,
    height: usize,
}

impl Displayable<&str> for TextDisplay {
    fn display(&self, content: &str, x:usize, y:usize, fg_color:u32, bg_color:u32, framebuffer: &mut FrameBuffer) -> Result<(), &'static str> {
        frame_buffer_printer::print_by_bytes(framebuffer, x, y, self.width, self.height, content, fg_color, bg_color)
    }
}


impl TextDisplay
{
    /// create a new displayable of size (width, height)
    pub fn new(width: usize, height: usize) -> Result <TextDisplay, &'static str> {
        Ok(TextDisplay{
            width: width,
            height: height,
        })
    }

    /// Gets the dimensions of the text area to display
    pub fn get_dimensions(&self) -> (usize, usize){
        (self.width / CHARACTER_WIDTH, self.height / CHARACTER_HEIGHT)
    }

    /// Gets the size of the text area
    pub fn get_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// resize the text displayable area
    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
    }

    /// display a cursor in the text displayable
    pub fn display_cursor(&self, x:usize, y:usize, color:u32, framebuffer:&mut FrameBuffer) {
        frame_buffer_drawer::fill_rectangle(framebuffer, x, y, CHARACTER_WIDTH, CHARACTER_HEIGHT, color);
    }
}

/// A cursor struct. It contains whether it is enabled, 
/// the frequency it blinks, the last time it blinks, and the current blink state show/hidden.
/// A cursor is a special symbol which can be displayed by a text displayable
pub struct Cursor {
    enabled: bool,
    freq: u64,
    time: TscTicks,
    show: bool,
}

impl Cursor {
    /// create a new cursor struct
    pub fn new() -> Cursor {
        Cursor {
            enabled: true,
            freq: DEFAULT_CURSOR_FREQ,
            time: tsc_ticks(),
            show: true,
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

