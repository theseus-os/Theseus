//! This crate defines a text displayable.
//! A text displayable profiles a block of text to be displayed onto a framebuffer.
//!
//! This crate also defines a cursor structure. A cursor is a special symbol which can be displayed. The structure specifies the size and blink frequency and implements the blink method for a cursor. It also provides a display function for the cursor.

#![no_std]

#[macro_use]
extern crate alloc;
extern crate displayable;
extern crate font;
extern crate frame_buffer;
extern crate frame_buffer_alpha;
extern crate frame_buffer_drawer;
extern crate frame_buffer_printer;
extern crate spin;
extern crate window;
extern crate window_manager_alpha;

use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::ops::DerefMut;
use displayable::{Displayable, TextDisplayable};
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH};
use frame_buffer::{Coord, FrameBuffer};
use frame_buffer_alpha::AlphaPixel;
use spin::{Mutex};
use window::WindowProfile;
use window_manager_alpha::WindowProfileAlpha;

/// a textarea with fixed size, showing matrix of chars.
///
/// The chars are allowed to be modified and update, however, one cannot change the matrix size during run-time.
pub struct TextArea {
    /// The position of the textarea in a window
    pub coordinate: Coord,
    line_spacing: usize,
    column_spacing: usize,
    background_color: AlphaPixel,
    text_color: AlphaPixel,
    /// the x dimension char count
    x_cnt: usize,
    /// the y dimension char count
    y_cnt: usize,
    char_matrix: Vec<u8>,
    pub winobj: Weak<Mutex<WindowProfileAlpha>>,
    next_index: usize,
    text: String,
}

impl TextArea {
    /// create new textarea with all characters initialized as ' ' (space character which shows nothing).
    /// after initialization, this textarea has a weak reference to the window object,
    /// and calling the API to change textarea will immediately update display on screen
    ///
    /// * `coordinate`, `width`, `height`: the position and size of this textarea. Note that position is relative to window
    /// * `winobj`: the window which the textarea is in
    /// * `line_spacing`: the spacing between lines, default to 2
    /// * `column_spacing`: the spacing between chars, default to 1
    /// * `background_color`: the background color, default to transparent
    /// * `text_color`: the color of text, default to opaque black
    pub fn new(
        coordinate: Coord,
        width: usize,
        height: usize,
        winobj: &Arc<Mutex<WindowProfileAlpha>>,
        line_spacing: Option<usize>,
        column_spacing: Option<usize>,
        background_color: Option<AlphaPixel>,
        text_color: Option<AlphaPixel>,
    ) -> Result<TextArea, &'static str> {
        let mut textarea: TextArea = TextArea {
            coordinate: coordinate,
            line_spacing: match line_spacing {
                Some(m) => m,
                _ => 2,
            },
            column_spacing: match column_spacing {
                Some(m) => m,
                _ => 1,
            },
            background_color: match background_color {
                Some(m) => m,
                _ => 0xFFFFFFFF, // default is a transparent one
            },
            text_color: match text_color {
                Some(m) => m,
                _ => 0x00000000, // default is an opaque black
            },
            x_cnt: 0, // will modify later
            y_cnt: 0, // will modify later
            char_matrix: Vec::new(),
            winobj: Arc::downgrade(winobj),
            next_index: 0,
            text: String::new(),
        };

        // compute x_cnt and y_cnt and remain constant
        if height < (CHARACTER_HEIGHT + textarea.line_spacing)
            || width < (CHARACTER_WIDTH - 1 + textarea.column_spacing)
        {
            return Err("textarea too small to put even one char");
        }
        textarea.x_cnt = width / (CHARACTER_WIDTH - 1 + textarea.column_spacing);
        textarea.y_cnt = height / (CHARACTER_HEIGHT + textarea.line_spacing);
        textarea
            .char_matrix
            .resize(textarea.x_cnt * textarea.y_cnt, ' ' as u8); // first fill with blank char

