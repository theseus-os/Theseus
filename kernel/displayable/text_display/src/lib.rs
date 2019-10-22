//! This crate defines a text displayable.
//! A text displayable profiles a block of text to be displayed onto a framebuffer.
//!
//! This crate also defines a cursor structure. A cursor is a special symbol which can be displayed. The structure specifies the size and blink frequency and implements the blink method for a cursor. It also provides a display function for the cursor.

#![no_std]

extern crate alloc;
extern crate font;
extern crate frame_buffer;
extern crate frame_buffer_drawer;
extern crate frame_buffer_printer;
extern crate tsc;
extern crate displayable;

use alloc::string::String;
use alloc::vec::Vec;
use displayable::Displayable;
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH};
use frame_buffer::{Coord, FrameBuffer};
use tsc::{tsc_ticks, TscTicks};

const DEFAULT_CURSOR_FREQ: u64 = 400000000;

/// A text displayable profiles the size and color of a block of text. It can display in a framebuffer.
pub struct TextDisplay {
    width: usize,
    height: usize,
    /// The position of the next symbol. It is updated after display and will be useful for optimization.
    next_col: usize,
    next_line: usize,
    text: String,
    fg_color: u32,
    bg_color: u32,
    /// The text cached since last display
    cache: String,
}

impl Displayable for TextDisplay {
    fn display(
        &mut self,
        coordinate: Coord,
        framebuffer: &mut dyn FrameBuffer,
    ) -> Vec<(usize, usize)> {
        // If the cache is the prefix of the new text, just print the additional characters.
        let (string, col, line) = if self.cache.len() > 0 && self.text.starts_with(self.cache.as_str()) {
            (
                &self.text.as_str()[self.cache.len()..self.text.len()],
                self.next_col,
                self.next_line,
            )
        } else {
            (self.text.as_str(), 0, 0)
        };

        let (next_col, next_line, blocks) = frame_buffer_printer::print_string(
            framebuffer,
            coordinate,
            self.width,
            self.height,
            string,
            self.fg_color,
            self.bg_color,
            col,
            line,
        );

        self.next_col = next_col;
        self.next_line = next_line;
        self.cache = self.text.clone();

        return blocks;
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
    /// Creates a new text displayable.
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
            cache: String::new(),
        })
    }

    /// Sets the content of the text displayable.
    pub fn set_text(&mut self, text: &str) {
        self.text = String::from(text);
    }

    /// Gets the dimensions of the text area to display.
    pub fn get_dimensions(&self) -> (usize, usize) {
        (self.width / CHARACTER_WIDTH, self.height / CHARACTER_HEIGHT)
    }

    /// Gets the (column, line) position of the next symbol.
    pub fn get_next_pos(&self) -> (usize, usize) {
        (self.next_col, self.next_line)
    }

    /// Gets the background color of the text area
    pub fn get_bg_color(&self) -> u32 {
        self.bg_color
    }
    
    /// Clear the cache of the text displayable.
    pub fn reset_cache(&mut self) {
        self.cache = String::new();
    }

    /// Translate the index of a character in the text to the location of the text displayable. Return (column, line).
    pub fn get_location(&self, index: usize) -> (usize, usize) {
        let text_width = self.width / CHARACTER_WIDTH;
        (index % text_width, index / text_width)
    }

    /// Translate the location of a character to its index in the text.
    pub fn get_index(&self, column: usize, line: usize) -> usize {
        let text_width = self.width / CHARACTER_WIDTH;
        line * text_width + column
    }
}

/// A cursor structure. It contains whether it is enabled,
/// the frequency it blinks, the last time it blinks, the current blink state show/hidden, and its color.
/// A cursor is a special symbol which can be displayed.
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
    /// It returns whether the cursor should be re-display. If the cursor is enabled, it returns whether the show/hidden state has been changed. Otherwise it returns true because the cursor is disabled and should refresh.
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
        true
    }

    /// Checks if the cursor should be shown.
    pub fn show(&self) -> bool {
        self.enabled && self.show
    }
}

/// Display a cursor at `coordinate` onto a frame buffer. 
/// # Arguments
/// * `cursor`: the cursor to display.
/// * `coordinate`: the coordinate within the `framebuffer` where the cursor is displayed. It specifies the location of the top-left point of the cursor. The `coordinate` is relative to the top-left point `(0, 0)` of the `framebuffer`.
/// * bg_color: the background color of the area if the cursor is hidden.
/// * `framebuffer:` the framebuffer to display onto.
pub fn display_cursor(
    cursor: &mut Cursor,
    coordinate: Coord,
    bg_color: u32,
    framebuffer: &mut dyn FrameBuffer,
) {
    if cursor.blink() {
        let color = if cursor.show() {
            cursor.color
        } else {
            bg_color
        };
        
        frame_buffer_drawer::fill_rectangle(
            framebuffer,
            coordinate + (0, 1),
            CHARACTER_WIDTH,
            CHARACTER_HEIGHT - 2,
            color,
        );
    }
}