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

// TODO: FIXME: remove this once the implementation is complete.
#![allow(dead_code, unused_variables, unused_imports)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate bitflags;
extern crate event_types;
extern crate unicode_width;
extern crate bare_io;
extern crate vte;

#[cfg(test)]
#[macro_use] extern crate std;

mod ansi_colors;
mod ansi_style;
pub use ansi_colors::*;
pub use ansi_style::*;

use core::cmp::{min, max};
use core::convert::TryInto;
use core::fmt;
use core::ops::{Deref, DerefMut, Index, IndexMut};
use alloc::string::String;
use alloc::vec::Vec;
use bare_io::{Read, Write};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use vte::{Parser, Perform};


/// The position ("viewport") that the terminal is currently scrolled to. 
/// 
/// By default, the terminal starts at the `Bottom`, 
/// such that it will auto-scroll upon new characters being displayed.
pub enum ScrollPosition {
    /// The terminal is scrolled all the way up.
    ///
    /// In this position, the terminal screen "viewport" is locked
    /// and will **NOT** auto-scroll down to show any newly-outputted lines of text.
    Top,
    /// The terminal is scrolled to a specific point,
    /// for which the starting position is given by the `Unit`
    /// located at the specified point.
    /// In other words, the contained `Point2D` points to the `Unit` that 
    /// will be displayed in the upper-left hand corner of the screen viewport.
    ///
    /// In this position, the terminal screen "viewport" is locked
    /// and will **NOT** auto-scroll down to show any newly-outputted lines of text.
    UnitIndex(Point2D),
    /// The terminal position is scrolled all the way down.
    ///
    /// In this position, the terminal screen "viewport" is **NOT** locked
    /// and will auto-scroll down to show any newly-outputted lines of text.
    ///
    /// For convenience in calculating the screen viewport,
    /// the contained `Point2D` points to the position of the `Unit` that 
    /// will be displayed in the upper-left hand corner,
    /// much like the above `UnitIndex` variant.
    /// Thus, the contained point will need to be updated every time a new line
    /// is displayed at the bottom and the rest of the content is shifted up.
    Bottom(Point2D),
}
impl Default for ScrollPosition {
    fn default() -> Self {
        ScrollPosition::Bottom(Point2D { x: 0, y: 0 })
    }
}


/// An entire unbroken line of characters (`Unit`s) that has been written to a terminal.
///
/// `Line`s *only* end at an actual hard line break, i.e., a newline character `'\n'`.
///
struct Line {
    /// The actual characters that comprise this `Line`.
    units: Vec<Unit>,
    /// The number of columns (character spaces) required to display this entire row.
    /// This does NOT necessarily correspond to the number of units, 
    /// because some wider characters like tabs may consume more than one column.
    ///
    /// This is a cached value that may need to be recalculated
    /// whenever the characters (`units`) in this `Line` are modified.
    displayed_width: usize,
}
impl Index<u16> for Line {
    type Output = Unit;
    fn index(&self, index: u16) -> &Self::Output {
        &self.units[index as usize]
    }
}
impl IndexMut<u16> for Line {
    fn index_mut(&mut self, index: u16) -> &mut Self::Output {
        &mut self.units[index as usize]
    }
}
impl Line {
    /// Returns a new empty Line.
    fn new() -> Line {
        Line { units: Vec::new(), displayed_width: 0 }
    }

