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
#[macro_use] extern crate derive_more;

#[cfg(test)]
#[macro_use] extern crate std;

mod ansi_colors;
mod ansi_style;
pub use ansi_colors::*;
pub use ansi_style::*;

use core::cmp::{min, max};
use core::convert::TryInto;
use core::fmt;
use core::num::NonZeroUsize;
use core::ops::{Bound, Deref, DerefMut, Index, IndexMut};
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
    /// The terminal is scrolled to a specific point, given by the 
    /// contained `ScrollbackBufferPoint` that points to the `Unit` 
    /// that will be displayed in the upper-left hand corner of the screen viewport.
    ///
    /// The contained `usize` is the number of screen rows that the pointed-to `Unit`
    /// is displayed after the beginning of that `Unit`'s line.
    ///
    /// In this position, the terminal screen "viewport" is locked
    /// and will **NOT** auto-scroll down to show any newly-outputted lines of text.
    AtUnit(ScrollbackBufferPoint, usize),
    /// The terminal position is scrolled all the way down.
    ///
    /// In this position, the terminal screen "viewport" is **NOT** locked
    /// and will auto-scroll down to show any newly-outputted lines of text.
    ///
    /// For convenience in calculating the screen viewport,
    /// the contained fields are the same as in the `AtUnit` varient.
    ///
    /// In this mode, the contained point must be updated whenever the screen is 
    /// scrolled down by virtue of a new line being displayed at the bottom.
    /// the screen viewport is scrolled up or down.
    Bottom(ScrollbackBufferPoint, usize),
}
impl Default for ScrollPosition {
    fn default() -> Self {
        ScrollPosition::Bottom(ScrollbackBufferPoint::default(), 0)
    }
}
impl ScrollPosition {
    /// Returns a two-item tuple:
    /// 1. The point (`Unit`) in the scrollback_buffer at which the screen viewport starts,
    ///    which maps to `ScreenPoint(0, 0)`. 
    /// 2. The offset in number of displayed rows that the above `ScrollbackBufferPoint`
    ///    is at from the beginning of its `Line`.  
    ///    If `0`, the above point represents the first `Unit` at the beginning of its `Line`.
    ///    This is useful for calculating how many more rows will be occupied by
    ///    the remainder of the `Line` starting from the `Unit` at the point given above.
    fn start_point(&self) -> (ScrollbackBufferPoint, usize) {
        match self {
            ScrollPosition::Top => (ScrollbackBufferPoint::default(), 0), // (0,0)
            ScrollPosition::AtUnit(point, row_offset) => (*point, *row_offset),
            ScrollPosition::Bottom(point, row_offset) => (*point, *row_offset),
        }
    }
}


/// An entire unbroken line of characters (`Unit`s) that has been written to a terminal.
///
/// `Line`s *only* end at an actual hard line break, i.e., a newline character `'\n'`.
///
#[derive(Debug, Default, Deref, DerefMut)]
struct Line {
    /// The actual characters that comprise this `Line`.
    #[deref] #[deref_mut]
    units: Vec<Unit>,
    /// The indices of the `Unit`s in the `Line` that come at the very beginning of a 
    /// soft line break, i.e., a wrapped line. 
    /// This is a cached value used to accelerate the calculations of which screen coordinates
    /// point to which coordinates of lines/units in the scrollback buffer.
    soft_line_breaks: Vec<UnitIndex>,
}

impl Index<UnitIndex> for Line {
    type Output = Unit;
    fn index(&self, index: UnitIndex) -> &Self::Output {
        &self.units[index.0]
    }
}
impl IndexMut<UnitIndex> for Line {
    fn index_mut(&mut self, index: UnitIndex) -> &mut Self::Output {
        &mut self.units[index.0]
    }
}
impl Line {
    /// Returns a new empty Line.
    fn new() -> Line {
        Line::default()
    }

