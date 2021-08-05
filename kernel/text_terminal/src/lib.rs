//! A text-mode terminal emulator. 
//!
//! This provides basic support for various standards of control codes and escape sequences:
//! * ANSI
//! * VT100
//! * xterm
//! 
//! This terminal emulator also supports Unicode characters;
//! see the [unicode-segmentation](https://crates.io/crates/unicode-segmentation) crate.
//! This support stems from our usage of Rust [`String`]s, which must be valid UTF-8.
//!
//! The text terminal emulator has several main responsibilities: 
//! * Managing the scrollback buffer, a string of characters that should be printed to the screen.
//! * Determining which parts of that buffer should be displayed and using the window manager to do so.
//! * Handling the command line user input.
//! * Displaying the cursor at the right position
//! * Handling events delivered from the window manager.

#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate event_types;
extern crate color;

use core::cmp::max;
use core::ops::DerefMut;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use event_types::Event;
use color::{Color};



/// A whole unbroken line of characters, inclusive of control/escape sequences and newline characters. 
/// 
struct Line {
    /// Inclusive of the actual newline character at the end.
    /// Thus, an empy line of 
    s: String,
    /// The number of characters required to display this entire `Line`,
    /// i.e., the size of this `Line` in characters excluding 
    displayed_size: usize,
}


/// A text-based terminal that follows the ANSI, xterm, VT100, and other standards. 
pub struct TextTerminal {
    /// The buffer of all content that is currently displayed on this terminal's screen,
    /// including in-band control and escape sequences.
    /// This is what should be written out directly to the terminal backend.
    ///
    /// Because this includes control/escape sequences in addition to regular characters,
    /// the size of this scrollback buffer cannot be used to calculate line wrap lengths or scroll/cursor positions.
    scrollback_buffer: Vec<Line>,

    units: Vec<Unit>,

    /// The width of this terminal's screen, i.e. how many columns of characters it can display. 
    screen_width: u16,
    /// The height of this terminal's screen, i.e. how many rows of characters it can display. 
    screen_height: u16,

    /// Indicates whether the text display is displaying the last part of the scrollback buffer slice
    is_scroll_end: bool,
    /// The starting index of the scrollback buffer string slice that is currently being displayed on the text display
    scroll_start_idx: usize,
    // /// The cursor of the terminal.
    // cursor: Cursor,
}

impl TextTerminal {
    pub fn new() -> TextTerminal {
        unimplemented!()
    }

    /// Resizes this terminal's screen to be `width` columns and `height` rows (lines),
    /// in units of *number of characters*.
    ///
    /// This does not automatically flush the terminal, redisplay its output, or recalculate its cursor position.
    ///
    /// Note: the minimum width and height is `2`. 
    /// Values lower than that will be bumped up to `2`.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.screen_width = max(2, width);
        self.screen_height = max(2, height);
    }

    /// Returns the size `(width, height)` of this terminal's screen, in number of characters. 
    pub fn size(&self) -> (u16, u16) {
        (self.screen_width, self.screen_height)
    }
}


/// A `Unit` is a single character block displayed in the terminal.
///
/// The terminal's text buffer (scrollback buffer) is simply a sequence of `Unit`s,
/// which is stored as a vector but logically represented as a 2-D matrix of `Unit`s:
/// ```ignore
/// [[Unit; SCREEN_WIDTH]; SCREEN_HEIGHT]]
/// ```
/// This representation is needed to support dynamically-resizable screens of terminal text. 
///
/// Displayable control/escape sequences, i.e., those that affect text style,
/// **DO** exist as `Unit`s and are combined into a single `Unit` with the next non-escape/control character,
/// such as a regular ASCII character. 
/// 
/// Non-displayable control/escape sequences, i.e., bells, backspace, delete, etc,
/// are **NOT** saved as `Unit`s in the terminal's scrollback buffer,
/// as they cannot be displayed and are simply transient actions.
pub struct Unit {
    prefix: Vec<u8>,
    /// The main displayable character here. 
    /// We use a String to support 
    character: String,
    suffix: Vec<u8>,
}


impl TextTerminal {

}