    /// Writes this entire `Line` to the given `writer` output stream.
    ///
    /// Returns the total number of bytes written.
    fn write_line_to<W: Write>(
        &self,
        writer: &mut W,
        previous_style: Option<Style>
    ) -> bare_io::Result<usize> {
        let mut char_encode_buf = [0u8; 4];
        let mut bytes_written = 0;

        let mut previous_style = previous_style.unwrap_or_default();

        for unit in &self.units {
            // First, write out the escape sequences for the difference in style.
            if unit.style != previous_style {
                let mut diff_iter = unit.style.diff(&previous_style);
                // Only write out the escape sequences if there is at least one style difference.
                if let Some(first_code) = diff_iter.next() {
                    bytes_written += writer.write(AnsiStyleCodes::ESCAPE_PREFIX)?;
                    bytes_written += writer.write(first_code.to_escape_code().as_bytes())?;
                    for code in diff_iter {
                        bytes_written += writer.write(AnsiStyleCodes::ESCAPE_DELIM)?;
                        bytes_written += writer.write(code.to_escape_code().as_bytes())?;
                    }
                    bytes_written += writer.write(AnsiStyleCodes::ESCAPE_SUFFIX)?;
                }
            }
            previous_style = unit.style;

            // Second, write out the actual character(s).
            bytes_written += writer.write(match unit.character {
                Character::Single(ref ch) => ch.encode_utf8(&mut char_encode_buf[..]).as_bytes(),
                Character::Multi(ref s) => s.as_bytes(),
            })?;
        }
        // At the end of the `Line`, write out a newline character.
        bytes_written += writer.write(b"\n")?;
        
        Ok(bytes_written)
    }
}


/// A text-based terminal that supports the ANSI, xterm, VT100, and other standards. 
///
/// The terminal's text buffer (scrollback buffer) is simply a sequence of `Unit`s,
/// in which each `Unit` contains one or more characters to be displayed. 
/// The scrollback buffer is logically a 2-D array of `Unit`s but is stored on a per-line basis,
/// such that a `Line` is a `Vec<Unit>`, and the buffer itself is a `Vec<Line>`. 
/// This representation helps avoid huge contiguous dynamic memory allocations. 
///
pub struct TextTerminal<Output> where Output: bare_io::Write {
    /// The actual terminal state. 
    inner: TerminalInner<Output>,

    /// The VTE parser for parsing VT100/ANSI/xterm control and escape sequences.
    ///
    /// The event handler for the [`Parser`] is a transient zero-cost object 
    /// of type [`TerminalParserHandler`] that is created on demand in 
    /// [`TextTerminal::handle_input()`] every time an input byte needs to be handled.
    parser: Parser,
}
impl<Output: bare_io::Write> Deref for TextTerminal<Output> {
    type Target = TerminalInner<Output>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl<Output: bare_io::Write> DerefMut for TextTerminal<Output> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}


/// A row-major vector of [`Line`]s that allows custom indexing.
///
/// If indexed by a `u16` value representing the row index (y-value),
/// it returns a [`Line`] reference, which itself can be indexed by a `u16` column index (x-value);
///
/// If indexed by a `Point2D` value, it returns a reference to the [`Unit`]
struct Lines(Vec<Line>);
impl Index<u16> for Lines {
    type Output = Line;
    fn index(&self, index: u16) -> &Self::Output {
        &self.0[index as usize]
    }
}
impl IndexMut<u16> for Lines {
    fn index_mut(&mut self, index: u16) -> &mut Self::Output {
        &mut self.0[index as usize]
    }
}
impl Index<Point2D> for Lines {
    type Output = Unit;
    fn index(&self, index: Point2D) -> &Self::Output {
        &self.0[index.y as usize][index.x]
    }
}
impl IndexMut<Point2D> for Lines {
    fn index_mut(&mut self, index: Point2D) -> &mut Self::Output {
        &mut self.0[index.y as usize][index.x]
    }
}
impl Deref for Lines {
    type Target = Vec<Line>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for Lines {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}


pub struct TerminalInner<Output> where Output: bare_io::Write {
    /// The buffer of all content that is currently displayed or has been previously displayed
    /// on this terminal's screen, including in-band control and escape sequences.
    /// This is what should be written out directly to the terminal backend.
    ///
    /// Because this includes control/escape sequences in addition to regular characters,
    /// the size of this scrollback buffer cannot be used to calculate line wrap lengths or scroll/cursor positions.
    scrollback_buffer: Lines,