    /// Inserts the given `Unit` into this `Line` at the given index. 
    ///
    /// This adjusts all soft line breaks (line wraps) as needed to properly
    /// display this `Line` on screen, but does not actually re-display it.
    ///
    /// If the given `UnitIndex` is within the existing bounds of this `Line`, 
    /// all `Unit`s after it will be shifted to the right by one,
    /// and the soft line breaks will be updated accordingly.
    ///
    /// If the given `UnitIndex` is beyond the existing bounds of this `Line`,
    /// then the `Line` will be padded with enough empty `Units` such that the given `Unit`
    /// will be inserted at the correct `UnitIndex`.
    /// The empty padding `Unit`s will have the same [`Style`] as the given `Unit`.
    fn insert_unit(&mut self, idx: UnitIndex, unit: Unit, screen_width: ColumnIndex, tab_width: u16) {
        if idx.0 < self.units.len() {
            // The unit index is withing the existing bounds of the Line, so insert it.
            self.units.insert(idx.0, unit);
            // TODO: there is definitely a more efficient way to recalculate the soft line breaks
            //       rather than iterating over every single unit in this line.
            self.recalculate_soft_line_breaks(screen_width, tab_width);
        } 
        else {
            // The unit index is beyond the existing bounds of this Line, so fill it with empty Units as padding.
            let range_of_empty_padding = self.units.len() .. idx.0;
            warn!("Untested scenario: padding Line with {} empty Units from {:?}", 
                range_of_empty_padding.len(), range_of_empty_padding,
            );
            self.units.reserve(range_of_empty_padding.len() + 1);
            for _i in range_of_empty_padding {
                self.units.push(Unit { character: Character::default(), style: unit.style });
            }
            self.units.push(unit);
            self.recalculate_soft_line_breaks(screen_width, tab_width);
        }
    }

    /// Calculates and returns the displayabe width in columns 
    /// of the `Unit`s in this `Line` from the given `start` index (inclusive)
    /// to the given `end` index (exclusive).
    fn calculate_displayed_width_starting_at_unit(&self, start: UnitIndex, end: UnitIndex, tab_width: u16) -> usize {
        (&self.units[start.0 .. end.0])
            .iter()
            .map(|unit| match unit.displayable_width() {
                0 => tab_width,
                w => w,
            } as usize)
            .sum()
    }

    /// Returns the number of rows on the screen that this `Line` will span when displayed.
    fn num_rows_as_displayed(&self) -> usize {
        self.soft_line_breaks.len() + 1
    }