        Ok(textarea)
    }

    /// get the x dimension char count
    pub fn get_x_cnt(&self) -> usize {
        self.x_cnt
    }

    /// get the y dimension char count
    pub fn get_y_cnt(&self) -> usize {
        self.y_cnt
    }

    /// compute the index of char, does not check bound. one can use this to compute index as argument for `set_char_absolute`.
    pub fn index(&self, col: usize, line: usize) -> usize {
        // does not check bound
        return col + line * self.x_cnt;
    }

    /// set char at given index, for example, if you want to modify the char at (i, j), the `idx` should be `self.index(i, j)`
    pub fn set_char_absolute(&mut self, idx: usize, c: u8) -> Result<(), &'static str> {
        if idx >= self.x_cnt * self.y_cnt {
            return Err("x out of range");
        }
        self.set_char(idx % self.x_cnt, idx / self.x_cnt, c)
    }

    /// set char at given position, where i < self.x_cnt, j < self.y_cnt
    pub fn set_char(&mut self, col: usize, line: usize, c: u8) -> Result<(), &'static str> {
        if col >= self.x_cnt {
            return Err("x out of range");
        }
        if line >= self.y_cnt {
            return Err("y out of range");
        }
        if let Some(winobj_mutex) = self.winobj.upgrade() {
            if self.char_matrix[self.index(col, line)] != c {
                // need to redraw
                let idx = self.index(col, line);
                self.char_matrix[idx] = c;
                let win_coordinate = {
                    let mut winobj = winobj_mutex.lock();
                    let win_coordinate = winobj.get_content_position();
                    self.set_char_in(col, line, c, winobj.framebuffer.deref_mut())?;
                    win_coordinate
                };
                let wcoordinate = self.coordinate
                    + (
                        (col * (CHARACTER_WIDTH - 1 + self.column_spacing)) as isize,
                        (line * (CHARACTER_HEIGHT + self.line_spacing)) as isize,
                    );
                for j in 0..CHARACTER_HEIGHT {
                    for i in 0..CHARACTER_WIDTH - 1 {
                        window_manager_alpha::refresh_pixel_absolute(
                            win_coordinate + wcoordinate + (i as isize, j as isize),
                        )?;
                    }
                }
            }
        } else {
            return Err(
                "winobj not existed, perhaps calling this function after window is destoryed",
            );
        }
        Ok(())
    }

    /// set char at given position in a framebuffer, where i < self.x_cnt, j < self.y_cnt
    pub fn set_char_in(
        &mut self,
        col: usize,
        line: usize,
        c: u8,
        framebuffer: &mut dyn FrameBuffer,
    ) -> Result<(), &'static str> {
        let wcoordinate = self.coordinate
            + (
                (col * (CHARACTER_WIDTH - 1 + self.column_spacing)) as isize,
                (line * (CHARACTER_HEIGHT + self.line_spacing)) as isize,
            );
        for j in 0..CHARACTER_HEIGHT {
            let char_font: u8 = font::FONT_BASIC[c as usize][j];
            for i in 0..CHARACTER_WIDTH - 1 {
                let ncoordinate = wcoordinate + (i as isize, j as isize);
                if char_font & (0x80u8 >> i) != 0 {
                    framebuffer.overwrite_pixel(ncoordinate, self.text_color);
                } else {
                    framebuffer.overwrite_pixel(ncoordinate, self.background_color);
                }
            }
        }
        Ok(())
    }

    /// update char matrix with a new one, must be equal size of current one
    pub fn set_char_matrix(&mut self, char_matrix: &Vec<u8>) -> Result<(), &'static str> {
        if char_matrix.len() != self.char_matrix.len() {
            return Err("char matrix size not match");
        }
        for i in 0..self.x_cnt {
            for j in 0..self.y_cnt {
                self.set_char(i, j, char_matrix[self.index(i, j)])?;
            }
        }
        Ok(())
    }
}

impl TextDisplayable for TextArea {
    fn get_dimensions(&self) -> (usize, usize) {
        (self.x_cnt, self.y_cnt)
    }

    fn get_next_index(&self) -> usize {
        self.next_index
    }

    fn set_text(&mut self, text: &str) {
        self.text = String::from(text);
    }
}

impl Displayable for TextArea {
    fn display(
        &mut self,
        coordinate: Coord,
        framebuffer: Option<&mut dyn FrameBuffer>,
    ) -> Result<Vec<(usize, usize)>, &'static str> {
        if framebuffer.is_some() {
            return Err("TextArea can only display in its default framebuffer");
        }
        self.coordinate = coordinate;
        let a = self.text.clone();
        let a = a.as_bytes();
        let mut i = 0;
        let mut j = 0;
        for k in 0..a.len() {
            let c = a[k] as u8;
            if c == '\n' as u8 {
                for x in i..self.x_cnt {
                    self.set_char(x, j, ' ' as u8)?;
                }
                j += 1;
                i = 0;
            } else {
                self.set_char(i, j, c)?;
                i += 1;
                if i >= self.x_cnt {
                    j += 1;
                    i = 0;
                }
            }
            if j >= self.y_cnt {
                break;
            }
        }

        self.next_index = self.index(i, j);

        if j < self.y_cnt {
            for x in i..self.x_cnt {
                self.set_char(x, j, ' ' as u8)?;
            }
            for y in j + 1..self.y_cnt {
                for x in 0..self.x_cnt {
                    self.set_char(x, y, ' ' as u8)?;
                }
            }
        }
        return Ok(vec![]);
    }

    fn get_position(&self) -> Coord {
        self.coordinate
    }

    fn resize(&mut self, x_cnt: usize, y_cnt: usize) {
        self.x_cnt = x_cnt;
        self.y_cnt = y_cnt;
    }

    fn get_size(&self) -> (usize, usize) {
        (self.x_cnt, self.y_cnt)
    }

    fn as_text_mut(&mut self) -> Result<&mut dyn TextDisplayable, &'static str> {
        Ok(self)
    }

    fn as_text(&self) -> Result<&dyn TextDisplayable, &'static str> {
        Ok(self)
    }
}