    /// The width and height of this terminal's screen, i.e. how many columns and rows of characters it can display.
    screen_size: Point2D,

    /// The starting index of the scrollback buffer string slice that is currently being displayed on the text display
    scroll_position: ScrollPosition,

    /// The number of spaces a tab character `'\t'` occupies when displayed.
    tab_size: u16,

    /// The cursor of the terminal.
    ///
    /// The cursor determines *where* the next input action will be applied to the terminal, 
    /// such as inserting or overwriting a character, deleting text, selecting, etc. 
    cursor: Cursor,

    // /// The mode determines what specific action will be taken on receiving an input,
    // /// such as whether we should insert or overwrite new character input. 
    // mode: TerminalMode,

    /// The sink (I/O stream) to which sequences of data are written,
    /// inclusive of all control and escape sequences. 
    /// This should be treated as an opaque device that can only accept a stream of bytes.
    backend: Output,
}

impl<Output: bare_io::Write> TextTerminal<Output> {
    /// Create an empty `TextTerminal` with no text content.
    ///
    /// # Arguments 
    /// * (`width`, `height`): the screen size of the terminal in number of `(columns, rows)`.
    /// * `backend`: the I/O stream to which data bytes will be written.
    ///
    /// For example, a standard VGA text mode terminal is 80x25 (columns x rows).
    pub fn new(width: u16, height: u16, backend: Output) -> TextTerminal<Output> {
        let mut terminal = TextTerminal {
            inner: TerminalInner {
                scrollback_buffer: Lines(vec![Line::new()]), // start with one empty line
                screen_size: Point2D { x: width, y: height },
                scroll_position: ScrollPosition::default(),
                tab_size: 4,
                cursor: Cursor::default(),
                // mode: TerminalMode::default(),
                backend,
            },
            parser: Parser::new(),
        };

        // TODO: test printing some formatted text to the terminal
        let _ = terminal.inner.backend.write(b"Hello from the TextTerminal! This is not yet functional.\n");

        // TODO: issue a term info command to the terminal backend
        //       to obtain its size, and then resize this new `terminal` accordingly

        terminal
    }

    /// Pulls as many bytes as possible from the given [`Read`]er
    /// and handles that stream of bytes as input into this terminal.
    ///
    /// Returns the number of bytes read from the given reader.
    pub fn handle_input<R: Read>(&mut self, reader: &mut R) -> bare_io::Result<usize> {
        const READ_BATCH_SIZE: usize = 128;
        let mut total_bytes_read = 0;
        let mut buf = [0; READ_BATCH_SIZE];

        let mut handler = TerminalParserHandler { terminal: &mut self.inner };

        // Keep reading for as long as there are more bytes available.
        let mut n = READ_BATCH_SIZE;
        while n == READ_BATCH_SIZE {
            n = reader.read(&mut buf)?;
            total_bytes_read += n;

            for byte in &buf[..n] {
                self.parser.advance(&mut handler, *byte);
            }
        }

        Ok(total_bytes_read)
    }

    /// Resizes this terminal's screen to be `width` columns and `height` rows (lines),
    /// in units of *number of characters*.
    ///
    /// This does not automatically flush the terminal, redisplay its output, or recalculate its cursor position.
    ///
    /// Note: values will be adjusted to the minimum width and height of `2`. 
    pub fn resize(&mut self, width: u16, height: u16) {
        self.screen_size.x = max(2, width);
        self.screen_size.y = max(2, height);
    }

    /// Returns the size `(columns, rows)` of this terminal's screen, 
    /// in units of displayable characters.
    pub fn screen_size(&self) -> Point2D {
        self.screen_size
    }


    /// Flushes the entire viewable region of the terminal's screen
    /// to the backend output stream.
    ///
    /// No caching or performance optimizations are used. 
    pub fn flush(&mut self) -> bare_io::Result<usize> {
        unimplemented!()
    }
}

impl<Output: bare_io::Write> TerminalInner<Output> {