    /// Iterates over all `Unit`s in this `Line` to recalculate where the soft line breaks
    /// (i.e., line wraps) should occur.
    fn recalculate_soft_line_breaks(&mut self, screen_width: ColumnIndex, tab_width: u16) {
        let mut breaks = Vec::new();
        let mut column_idx_of_unit = ColumnIndex(0);
        for (i, unit) in self.units.iter().enumerate() {
            let width = ColumnIndex(match unit.displayable_width() {
                0 => tab_width,
                w => w,
            });
            column_idx_of_unit += width;
            if column_idx_of_unit >= screen_width {
                breaks.push(UnitIndex(i));
                column_idx_of_unit = ColumnIndex(0);
            } 
        }
        self.soft_line_breaks = breaks;
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
#[derive(Deref, DerefMut)]
pub struct TextTerminal<Output> where Output: bare_io::Write {
    /// The actual terminal state. 
    #[deref] #[deref_mut]
    inner: TerminalInner<Output>,

    /// The VTE parser for parsing VT100/ANSI/xterm control and escape sequences.
    ///
    /// The event handler for the [`Parser`] is a transient zero-cost object 
    /// of type [`TerminalParserHandler`] that is created on demand in 
    /// [`TextTerminal::handle_input()`] every time an input byte needs to be handled.
    parser: Parser,
}


/// A row-major vector of [`Line`]s that allows custom indexing.
///
/// If indexed by a [`LineIndex`], it returns a [`Line`] reference,
/// which itself can be indexed by a [`UnitIndex`].
///
/// If indexed by a [`ScrollbackBufferPoint`] value, 
/// it returns a reference to the [`Unit`] at that point.
#[derive(Deref, DerefMut)]
struct Lines(Vec<Line>);
impl Index<LineIndex> for Lines {
    type Output = Line;
    fn index(&self, index: LineIndex) -> &Self::Output {
        &self.0[index.0]
    }
}
impl IndexMut<LineIndex> for Lines {
    fn index_mut(&mut self, index: LineIndex) -> &mut Self::Output {
        &mut self.0[index.0]
    }
}
impl Index<ScrollbackBufferPoint> for Lines {
    type Output = Unit;
    fn index(&self, index: ScrollbackBufferPoint) -> &Self::Output {
        &self[index.line_idx][index.unit_idx]
    }
}
impl IndexMut<ScrollbackBufferPoint> for Lines {
    fn index_mut(&mut self, index: ScrollbackBufferPoint) -> &mut Self::Output {
        &mut self[index.line_idx][index.unit_idx]
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
    screen_size: ScreenPoint,

    /// The starting index of the scrollback buffer string slice that is currently being displayed on the text display
    scroll_position: ScrollPosition,

    /// The number of spaces a tab character `'\t'` occupies when displayed.
    tab_width: u16,

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
                screen_size: ScreenPoint { column: ColumnIndex(width), row: RowIndex(height) },
                scroll_position: ScrollPosition::default(),
                tab_width: 4,
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
        self.screen_size.column = ColumnIndex(max(2, width));
        self.screen_size.row    = RowIndex(   max(2, height));
    }

    /// Returns the size `(columns, rows)` of this terminal's screen, 
    /// in units of displayable characters.
    pub fn screen_size(&self) -> (u16, u16) {
        (self.screen_size.column.0, self.screen_size.row.0)
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
    /*
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
    */


    /// Calculates the point in the scrollback buffer that the given `cursor_point`
    /// is pointing to, based on the current position of the screen viewport.
    ///
    /// If the given `cursor_point` does not point to a `Unit` that exists in the scrollback buffer,
    /// then it is adjusted to the nearest row and column that **does** align with a 
    /// line and unit index that actually exist in the scrollback buffer. 
    ///
    /// Returns a tuple of the point in the scrollback buffer that corresponds to the 
    /// adjusted cursor point, and the adjusted cursor point itself.
    fn cursor_position_to_scrollback_position(
        &self,
        cursor_point: ScreenPoint
    ) -> (ScrollbackBufferPoint, ScreenPoint) {
        let target_row = cursor_point.row.0 as usize;

        // The `start_point` corresponds to the unit that is currently displayed at `ScreenPoint(0,0)`
        let (start_point, mut row_offset) = self.scroll_position.start_point();
        let screen_width = self.screen_size.column.0 as usize;

        let mut line_index   = start_point.line_idx;
        let mut unit_index   = start_point.unit_idx;
        let mut row_index    = 0;
        let mut line;
        
        loop {
            // TODO: handle the case when the cursor point is beyond the bounds of the scrollback buffer 
            line = &self.scrollback_buffer[start_point.line_idx];
            row_index += line.num_rows_as_displayed() - row_offset;

            // If this `line` (as displayed) contains the requested cursor row,
            // then we're done iterating over the lines in the scrollback buffer.
            if row_index >= target_row {
                break;
            }

            // Advance to the next line in the scrollback buffer
            line_index += LineIndex(1);
            unit_index = UnitIndex(0);
            row_offset = 0;
        }

        let row_overshoot = row_index.saturating_sub(target_row);
        // TODO: handle the case when there are no soft line breaks in the given `line` 
        let unit_idx_at_cursor = line.soft_line_breaks[line.soft_line_breaks.len() - 1 - row_overshoot];
        let mut column_idx_of_unit = ColumnIndex(0);
        let mut found_unit = None;
        // TODO: set the end bound of the iteration at the next soft line break if there is one, or if not, stick with `..`.
        for (i, unit) in (&line.units[unit_idx_at_cursor.0 ..]).iter().enumerate() {
            let width = ColumnIndex(match unit.displayable_width() {
                0 => self.tab_width,
                w => w,
            });
            if cursor_point.column >= column_idx_of_unit 
                && cursor_point.column < (column_idx_of_unit + width)
            {
                found_unit = Some(UnitIndex(unit_idx_at_cursor.0 + i));
                break;
            }
            column_idx_of_unit += width;
        }

        let found_unit = if let Some(idx) = found_unit {
            idx
        } else {
            // TODO: adjust the `column_idx_of_unit` when we change the 
            UnitIndex(line.len() - 1)
        };

        (
            ScrollbackBufferPoint { unit_idx: unit_index, line_idx: line_index },
            ScreenPoint { column: column_idx_of_unit, row: cursor_point.row, }
        )
    }

    /// Advances the cursor's screen coordinate forward by one unit of the given width.
    /// This does not modify the cursor's scrollback buffer position.
    ///
    /// Returns `true` if the screen needs to be scrolled down by one line.
    fn increment_screen_cursor(&mut self, unit_width: u16) -> bool {
        self.cursor.screen_point.column += ColumnIndex(unit_width);
        if self.cursor.screen_point.column >= self.screen_size.column {
            self.cursor.screen_point.column.0 %= self.screen_size.column.0;
            self.cursor.screen_point.row.0 += 1;
            if self.cursor.screen_point.row >= self.screen_size.row {
                self.cursor.screen_point.row.0 = self.screen_size.row.0 - 1;
                return true;
            }
        }
        false
    }

    // TODO: implement function that calculates, retrieves, and/or creates a unit
    //       in the scrollback buffer at the location corresponding to the current cursor position. 
    //       Keep in mind that the cursor position is relative to the scroll position,
    //       which itself is an index into the scrollback_buffer, so we should start there 
    //       when calculating where in the scrollback_buffer a cursor is pointing to. 


    /*
    /// Moves the cursor to the given position, snapping it to the nearest line and column 
    /// that actually exist in the scrollback buffer. 
    ///
    /// This performs all the logic necessary to update the cursor:
    /// * 
    fn update_cursor_to_position(&mut self, new_cursor_point: ScreenPoint) {
        let start_point = self.scroll_position.start_point();

        let target_point = start_point + new_cursor_point;
        let closest_line_idx = min(target_point.y, self.scrollback_buffer.len().saturating_sub(1) as u16);
        let closest_line = &self.scrollback_buffer[closest_line_idx];
        let closest_column_idx = min(target_point.x, closest_line.len().saturating_sub(1) as u16);
        let closest_column = &closest_line[closest_column_idx];

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
    */

    /// Displays the given range of `Unit`s in the scrollback buffer
    /// by writing them to this terminal's backend.
    ///
    /// The `Unit` at the `scrollback_start` point will be displayed at `screen_start`,
    /// and all `Unit`s up until the given `scrollback_end` point will be written to
    /// successive points on the screen.
    fn display(
        &mut self,
        scrollback_start: ScrollbackBufferPoint,
        scrollback_end:   ScrollbackBufferPoint,
        _screen_start:    ScreenPoint,
        previous_style:   Option<Style>,
    ) -> bare_io::Result<usize> {

        let mut char_encode_buf = [0u8; 4];
        let mut bytes_written = 0;
        let mut previous_style = previous_style.unwrap_or_default();

        // For now, we just assume that the backend output stream is a linear "file"
        // that can't adjust its position, so we just write directly to it 
        // whilst ignoring the `screen_start` parameter.
        let mut start_unit = scrollback_start.unit_idx; 
        for line_idx in scrollback_start.line_idx.0 ..= scrollback_end.line_idx.0 {
            let line_idx = LineIndex(line_idx);
            let line = &self.scrollback_buffer[line_idx];

            // Write the requested part of this line, up to the entire line.
            let end = if scrollback_end.line_idx == line_idx {
                scrollback_end.unit_idx.0
            } else {
                line.units.len()
            };
            for unit in &line.units[start_unit.0 ..= end] {
                // First, write out the escape sequences for the difference in style.
                if unit.style != previous_style {
                    let mut diff_iter = unit.style.diff(&previous_style);
                    // Only write out the escape sequences if there is at least one style difference.
                    if let Some(first_code) = diff_iter.next() {
                        bytes_written += self.backend.write(AnsiStyleCodes::ESCAPE_PREFIX)?;
                        bytes_written += self.backend.write(first_code.to_escape_code().as_bytes())?;
                        for code in diff_iter {
                            bytes_written += self.backend.write(AnsiStyleCodes::ESCAPE_DELIM)?;
                            bytes_written += self.backend.write(code.to_escape_code().as_bytes())?;
                        }
                        bytes_written += self.backend.write(AnsiStyleCodes::ESCAPE_SUFFIX)?;
                    }
                }
                previous_style = unit.style;
    
                // Second, write out the actual character(s).
                bytes_written += self.backend.write(match unit.character {
                    Character::Single(ref ch) => ch.encode_utf8(&mut char_encode_buf[..]).as_bytes(),
                    Character::Multi(ref s) => s.as_bytes(),
                })?;
            }
            // If we wrote out the entire `Line`, write out a newline character.
            if line_idx < scrollback_end.line_idx {
                bytes_written += self.backend.write(b"\n")?;
            } 

            start_unit = UnitIndex(0);
        }

        Ok(bytes_written)
    }
}

#[derive(Deref, DerefMut)]
struct TerminalParserHandler<'term, Output: bare_io::Write> {
    terminal: &'term mut TerminalInner<Output>,
}

impl<'term, Output: bare_io::Write> Perform for TerminalParserHandler<'term, Output> {
    fn print(&mut self, c: char) {
        debug!("[PRINT]: char: {:?}", c);
        let screen_size = self.screen_size;
        let tab_width = self.tab_width;
        let buf_pos = self.cursor.scrollback_point;
        let dest_line = &mut self.scrollback_buffer[buf_pos.line_idx];
        let new_unit = Unit { character: Character::Single(c), style: Style::default() };
        let new_unit_width = new_unit.displayable_width();
        dest_line.insert_unit(
            buf_pos.unit_idx,
            new_unit,
            screen_size.column,
            tab_width,
        );

        let orig_screen_pos = self.cursor.screen_point;
        let needs_scroll_down = self.increment_screen_cursor(new_unit_width);
        let new_buf_pos = self.cursor.scrollback_point;
        self.display(buf_pos, new_buf_pos, orig_screen_pos, None).expect("print(): writing to backend failed"); // TODO: use previous style
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            AsciiControlCodes::NewLine | AsciiControlCodes::CarriageReturn => {
                // Insert a new line into the scrollback buffer
                let new_line_idx = self.cursor.scrollback_point.line_idx + LineIndex(1); 
                self.cursor.scrollback_point.line_idx = new_line_idx;
                self.cursor.scrollback_point.unit_idx = UnitIndex(0); 
                self.scrollback_buffer.insert(new_line_idx.0, Line::new());
                self.cursor.underneath = Unit::default(); // TODO: use style from the previous unit
                
                // Adjust the screen cursor to the next row
                let needs_scroll_down = {
                    self.cursor.screen_point.column = ColumnIndex(0);
                    self.cursor.screen_point.row.0 += 1;
                    if self.cursor.screen_point.row >= self.screen_size.row {
                        self.cursor.screen_point.row.0 = self.screen_size.row.0 - 1;
                        true
                    } else {
                        false
                    }
                };
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
        Character::Single('\u{0}')
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

/// A 2D position value that represents a point on the screen,
/// in which `(0, 0)` represents the top-left corner.
/// Thus, a valid `ScreenPoint` must fit be the bounds of 
/// the current screen dimensions.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[derive(Add, AddAssign, Sub, SubAssign)]
pub struct ScreenPoint {
    column: ColumnIndex,
    row: RowIndex,
}

/// An index of a row in the screen viewport. 
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[derive(Add, AddAssign, Sub, SubAssign)]
pub struct RowIndex(u16);
/// An index of a column in the screen viewport. 
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[derive(Add, AddAssign, Sub, SubAssign)]
pub struct ColumnIndex(u16);


/// A 2D position value that represents a point in the scrollback buffer,
/// in which `(0, 0)` represents the `Unit` at the first column of the first line.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[derive(Add, AddAssign, Sub, SubAssign)]
pub struct ScrollbackBufferPoint {
    unit_idx: UnitIndex,
    line_idx: LineIndex,
}

/// An index of a `Line` in the scrollback buffer.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[derive(Add, AddAssign, Sub, SubAssign)]
pub struct LineIndex(usize);
/// An index of a `Unit` in a `Line` in the scrollback buffer.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[derive(Add, AddAssign, Sub, SubAssign)]
pub struct UnitIndex(usize);


#[derive(Debug, Default)]
struct Cursor {
    /// The position of the cursor on the terminal screen,
    /// given as `(x, y)` where `x` is the row index
    /// and `y` is the column index.
    screen_point: ScreenPoint,
    /// The position in the scrollback buffer of the `Line` and `Unit`
    /// that the cursor is currently pointing to. 
    /// This determines where the next printed character will be written.
    scrollback_point: ScrollbackBufferPoint,
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
