//! This crate defines text displayable.
//! A text displayable profiles a block of text to be displayed in a framebuffer.
//!
//! This crate also defines a cursor structure. A cursor is a special symbol which can be displayed within a text displayable. The structure specifies the size and blink frequency and implement the blink method for a cursor.

#![no_std]

extern crate alloc;
extern crate font;
extern crate frame_buffer;
extern crate frame_buffer_drawer;
extern crate frame_buffer_printer;
extern crate tsc;

extern crate displayable;

use alloc::string::String;
use displayable::Displayable;
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH};
use frame_buffer::FrameBuffer;
use tsc::{tsc_ticks, TscTicks};

const DEFAULT_CURSOR_FREQ: u64 = 400000000;

/// A text displayable profiles the size and color of a block of text. It can display in a framebuffer.
pub struct TextDisplay {
    width: usize,
    height: usize,
    // the position of the next symbol
    // the position is updated after display and they will be useful for optimization.
    next_col: usize,
    next_line: usize,
    text: String,
    fg_color: u32,
    bg_color: u32,
}

impl Displayable for TextDisplay {
    fn display(
        &mut self,
        x: usize,
        y: usize,
        framebuffer: &mut dyn FrameBuffer,
    ) -> Result<(), &'static str> {
        let (col, line) = frame_buffer_printer::print_by_bytes(
            framebuffer,
            x,
            y,
            self.width,
            self.height,
            self.text.as_str(),
            self.fg_color,
            self.bg_color,
        )?;
        self.next_col = col;
        self.next_line = line;
        Ok(())
    }

    fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
    }

    fn get_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }
}

impl TextDisplay {
    /// Create a new text displayable.
    /// # Arguments
    /// * `(width, height)`: the size of the text area.
    /// * `(fg_color, bg_color)`: the foreground and background color of the text area.
    pub fn new(
        width: usize,
        height: usize,
        fg_color: u32,
        bg_color: u32,
    ) -> Result<TextDisplay, &'static str> {
        Ok(TextDisplay {
            width: width,
            height: height,
            next_col: 0,
            next_line: 0,
            text: String::new(),
            fg_color: fg_color,
            bg_color: bg_color,
        })
    }

    /// Sets the content of the text displayable.
    pub fn set_text(&mut self, text: &str) {
        self.text = String::from(text);
    }

    /// Gets the dimensions of the text area to display
    pub fn get_dimensions(&self) -> (usize, usize) {
        (self.width / CHARACTER_WIDTH, self.height / CHARACTER_HEIGHT)
    }

    /// Display a cursor within the text displayable.
    /// # Arguments
    /// * `cursor`: the cursor to display.
    /// * `(x, y)`: the position of the text displayable in the framebuffer.
    /// * `(col, line):` the position of the cursor in the text displayable.
    /// * `framebuffer:` the framebuffer to display in.
    pub fn display_cursor(
        &mut self,
        cursor: &mut Cursor,
        x: usize,
        y: usize,
        col: usize,
        line: usize,
        framebuffer: &mut dyn FrameBuffer,
    ) {
        if cursor.blink() {
            let color = if cursor.show {
                cursor.color
            } else {
                self.bg_color
            };
            frame_buffer_drawer::fill_rectangle(
                framebuffer,
                x + col * CHARACTER_WIDTH,
                y + line * CHARACTER_HEIGHT,
                CHARACTER_WIDTH,
                CHARACTER_HEIGHT,
                color,
            );
        }
    }

    /// Gets the (column, line) position of the next symbol.
    pub fn get_next_pos(&self) -> (usize, usize) {
        (self.next_col, self.next_line)
    }
}

/// A cursor structure. It contains whether it is enabled,
/// the frequency it blinks, the last time it blinks, the current blink state show/hidden, and its color.
/// A cursor is a special symbol which can be displayed within a text displayable.
pub struct Cursor {
    enabled: bool,
    freq: u64,
    time: TscTicks,
    show: bool,
    color: u32,
}

impl Cursor {
    /// Creates a new cursor structure.
    pub fn new(color: u32) -> Cursor {
        Cursor {
            enabled: true,
            freq: DEFAULT_CURSOR_FREQ,
            time: tsc_ticks(),
            show: true,
            color: color,
        }
    }

    /// Resets the blink state of the cursor.
    pub fn reset(&mut self) {
        self.show = true;
        self.time = tsc_ticks();
    }

    /// Enables a cursor.
    pub fn enable(&mut self) {
        self.enabled = true;
        self.reset();
    }

    /// Disables a cursor.
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Changes the blink state show/hidden of a cursor based on its frequency. An application calls this function in a loop.
    pub fn blink(&mut self) -> bool {
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

    /// Checks if the cursor should be shown.
    pub fn show(&self) -> bool {
        self.enabled && self.show
    }
}