    fn get_unit_from_cursor(&self) {

    }

    /// Moves the cursor by adding the given `number` of units (columns)
    /// to its horizontal `x` point.
    ///
    /// The cursor will wrap to the previous or next row it it encounters
    /// the beginning or end of a line.
    fn move_cursor_by(&mut self, number: i16) {
        let Point2D { x: x_old, y: y_old } = self.cursor.position;
        let new_x: i32 = x_old as i32 + number as i32;
        let quotient  = new_x / self.screen_size.x as i32;
        let remainder = new_x % self.screen_size.x as i32;
        self.cursor.position.x = (x_old as i32 + remainder) as u16;
        self.cursor.position.y = (y_old as i32 + quotient)  as u16;
        debug!("Updated cursor: ({}, {}) + {} units --> ({}, {})",
            x_old, y_old, number, self.cursor.position.x, self.cursor.position.y,
        );
    }

    // TODO: implement function that calculates, retrieves, and/or creates a unit
    //       in the scrollback buffer at the location corresponding to the current cursor position. 
    //       Keep in mind that the cursor position is relative to the scroll position,
    //       which itself is an index into the scrollback_buffer, so we should start there 
    //       when calculating where in the scrollback_buffer a cursor is pointing to. 
    fn get_unit_at_cursor(&self) {
        let start_unit = match self.scroll_position {
            ScrollPosition::Top => Point2D { x: 0, y: 0 },
            ScrollPosition::UnitIndex(point) => point,
            ScrollPosition::Bottom(point) => point,
        };
        
    }


    /// Moves the cursor to the given `new_position`.
    ///
    /// If the cursor position specified does not match an existing `Unit`,
    /// the cursor will be moved back to the next closest `Unit` before the `new_position`.
    fn move_cursor_to(&mut self, new_position: Point2D) {
        self.cursor.position = Point2D { 
            x: min(self.screen_size.x, new_position.x),
            y: min(self.screen_size.y, new_position.y),
        }
    }
}

struct TerminalParserHandler<'term, Output: bare_io::Write> {
    terminal: &'term mut TerminalInner<Output>,
}
impl<'term, Output: bare_io::Write> Deref for TerminalParserHandler<'term, Output> {
    type Target = TerminalInner<Output>;
    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}
impl<'term, Output: bare_io::Write> DerefMut for TerminalParserHandler<'term, Output> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.terminal
    }
}

