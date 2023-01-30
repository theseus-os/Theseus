//! This crate defines a text displayable.
//! A text displayable profiles a block of text to be displayed onto a framebuffer.

#![no_std]

extern crate alloc;
extern crate displayable;
extern crate font;
extern crate framebuffer;
extern crate framebuffer_printer;
extern crate shapes;
extern crate color;

use alloc::string::String;
use displayable::{Displayable};
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH};
use framebuffer::{Pixel, Framebuffer};
use color::Color;
use shapes::{Coord, Rectangle};


/// A text displayable profiles the size and color of a block of text. It can display in a framebuffer.
#[derive(Debug)]
pub struct TextDisplay {
    width: usize,
    height: usize,
    /// The position where the next character will be displayed. 
    /// This is updated after each `display()` invocation, and is useful for optimization.
    next_col: usize,
    next_line: usize,
    text: String,
    fg_color: Color,
    bg_color: Color,
    /// The cache of the text that was last displayed.
    cache: String,
}

impl Displayable for TextDisplay {
    fn display<P: Pixel+ From<Color>> (
        &mut self,
        coordinate: Coord,
        framebuffer: &mut Framebuffer<P>,
    ) -> Result<Rectangle, &'static str> {
        let (string, col, line) = if !self.cache.is_empty() && self.text.starts_with(self.cache.as_str()) {
            (
                &self.text.as_str()[self.cache.len()..self.text.len()],
                self.next_col,
                self.next_line,
            )
        } else {
            (self.text.as_str(), 0, 0)
        };

        let (next_col, next_line, mut bounding_box) = framebuffer_printer::print_string(
            framebuffer,
            coordinate,
            self.width,
            self.height,
            string,
            self.fg_color.into(),
            self.bg_color.into(),
            col,
            line,
        );

        if next_line < self.next_line {
            bounding_box.bottom_right.y = ((self.next_line + 1 ) * CHARACTER_HEIGHT) as isize
        }

        self.next_col = next_col;
        self.next_line = next_line;
        self.cache = self.text.clone();

        Ok(bounding_box + coordinate)
    }

    fn set_size(&mut self, width: usize, height: usize) {
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
    /// * `width`, `height`: the dimensions of the text area, in number of characters.
    /// * `fg_color`, `bg_color`: the color of the text and the background behind the text, respectively.
    pub fn new(
        width: usize,
        height: usize,
        fg_color: Color,
        bg_color: Color,
    ) -> Result<TextDisplay, &'static str> {
        Ok(TextDisplay {
            width,
            height,
            next_col: 0,
            next_line: 0,
            text: String::new(),
            fg_color,
            bg_color,
            cache: String::new(),
        })
    }

    /// Gets the background color of the text area
    pub fn get_bg_color(&self) -> Color {
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

    /// Gets the size of a text displayable in number of characters.
    pub fn get_dimensions(&self) -> (usize, usize) {
        (self.width / CHARACTER_WIDTH, self.height / CHARACTER_HEIGHT)
    }

    /// Gets the index of next character to be displayabled. It is the position next to existing printed characters in the text displayable.
    pub fn get_next_index(&self) -> usize {
        let col_num = self.width / CHARACTER_WIDTH;
        self.next_line * col_num + self.next_col
    }

    /// Sets the text of the text displayable
    pub fn set_text(&mut self, text: &str) {
        self.text = String::from(text);
    }
}