impl<'term, Output: bare_io::Write> Perform for TerminalParserHandler<'term, Output> {
    fn print(&mut self, c: char) {
        debug!("[PRINT]: char: {:?}", c);
        return; 

        let pos = self.cursor.position;
        let dest_unit = &mut self.scrollback_buffer.get(pos);
        dest_unit.character = Character::Single(c);
        self.move_cursor_by(1);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            AsciiControlCodes::NewLine | AsciiControlCodes::CarriageReturn => {
                let pos = self.cursor.position;
                let new_y = pos.y + 1;
                self.scrollback_buffer.insert(new_y as usize, Line::new());
                
                self.move_cursor_to(Point2D { x: 0, y: new_y });
                
            }
            _ => debug!("[EXECUTE]: unhandled byte: {:#X}", byte),
        }
    }

    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {
        debug!("[HOOK]: parameters: {:?}\n\t intermediates: {:X?}\n\t ignore?: {}, action: {:?}",
            _params, _intermediates, _ignore, _action,
        );
    }

    fn put(&mut self, byte: u8) {
        debug!("[PUT]: byte: {:#X?}", byte);
    }

    fn unhook(&mut self) {
        debug!("[UNHOOK]");
    }

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {
        debug!("[OSC_DISPATCH]: bell_terminated?: {:?},\n\t params: {:X?}",
            _bell_terminated, _params,
        );
    }

    fn csi_dispatch(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {
        debug!("[CSI_DISPATCH]: parameters: {:?}\n\t intermediates: {:X?}\n\t ignore?: {}, action: {:?}",
            _params, _intermediates, _ignore, _action,
        );
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {
        debug!("[ESC_DISPATCH]: intermediates: {:X?}\n\t ignore?: {}, byte: {:#X}",
            _intermediates, _ignore, _byte,
        );
    }
}



/// The character stored in each [`Unit`] of the terminal screen. 
///
/// In the typical case, a character (e.g., an ASCII letter or a single UTF-8 character)
/// fits into Rust's primitive `char` type, so we use that by default.
///
/// In the rare case of a character that consist of multiple UTF-8 sequences, e.g., complex emoji,
/// we store the entire character here as a dynamically-allocated `String`. 
/// This saves space in the typical case of a character being 4 bytes or less (`char`-sized).
#[derive(Debug)]
pub enum Character {
    Single(char),
    Multi(String),
}
impl Character {
    /// Returns the number of columns required to display this `Character` within a `Unit`,
    /// either a single `char` or a `String`.
    ///
    /// A return value of `0` indicates this `Unit` requires special handling
    /// to determine its displayable width.
    /// This includes characters like new lines, carriage returns, tabs, etc.
    pub fn displayable_width(&self) -> u16 {
        match &self {
            Character::Single(c) => UnicodeWidthChar::width(*c).unwrap_or(0) as u16,
            Character::Multi(s)  => UnicodeWidthStr::width(&**s) as u16,
        }
    }
}
impl fmt::Display for Character {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            Character::Single(c) => write!(f, "{}", c),
            Character::Multi(s)  => write!(f, "{}", s),
        }
    }
}
impl Default for Character {
    fn default() -> Self {
        Character::Single(' ')
    }
}


/// A `Unit` is a single character block displayed in the terminal.
///
/// Some terminal emulators call this structure a `cell`, 
/// but this is different from the concept of a `cell` because it may contain 
/// more than just a single displayable character, in order to support complex Unicode/emoji.
///
/// Displayable control/escape sequences, i.e., those that affect text style,
/// **do not** exist as individual `Unit`s,
/// though their effects on text style are represented by a `Unit`'s `FormatFlags`.
/// 
/// Non-displayable control/escape sequences, i.e., bells, backspace, delete, etc,
/// are **NOT** saved as `Unit`s in the terminal's scrollback buffer,
/// as they cannot be displayed and are simply transient actions.
#[derive(Debug, Default)]
pub struct Unit {
    /// The displayable character(s) held in this `Unit`.
    character: Character,
    /// The style/formatting with which this `Unit`s character(s) should be displayed.
    style: Style,
}
impl Deref for Unit {
    type Target = Character;
    fn deref(&self) -> &Self::Target {
        &self.character
    }
}

/// A 2D position point where `(0, 0)` represents the top-left corner.
#[derive(Copy, Clone, Debug, Default)]
pub struct Point2D {
    /// Horizontal position: column index.
    x: u16,
    /// Vertical position: row index.
    y: u16,
}

#[derive(Debug, Default)]
struct Cursor {
    /// The position of the cursor on the terminal screen,
    /// given as `(x, y)` where `x` is the line/row index
    /// and `y` is the column index.
    position: Point2D,
    /// The character that is beneath the cursor,
    /// which is possibly occluded by the cursor (depending on its style).
    underneath: Unit,
    /// The style of the cursor when it is displayed.
    style: CursorStyle,
}

#[derive(Debug)]
pub enum CursorStyle {
    /// A rectangle that covers the entire character box. This is the default.
    FilledBox,
    /// A line beneath the character box.
    Underscore,
    /// A line before (to the left of) the character box.
    Bar,
    /// An empty box that surrounds the character but does not occlude it.
    EmptyBox,
}
impl Default for CursorStyle {
    fn default() -> Self {
        CursorStyle::FilledBox
    }
}
