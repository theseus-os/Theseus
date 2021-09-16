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
//!
//! # Resources
//! * <https://www.gnu.org/software/screen/manual/screen.html#Control-Sequences>
//! * <https://man7.org/linux/man-pages/man4/console_codes.4.html>
//! * <https://vt100.net/docs/vt510-rm/chapter4.html>
//! * <https://en.wikipedia.org/wiki/ANSI_escape_code>

#![no_std]
#![feature(drain_filter)]

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
use alloc::boxed::Box;
pub use ansi_colors::*;
pub use ansi_style::*;

use core::cmp::{Ordering, max, min};
use core::convert::TryInto;
use core::fmt;
use core::iter::repeat;
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
/// `Line`s *only* end at an actual hard line break, i.e., a line feed / newline character.
///
/// Because each `Unit` in a `Line` represents exactly one displayed `Column` on the screen,
/// it is easy to calculate where soft line breaks (line wraps) will occur
/// based on the width of the screen.
#[derive(Debug, Default, Deref, DerefMut)]
pub struct Line {
    /// The actual characters that comprise this `Line`.
    units: Vec<Unit>,
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

    /// Iterates over the `Unit`s in this `Line` to find the next non-continuance `Unit`.
    ///
    /// Returns the `UnitIndex` of that `Unit`.
    #[inline(always)]
    fn next_non_continuance(&self, mut index: UnitIndex) -> UnitIndex {
        while self.units.get(index.0).map(|u| u.wide.is_continuance()).unwrap_or_default() {
            warn!("Untested: forward skipping continuance Unit at {:?}", index);
            index += UnitIndex(1);
        }
        index
    }

    /// Iterates over the `Unit`s in this `Line` to find the previous non-continuance `Unit`.
    ///
    /// Returns the `UnitIndex` of that `Unit`.
    #[inline(always)]
    fn previous_non_continuance(&self, mut index: UnitIndex) -> UnitIndex {
        while self.units.get(index.0).map(|u| u.wide.is_continuance()).unwrap_or_default() {
            warn!("Untested: backward skipping continuance Unit at {:?}", index);
            index -= UnitIndex(1);
        }
        index
    }

    /// Inserts the given character with the given `Style` into this `Line` at the given `index`. 
    ///
    /// If the given `UnitIndex` is within the existing bounds of this `Line`, 
    /// all `Unit`s after it will be shifted to the right by one.
    ///
    /// If the given `UnitIndex` is beyond the existing bounds of this `Line`,
    /// then the `Line` will be padded with enough empty `Units` such that the given `Unit`
    /// will be inserted at the correct `UnitIndex`.
    /// The empty padding `Unit`s will have the given `Style`.
    ///
    /// Returns the `UnitIndex` immediately following the newly-inserted `Unit`(s),
    /// which is where the next `Unit` would be inserted on a future insert operation.
    fn insert_unit(
        &mut self,
        index: UnitIndex,
        c: char,
        style: Style,
        tab_width: u16,
    ) -> UnitIndex {
        let mut index = self.next_non_continuance(index);

        // If the index is beyond the existing bounds of this Line, fill it with empty Units as padding.
        if index.0 > self.units.len() {
            let range_of_empty_padding = self.units.len() .. index.0;
            self.units.reserve(range_of_empty_padding.len() + 1);
            if range_of_empty_padding.len() > 0 {
                warn!("Untested scenario: pushing {} empty padding character(s) to line.", range_of_empty_padding.len());
            }
            for _i in range_of_empty_padding {
                self.units.push(Unit { style, ..Default::default() });
            }
            index += UnitIndex(range_of_empty_padding.len());
        };
        // Now that we've inserted any padding necessary, we simply insert the new Unit.

        // The tab character '\t' requires special handling.
        let num_units_added = if c == '\t' {
            let tab_width = tab_width as usize;
            self.units.reserve(tab_width);
            // Insert one Unit to represent the actual start of the tab character.
            self.units.insert(
                index.0,
                Unit {
                    character: Character::Single(c),
                    style,
                    wide: WideDisplayedUnit::TabStart,
                }
            );
            // Insert `tab_width - 1` empty units to represent the rest of the tab space.
            self.units.splice(
                (index.0 + 1) .. (index.0 + 1), 
                core::iter::repeat(Unit {
                    character: Character::default(),
                    style,
                    wide: WideDisplayedUnit::TabFill,
                }).take(tab_width - 1)
            );
            tab_width
        } 
        else {
            let character = Character::Single(c);
            let displayed_width = character.displayable_width() as usize;
            match displayed_width {
                0 => panic!("Unsupported: inserting 0-width non-TAB character {} ({:?})", c, c),
                1 => {
                    // TODO: check for ligatures for certain characters, which need to be combined into the previous `Unit`.
                    //       In this case, we won't insert `c` into a new Unit, we'll simply append it to the previous `Unit`'s
                    //       contained Character::Multi(...), converting it from a Character::Single(...) as necessary.
                    //       We would then insert a new empty unit with the WideDisplayedUnit::MultiFill tag set.

                    self.units.insert(
                        index.0,
                        Unit { character, style, wide: WideDisplayedUnit::None }
                    );
                    1
                }
                width => {
                    self.units.insert(
                        index.0,
                        Unit { character, style, wide: WideDisplayedUnit::MultiStart }
                    );
                    // Insert `width - 1` empty units to represent the rest of the space occupied by this wide character.
                    self.units.splice(
                        (index.0 + 1) .. (index.0 + 1), 
                        core::iter::repeat(Unit {
                            character: Character::default(),
                            style,
                            wide: WideDisplayedUnit::MultiFill,
                        }).take(width - 1)
                    );
                    width
                }
            }
        };

        index + UnitIndex(num_units_added)
    }

    /// Replaces the existing `Unit`(s) at the given `index` in this `Line` with the given character.
    ///
    /// If the given `UnitIndex` is within the existing bounds of this `Line`, 
    /// the existing `Unit`(s) at that index will be replaced.
    ///
    /// If the given `UnitIndex` is beyond the existing bounds of this `Line`,
    /// this function does the same thing as [`Line::insert_unit()`].
    ///
    /// Returns the `UnitIndex` immediately following the newly-replaced `Unit`(s),
    /// which is where the next `Unit` would be replaced on a future operation.
    fn replace_unit(
        &mut self,
        index: UnitIndex,
        c: char,
        style: Style,
        tab_width: u16
    ) -> UnitIndex {
        let mut index = self.next_non_continuance(index);

        if let Some(unit_to_replace) = self.units.get_mut(index.0) {
            let character = Character::Single(c);
            let old_width = unit_to_replace.displayable_width();
            let new_width = character.displayable_width();
            if old_width == new_width {
                unit_to_replace.character = character;
                unit_to_replace.style = style;
                self.next_non_continuance(index) // move to the next unit
            } else {
                // To handle the case when a new character has a different displayable width 
                // than the existing character it's replacing,
                // we simply remove the existing character's `Unit`(s) 
                // and then insert the new character. 
                self.delete_unit(index);
                self.insert_unit(index, c, style, tab_width)
            }
        } else {
            self.insert_unit(index, c, style, tab_width)
        }
    }

    /// Deletes the given `Unit` from this `Line` at the given index. 
    ///
    /// This also deletes all continuance characters that correspond to this `Unit`.
    ///
    /// As with standard [`Vec`] behavior, all `Unit`s after the given `index` are
    /// shifted to the left.
    /// As such, the given `index` does not need to be moved after invoking this function,
    /// as it will already point to the `Unit` right after the last deleted `Unit`.
    fn delete_unit(&mut self, index: UnitIndex) {
        let index = self.previous_non_continuance(index);

        let _removed_unit = self.units.remove(index.0);
        let _removed_continuance_units = self.units.drain_filter(|&mut unit| unit.wide.is_continuance());

        warn!("Deleted {}-width {:?}", _removed_unit.displayable_width(), _removed_unit);
        for _u in _removed_continuance_units {
            warn!("\t also deleted continuance {:?}", _u);
        }
    }

    /// Returns the number of rows on the screen that this `Line` will span when displayed.
    fn num_rows_as_displayed(&self, screen_width: Column) -> usize {
        (self.units.len() / screen_width.0 as usize) + 1
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
pub struct TextTerminal<Backend> where Backend: TerminalBackend {
    /// The buffer of all content that is currently displayed or has been previously displayed
    /// on this terminal's screen, including in-band control and escape sequences.
    /// This is what should be written out directly to the terminal backend.
    ///
    /// Because this includes control/escape sequences in addition to regular characters,
    /// the size of this scrollback buffer cannot be used to calculate line wrap lengths or scroll/cursor positions.
    scrollback_buffer: ScrollbackBuffer,

    /// The current position in the scrollback buffer, i.e.,
    /// the specific `Line` and `Unit` that the cursor is pointing to.
    /// This determines where the next input action will be applied to the scrollback_buffer, 
    /// such as inserting or overwriting a character, deleting text, selecting, etc. 
    ///
    /// This is the cursor that's modified by calculations in the terminal frontend,
    /// while the `screen_cursor` is modified by calculations in the terminal backend display logic.
    scrollback_cursor: ScrollbackBufferPoint,

    /// The starting index of the scrollback buffer string slice that is currently being displayed on the text display
    scroll_position: ScrollPosition,

    /// The number of spaces a tab character `'\t'` occupies when displayed.
    tab_width: u16,

    /// The on-screen cursor of the terminal.
    cursor: Cursor,

    /// The mode settings/options that define the terminal's behavior.
    mode: TerminalMode,

    /// The terminal backend to which display actions are sent to be handled 
    /// in a backend-specific manner.
    backend: Backend,

    /// The VTE parser for parsing VT100/ANSI/xterm control and escape sequences.
    ///
    /// The event handler for the [`Parser`] is a transient zero-cost object 
    /// of type [`TerminalParserHandler`] that is created on demand in 
    /// [`TextTerminal::handle_input()`] every time an input byte needs to be handled.
    parser: Parser,
}


/// The scrollback buffer is stored as a row-major vector of [`Line`]s.
///
/// If indexed by a [`LineIndex`], it returns a [`Line`] reference,
/// which itself can be indexed by a [`UnitIndex`].
///
/// If indexed by a [`ScrollbackBufferPoint`] value, 
/// it returns a reference to the [`Unit`] at that point.
#[derive(Debug, Deref, DerefMut)]
pub struct ScrollbackBuffer(Vec<Line>);
impl Index<LineIndex> for ScrollbackBuffer {
    type Output = Line;
    fn index(&self, index: LineIndex) -> &Self::Output {
        &self.0[index.0]
    }
}
impl IndexMut<LineIndex> for ScrollbackBuffer {
    fn index_mut(&mut self, index: LineIndex) -> &mut Self::Output {
        &mut self.0[index.0]
    }
}
impl Index<ScrollbackBufferPoint> for ScrollbackBuffer {
    type Output = Unit;
    fn index(&self, index: ScrollbackBufferPoint) -> &Self::Output {
        &self[index.line_idx][index.unit_idx]
    }
}
impl IndexMut<ScrollbackBufferPoint> for ScrollbackBuffer {
    fn index_mut(&mut self, index: ScrollbackBufferPoint) -> &mut Self::Output {
        &mut self[index.line_idx][index.unit_idx]
    }
}


impl<Backend: TerminalBackend> TextTerminal<Backend> {
    /// Create an empty `TextTerminal` with no text content.
    ///
    /// # Arguments 
    /// * (`width`, `height`): the size of the terminal's backing screen in number of `(columns, rows)`.
    /// * `backend`: the I/O stream to which data bytes will be written.
    ///
    /// For example, a standard VGA text mode terminal is 80x25 (columns x rows).
    pub fn new(width: u16, height: u16, mut backend: Backend) -> TextTerminal<Backend> {

        backend.update_screen_size(ScreenSize {
            num_columns: Column(width),
            num_rows: Row(height),
        });

        let mut terminal = TextTerminal {
            scrollback_buffer: ScrollbackBuffer(vec![Line::new()]), // start with one empty line
            scrollback_cursor: ScrollbackBufferPoint::default(),
            scroll_position: ScrollPosition::default(),
            tab_width: 4,
            cursor: Cursor::default(),
            mode: TerminalMode::default(),
            backend,
            parser: Parser::new(),
        };

        // Clear the terminal backend upon start.
        terminal.backend.clear_screen();
        terminal.backend.move_cursor_to(ScreenPoint::default());

        // By default, terminal backends typically operate in Overwrite mode, not Insert mode.
        terminal.backend.set_insert_mode(InsertMode::Overwrite);
        
        let welcome = "Welcome to Theseus's text terminal!";
        terminal.handle_input(&mut welcome.as_bytes()).expect("failed to write terminal welcome message");

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

        let mut handler = TerminalParserHandler { 
            scrollback_buffer: &mut self.scrollback_buffer,
            scrollback_cursor: &mut self.scrollback_cursor,
            cursor: &mut self.cursor,
            backend: &mut self.backend,
            tab_width: &mut self.tab_width,
            mode: &mut self.mode,
        };

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

    /// Flushes the entire viewable region of the terminal's screen
    /// to the backend output stream.
    ///
    /// No caching or performance optimizations are used. 
    pub fn flush(&mut self) -> bare_io::Result<usize> {
        unimplemented!()
    }


    /// Resizes this terminal's screen to be `width` columns and `height` rows (lines),
    /// in units of *number of characters*.
    ///
    /// Currently, this does not automatically flush the terminal, redisplay its output,
    /// or recalculate its cursor position.
    ///
    /// Note: values will be adjusted to the minimum width and height of `2`. 
    pub fn resize_screen(&mut self, width: u16, height: u16) {
        self.backend.update_screen_size(ScreenSize { 
            num_columns: Column(max(2, width)),
            num_rows:    Row(   max(2, height)),
        });
    }

    /// Returns the size of this terminal's screen.
    #[inline(always)]
    pub fn screen_size(&self) -> ScreenSize {
        self.backend.screen_size()
    }

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

    /*
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
        let screen_width = self.screen_size().num_columns.0 as usize;

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
        let mut column_idx_of_unit = Column(0);
        let mut found_unit = None;
        // TODO: set the end bound of the iteration at the next soft line break if there is one, or if not, stick with `..`.
        for (i, unit) in (&line.units[unit_idx_at_cursor.0 ..]).iter().enumerate() {
            let width = Column(match unit.displayable_width() {
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
    */

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
}

struct TerminalParserHandler<'term, Backend: TerminalBackend> {
    // Note: we capture a mutable reference to each relevant field from the [`TextTerminal`] struct
    //       in order to work around Rust's inability to split multiple mutable borrows.
    scrollback_buffer: &'term mut ScrollbackBuffer,
    scrollback_cursor: &'term mut ScrollbackBufferPoint,
    cursor: &'term mut Cursor,
    backend: &'term mut Backend,
    tab_width: &'term mut u16,
    mode: &'term mut TerminalMode,
}

impl<'term, Backend: TerminalBackend> Perform for TerminalParserHandler<'term, Backend> {

    // The callback invoked when a single character is ready to be printed to the screen.
    fn print(&mut self, c: char) {
        debug!("[PRINT]: char: {:?}", c);

        // The parser treats the ASCII "DEL" (0x7F) as a printable char, but it's not.
        // We pass it along to the `execute` function, which handles non-printable terminal actions.
        if c == AsciiControlCodes::BackwardsDelete as char {
            return self.execute(AsciiControlCodes::BackwardsDelete);
        }

        let style = Style::default(); // TODO: keep track of the current style changeset, if any, and use it here.

        let screen_size = self.backend.screen_size();
        let tab_width = *self.tab_width;
        let orig_scrollback_pos = *self.scrollback_cursor;
        let dest_line = &mut self.scrollback_buffer[orig_scrollback_pos.line_idx];
        let new_unit_index = match self.mode.insert { 
            InsertMode::Insert => dest_line.insert_unit(
                orig_scrollback_pos.unit_idx,
                c,
                style,
                tab_width,
            ),
            InsertMode::Overwrite => dest_line.replace_unit(
                orig_scrollback_pos.unit_idx,
                c,
                style,
                tab_width,
            ),
        };
        self.scrollback_cursor.unit_idx = new_unit_index;


        // Now that we've handled inserting everything into the scrollback buffer,
        // we can move on to refreshing the display.
        let display_action = match self.mode.insert { 
            InsertMode::Insert => DisplayAction::Insert {
                scrollback_start: orig_scrollback_pos,
                scrollback_end:   *self.scrollback_cursor,
                screen_start:     self.cursor.position,
            },
            InsertMode::Overwrite => DisplayAction::Replace {
                scrollback_start: orig_scrollback_pos,
                scrollback_end:   *self.scrollback_cursor,
                screen_start:     self.cursor.position,
            },
        };
        let lines = &self.scrollback_buffer;
        let new_screen_cursor = self.backend.display(display_action, lines, None).unwrap();

        let (new_screen_cursor, scroll_action) = increment_screen_cursor(self.cursor.position, new_unit_width, screen_size);
        self.cursor.position = new_screen_cursor;

        // TODO: handle scroll_action appropriately
    }

    fn execute(&mut self, byte: u8) {
        debug!("[EXECUTE]: byte: {:#X} ({:?})", byte, byte as char);

        let screen_size = self.backend.screen_size();

        match byte {
            AsciiControlCodes::CarriageReturn => {
                self.carriage_return();
                if self.mode.cr_sends_lf == CarriageReturnSendsLineFeed::Yes {
                    self.line_feed();
                }
            }
            AsciiControlCodes::LineFeed | AsciiControlCodes::VerticalTab => {
                self.line_feed();
                if self.mode.lf_sends_cr == LineFeedSendsCarriageReturn::Yes {
                    self.carriage_return();
                }
            }
            AsciiControlCodes::Tab => self.print('\t'),
            AsciiControlCodes::Backspace => {
                // The backspace action simply moves the cursor back by one unit, 
                // without modifying the content or wrapping to the previous line.
                let wrap = WrapLine::No;
                let (intended_cursor_position, _scroll) = decrement_screen_cursor(self.cursor.position, Column(1), screen_size, wrap);
                // Only move the scrollback cursor if the screen cursor actually needs to move.
                if self.cursor.position != intended_cursor_position {
                    let new_cursor_position = self.backend.move_cursor_by(-1, 0);
                    assert_eq!(intended_cursor_position, new_cursor_position);
                    self.cursor.position = new_cursor_position;
                    *self.scrollback_cursor = decrement_scrollback_cursor(*self.scrollback_cursor, &*self.scrollback_buffer, wrap);
                }
            }
            AsciiControlCodes::BackwardsDelete => {
                // Delete the previous unit from the scrollback buffer
                let wrap = WrapLine::Yes;
                let orig_buffer_pos = *self.scrollback_cursor;
                *self.scrollback_cursor = decrement_scrollback_cursor(orig_buffer_pos, &self.scrollback_buffer, wrap);
                if orig_buffer_pos == *self.scrollback_cursor {
                    return;
                }

                let scrollback_cursor = *self.scrollback_cursor;
                let tab_width = *self.tab_width;
                let removed_unit_width = self.scrollback_buffer[scrollback_cursor.line_idx].delete_unit(
                    scrollback_cursor.unit_idx,
                    screen_size.num_columns,
                    tab_width,
                );

                let (ending_screen_cursor, _scroll_action) = decrement_screen_cursor(self.cursor.position, removed_unit_width, screen_size, wrap); 
                let display_action = DisplayAction::Delete {
                    screen_start: self.cursor.position,
                    screen_end:   ending_screen_cursor,
                };
                let screen_cursor_after_display = self.backend.display(display_action, &self.scrollback_buffer, None).unwrap();
                debug!("After BackwardsDelete, screen cursor moved from {:?} -> {:?}", self.cursor.position, screen_cursor_after_display);
                self.cursor.position = screen_cursor_after_display;
                warn!("Scrollback Buffer: {:?}", self.scrollback_buffer);
            }
            // Temp hack to handle Ctrl + C being pressed
            0x03 => {
                warn!("Note: QEMU is forwarding control sequences (like Ctrl+C) to Theseus. To exit QEMU, press Ctrl+A then X.");
            }
            _ => {
                debug!("[EXECUTE]: unhandled byte: {:#X}", byte);
                self.backend.write_bytes(&[byte]);
            }
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

    fn csi_dispatch(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, action: char) {
        debug!("[CSI_DISPATCH]: parameters: {:?}\n\t intermediates: {:X?}\n\t ignore?: {}, action: {:?}",
            _params, _intermediates, _ignore, action,
        );
        let screen_size = self.backend.screen_size();

        match action {
            '~' => {
                // TODO: forward delete was pressed, only if the params are '3'
                // Delete the current unit from the scrollback buffer
                let wrap = WrapLine::Yes;
                // Check that the scrollback_cursor is not at the end of a Line, as there'd be no Unit to delete.
                let scrollback_cursor = *self.scrollback_cursor;
                if scrollback_cursor.unit_idx.0 >= self.scrollback_buffer[scrollback_cursor.line_idx].len() {
                    return;
                }

                let tab_width = *self.tab_width;
                let removed_unit_width = self.scrollback_buffer[scrollback_cursor.line_idx].delete_unit(
                    scrollback_cursor.unit_idx,
                    screen_size.num_columns,
                    tab_width,
                );

                let current_screen_cursor = self.cursor.position;
                let (ending_screen_cursor, _scroll_action) = increment_screen_cursor(current_screen_cursor, removed_unit_width, screen_size); 
                let display_action = DisplayAction::Delete {
                    screen_start: current_screen_cursor,
                    screen_end:   ending_screen_cursor,
                };
                let screen_cursor_after_display = self.backend.display(display_action, &self.scrollback_buffer, None).unwrap();
            }
            'A' => {
                // TODO: up arrow was pressed
            }
            'B' => {
                // TODO: down arrow as pressed
            }
            'C' => {
                // TODO: right arrow was pressed
            }
            'D' => {
                // left arrow was pressed, move the cursor left by one unit.
                let wrap = WrapLine::Yes;
                let scrollback_cursor = *self.scrollback_cursor;
                let intended_scrollback_position = decrement_scrollback_cursor(scrollback_cursor, &*self.scrollback_buffer, wrap);
                // Only move the screen cursor if the scrollback cursor would actually move.
                if intended_scrollback_position != scrollback_cursor {
                    *self.scrollback_cursor = intended_scrollback_position;
                    // TODO: adjust the screen cursor if the scrollback cursor wrapped to the previous line, 
                    //       since it wouldn't necessarily be displayed in the last column.
                    if intended_scrollback_position.line_idx != scrollback_cursor.line_idx {
                        warn!("Left arrow: unimplemented support for adjusting wrapped screen cursor properly based on buffer contents");
                    }
                    let screen_cursor = self.cursor.position;
                    let (intended_cursor_position, _scroll) = decrement_screen_cursor(screen_cursor, Column(1), screen_size, wrap);
                    let new_cursor_position = self.backend.move_cursor_by(-1, 0);
                    assert_eq!(intended_cursor_position, new_cursor_position);
                    self.cursor.position = new_cursor_position;
                    debug!("Left arrow moved from:\n\t {:?} -> {:?}\n\t {:?} -> {:?}", scrollback_cursor, self.scrollback_cursor, screen_cursor, self.cursor.position);
                    warn!("Scrollback Buffer: {:?}", self.scrollback_buffer);
                }
            }

            _ => debug!("[CSI_DISPATCH] unhandled action: {}", action),
        }

    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {
        debug!("[ESC_DISPATCH]: intermediates: {:X?}\n\t ignore?: {}, byte: {:#X}",
            _intermediates, _ignore, _byte,
        );
    }
}

impl<'term, Backend: TerminalBackend> TerminalParserHandler<'term, Backend> {
    /// Moves the screen cursor to the next row and adjusts the scrollback cursor position
    /// to the `Unit` in the scrollback buffer at the corresponding position.
    ///
    /// The screen cursor's column position is not changed.
    ///
    /// If in Insert mode, a new `Line` will be inserted into the scrollback buffer,
    /// with the remainder of the `Line` being reflowed onto the next new `Line`.
    ///
    /// In either Insert or Overwrite mode, a new `Line` will be added if the screen cursor
    /// is already at the last displayable line of the scrollback buffer.
    fn line_feed(&mut self) {
        let screen_size = self.backend.screen_size();
        let original_screen_cursor = self.cursor.position;

        // Adjust the scrollback cursor position to the unit displayed one row beneath it
        if self.mode.insert == InsertMode::Overwrite {
            let unit_idx = self.scrollback_cursor.unit_idx;
            let line = &self.scrollback_buffer[self.scrollback_cursor.line_idx];
            let unit_idx_of_current_line_break = line.soft_line_breaks.iter()
                .skip_while(|&&soft_break| unit_idx < soft_break)
                .next();

            // If there is a soft line break after the current unit, then use that to calculate 
            // what the next unit is.
            if let Some(next_soft_lb) = unit_idx_of_current_line_break {
                // Iterate over all the units in this Line, starting from the next soft line break
                // until we reach the displayed width specified by the current screen cursor's column position.
                let mut width_so_far = 0;
                let mut units_iterated = 0;
                let mut previous_style = Style::default();
                let mut target_unit = None;
                for (i, unit) in (&line.units[next_soft_lb.0 ..]).iter().enumerate() {
                    previous_style = unit.style;
                    width_so_far += match unit.displayable_width() {
                        0 => *self.tab_width,
                        w => w,
                    };
                    if width_so_far >= original_screen_cursor.column.0 {
                        // Found the proper unit
                        target_unit = Some(unit);
                        break;
                    }
                    units_iterated += 1;
                }

                if let Some(t) = target_unit {
                    // We found a Unit that corresponds to the screen cursor's column.
                    self.scrollback_cursor.unit_idx = UnitIndex(units_iterated);
                    self.cursor.underneath = t.clone();
                } else {
                    // We didn't find a Unit that corresponds to the screen cursor's column.
                    // Thus, the line was too short to reach that column, so we set the unit index of
                    // the scrollback buffer such that it will be padded to that column point upon next print.
                    self.scrollback_cursor.unit_idx = *next_soft_lb + UnitIndex(units_iterated) + 
                        UnitIndex((original_screen_cursor.column.0 - width_so_far) as usize);
                    self.cursor.underneath = Unit {
                        character: Character::default(),
                        style: previous_style,
                    };
                }
            }
            // If there are no future soft line breaks, then we're at the end of a displayed line,
            // so we need to insert a new line into the scrollback_buffer.
            else {
                let next_line_idx = self.scrollback_cursor.line_idx + LineIndex(1);
                let previous_style = line.last().cloned().unwrap_or_default().style;
                self.scrollback_buffer.insert(next_line_idx.0, Line::new());
                // This is not a carriage return, we don't move the cursor to column 0.
                *self.scrollback_cursor = ScrollbackBufferPoint {
                    line_idx: next_line_idx,
                    unit_idx: UnitIndex(self.cursor.position.column.0 as usize),
                };
                self.cursor.underneath = Unit {
                    character: Character::default(),
                    style: previous_style,
                };
            }
        }
        else {
            // If in Insert mode, we need to split the line at the current unit,
            // insert a new line, and move the remainder of that Line's content into the new line.
            let curr_line = &mut self.scrollback_buffer[self.scrollback_cursor.line_idx];
            let new_line_units = curr_line.units.split_off(self.scrollback_cursor.unit_idx.0);
            let mut new_line = Line {
                units: new_line_units,
                soft_line_breaks: Vec::new(),
            };
            // TODO: we can recalculate curr_line soft breaks faster by deleting all of the ones greater than the current unit_idx.
            curr_line.recalculate_soft_line_breaks(screen_size.num_columns, *self.tab_width);
            new_line.recalculate_soft_line_breaks(screen_size.num_columns, *self.tab_width);

            // Insert the split-off new line into the scrollback buffer
            let next_line_idx = self.scrollback_cursor.line_idx + LineIndex(1);
            *self.scrollback_cursor = ScrollbackBufferPoint {
                line_idx: next_line_idx,
                unit_idx: UnitIndex(0),
            };
            self.scrollback_buffer.insert(next_line_idx.0, new_line);
            
            self.cursor.underneath = self.scrollback_buffer[*self.scrollback_cursor].clone();
            self.cursor.position.column.0 = 0;
        }

        // Actually move the screen cursor down to the next row.
        // The screen cursor's column has already been adjusted above.
        let scroll_action = {
            self.cursor.position.row.0 += 1;
            if self.cursor.position.row >= screen_size.num_rows {
                self.cursor.position.row = screen_size.num_rows - Row(1);
                ScrollAction::Down(1)
            } else {
                ScrollAction::None
            }
        };
        // TODO: process the `scroll_action`
        self.backend.move_cursor_to(self.cursor.position);
    }

    /// Moves the screen cursor back to the beginning of the current row
    /// and adjusts the scrollback cursor position to point to that corresponding Unit.
    ///
    /// Note that a carriage return alone does not move the screen cursor down to the next row,
    /// only a line feed (new line) can do that.
    fn carriage_return(&mut self) {
        let unit_idx = self.scrollback_cursor.unit_idx;
        let line = &self.scrollback_buffer[self.scrollback_cursor.line_idx];
        let idx_of_preceding_line_break = line.soft_line_breaks.iter()
            .rfind(|&&soft_break| unit_idx >= soft_break)
            .cloned()
            .unwrap_or_default(); // No soft line breaks --> cursor goes to the beginning of the line.
        
        debug!("carriage_return: setting scrollback buffer at {:?} from {:?} to {:?}",
            self.scrollback_cursor.line_idx, self.scrollback_cursor.unit_idx, idx_of_preceding_line_break
        );
        self.scrollback_cursor.unit_idx = idx_of_preceding_line_break;

        // Move the screen cursor to the beginning of the current row.
        self.cursor.position.column = Column(0);
        self.backend.move_cursor_to(self.cursor.position);
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
#[derive(Clone, Debug, PartialEq, Eq)]
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
/// ## 1-to-1 Relationship between Units and Columns
/// It is guaranteed that one `Unit` in the scrollback buffer corresponds to exactly one screen column,
/// which makes it easy to calculate the conversions between screen cursor coordinate points
/// and scrollback buffer coordinate points.
/// For example, if the screen is 80 columns wide, a Line with 120 units will display across
/// exactly 1.5 lines, displaying right up to the 40th column of the second row.
///
/// Because a complex Unicode character may require more than one column to display,
/// such as a tab or emoji sequence, there are flags in a `Unit` that indicate whether it is 
/// part of a wider display character sequence.
/// There are flags for both the beginning unit and all of the placeholder units that follow it 
/// (which exist solely to satisfy the 1-to-1 relationship between screen columns and scrollback buffer units).
/// Thus, it is easy to determine where multi-column Unit sequences start and end.
///
/// Wide-display character sequences like tabs and emoji are **always** stored completely
/// in the starting Unit (as a [`Character::Multi`] variant),
/// with the following placeholder Units containing a default empty [`Character::Single`]
/// with the null character within it.
/// This conveniently allows the screen cursor to store a single `Unit` object within it
/// that represents the entirety of that displayable Unit,
/// instead of more complex storage strategy that splits up wide character sequences into
/// multiple `Unit`s.
///
///
/// ## What Units are Not
/// Displayable control/escape sequences, i.e., those that affect text style,
/// **do not** exist as individual `Unit`s,
/// though their effects on text style are represented by a `Unit`'s `FormatFlags`.
/// 
/// Non-displayable control/escape sequences, i.e., bells, backspace, delete, etc,
/// are **NOT** saved as `Unit`s in the terminal's scrollback buffer,
/// as they cannot be displayed and are simply transient actions.
#[derive(Clone, Debug, Default)]
pub struct Unit {
    /// The displayable character(s) held in this `Unit`.
    character: Character,
    /// The style/formatting with which this `Unit`s character(s) should be displayed.
    style: Style,
    /// Indicates if and how this `Unit` is part of a wide-displayed character sequence.
    wide: WideDisplayedUnit,
}
impl Deref for Unit {
    type Target = Character;
    fn deref(&self) -> &Self::Target {
        &self.character
    }
}

/// The size of a terminal screen, expressed as the
/// number of columns (x dimension) by the number of rows (y dimension).
///
/// The default screen size is 80 columns by 25 rows.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ScreenSize {
    /// The width of the screen viewport in number of columns (x dimension).
    pub num_columns: Column,
    /// The height of the screen viewport in number of rows (y dimension).
    pub num_rows: Row,
}
impl Default for ScreenSize {
    fn default() -> Self {
        ScreenSize {
            num_columns: Column(80),
            num_rows: Row(25),
        }
    }
}

/// A 2D position value that represents a point on the screen,
/// in which `(0, 0)` represents the top-left corner.
/// Thus, a valid `ScreenPoint` must fit be the bounds of 
/// the current [`ScreenSize`].
#[derive(Copy, Clone, Default, PartialEq, Eq, Ord)]
#[derive(Add, AddAssign, Sub, SubAssign)]
pub struct ScreenPoint {
    column: Column,
    row: Row,
} 
impl PartialOrd for ScreenPoint {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        if self.row == other.row {
            self.column.partial_cmp(&other.column)
        } else if self.row < other.row {
            Some(Ordering::Less)
        } else {
            Some(Ordering::Greater)
        }
    }
}
impl fmt::Debug for ScreenPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({:?}, {:?})", self.column, self.row)
    }
}

/// A row index or number of rows in the y-dimension of the screen viewport. 
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[derive(Add, AddAssign, Sub, SubAssign)]
pub struct Row(u16);
/// A column index or number of columns in the x-dimension of the screen viewport. 
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[derive(Add, AddAssign, Sub, SubAssign)]
pub struct Column(u16);


/// A 2D position value that represents a point in the scrollback buffer,
/// in which `(0, 0)` represents the `Unit` at the first column of the first line.
#[derive(Copy, Clone, Default, PartialEq, Eq, Ord)]
#[derive(Add, AddAssign, Sub, SubAssign)]
pub struct ScrollbackBufferPoint {
    unit_idx: UnitIndex,
    line_idx: LineIndex,
}
impl PartialOrd for ScrollbackBufferPoint {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        if self.line_idx == other.line_idx {
            self.unit_idx.partial_cmp(&other.unit_idx)
        } else if self.line_idx < other.line_idx {
            Some(Ordering::Less)
        } else {
            Some(Ordering::Greater)
        }
    }
}
impl fmt::Debug for ScrollbackBufferPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({:?}, {:?})", self.unit_idx, self.line_idx)
    }
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
    position: ScreenPoint,
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



/// Advances the cursor's screen coordinate forward by one unit of the given width.
/// This does not modify the cursor's scrollback buffer position.
///
/// Returns a tuple of the cursor's new screen position
/// and a `ScrollAction` describing what kind of scrolling action needs to be taken
/// to handle this screen cursor movement.
fn increment_screen_cursor(
    mut cursor_position: ScreenPoint,
    unit_width: Column,
    screen_size: ScreenSize,
    // TODO: add wrapping support
) -> (ScreenPoint, ScrollAction) {
    cursor_position.column += unit_width;
    if cursor_position.column >= screen_size.num_columns {
        cursor_position.column.0 %= screen_size.num_columns.0;
        cursor_position.row.0 += 1;
        if cursor_position.row >= screen_size.num_rows {
            cursor_position.row.0 = screen_size.num_rows.0 - 1;
            return (cursor_position, ScrollAction::Down(1));
        }
    }
    (cursor_position, ScrollAction::None)
}

/// Moves the cursor's screen coordinate backward by one unit of the given width.
/// This does not modify the cursor's scrollback buffer position.
///
/// Returns a tuple of the cursor's new screen position
/// and a `ScrollAction` describing what kind of scrolling action needs to be taken
/// to handle this screen cursor movement.
fn decrement_screen_cursor(
    mut cursor_position: ScreenPoint,
    num_columns: Column,
    screen_size: ScreenSize,
    wrap: WrapLine,
) -> (ScreenPoint, ScrollAction) {
    if wrap == WrapLine::No {
        cursor_position.column.0 = cursor_position.column.0.saturating_sub(num_columns.0);
        return (cursor_position, ScrollAction::None);
    }

    let new_col = cursor_position.column.0 as i32 - num_columns.0 as i32;
    if new_col < 0 {
        cursor_position.column.0 = (new_col + screen_size.num_columns.0 as i32) as u16;
        if cursor_position.row.0 == 0 {
            return (cursor_position, ScrollAction::Up(1));
        } else {
            cursor_position.row.0 -= 1;
        }
    } else {
        cursor_position.column.0 = new_col as u16;
    }
    (cursor_position, ScrollAction::None)
}


// /// Returns the position of the given `scrollback_cursor` moved forward by one unit.
// ///
// /// This is a pure calculation that does not modify any cursor positions.
// fn increment_scrollback_cursor(
//     mut scrollback_cursor: ScrollbackBufferPoint,
//     scrollback_buffer: &ScrollbackBuffer,
//     wrap_lines: WrapLine,
// ) -> ScrollbackBufferPoint {
//     let line_length = UnitIndex(scrollback_buffer[scrollback_cursor.line_idx].len());

//     if wrap_lines == WrapLine::No {
//         scrollback_cursor.unit_idx = min(
//             UnitIndex(scrollback_cursor.unit_idx.0.saturating_add(1)),
//             line_length,
//         );
//         return scrollback_cursor;
//     }

//     if scrollback_cursor.unit_idx < line_length {
//         scrollback_cursor.unit_idx += UnitIndex(1);
//     } else {
//         let next_line = scrollback_cursor.line_idx.0 + 1;
//         if next_line < scrollback_buffer.len() {
//             // wrap forwards to the beginning of the next line
//             scrollback_cursor.unit_idx = UnitIndex(0);
//             scrollback_cursor.line_idx = LineIndex(next_line);
//         }
//     }
//     scrollback_cursor
// }


/// Returns the position of the given `scrollback_cursor` moved backward by one unit.
///
/// This is a pure calculation that does not modify any cursor positions.
fn decrement_scrollback_cursor(
    mut scrollback_cursor: ScrollbackBufferPoint,
    scrollback_buffer: &ScrollbackBuffer,
    wrap_lines: WrapLine,
) -> ScrollbackBufferPoint {
    if wrap_lines == WrapLine::No {
        scrollback_cursor.unit_idx.0 = scrollback_cursor.unit_idx.0.saturating_sub(1);
        return scrollback_cursor;
    }

    if scrollback_cursor.unit_idx == UnitIndex(0) {
        if scrollback_cursor.line_idx > LineIndex(0) {
            // wrap backwards to the end of the previous line
            scrollback_cursor.line_idx -= LineIndex(1);
            scrollback_cursor.unit_idx.0 = scrollback_buffer[scrollback_cursor.line_idx].len();
        }
    } else {
        scrollback_cursor.unit_idx -= UnitIndex(1);
    }
    scrollback_cursor
}


pub trait TerminalBackend {
    /// The Error type returned by the [`TerminalBackend::display()`] function
    /// if it returns a [`Result::Err`] variant.
    type DisplayError: fmt::Debug;

    /// Returns the screen size of the terminal.
    fn screen_size(&self) -> ScreenSize;

    /// Resizes the terminal screen.
    /// TODO: perform a full reflow of the contents currently displayed on screen.
    fn update_screen_size(&mut self, new_size: ScreenSize);

    /// Displays the given range of `Unit`s in the scrollback buffer
    /// by writing them to this terminal's backend.
    ///
    /// The `Unit` at the `scrollback_start` point will be displayed at `screen_start`,
    /// and all `Unit`s up until the given `scrollback_end` point will be written to
    /// successive points on the screen.
    ///
    /// Returns the new position of the screen cursor.
    fn display(
        &mut self,
        display_action: DisplayAction,
        scrollback_buffer: &ScrollbackBuffer,
        previous_style: Option<Style>,
    ) -> Result<ScreenPoint, Self::DisplayError>;

    /// Moves the on-screen cursor to the given position.
    ///
    /// The cursor's position will be clipped (not wrapped) to the actual size
    /// of the screen, in both the column (x) the row (y) dimensions.
    ///
    /// Returns the new position of the on-screen cursor.
    fn move_cursor_to(&mut self, new_position: ScreenPoint) -> ScreenPoint;

    /// Moves the on-screen cursor by the given number of rows and columns,
    /// in which a value of `0` indicates no movement in that dimension.
    ///
    /// The cursor's position will be clipped (not wrapped) to the actual size
    /// of the screen, in both the column (x) the row (y) dimensions.
    ///
    /// Returns the new position of the on-screen cursor.
    #[must_use]
    fn move_cursor_by(&mut self, num_columns: i32, num_rows: i32) -> ScreenPoint;

    /// TODO: change this to support any arbitrary terminal mode
    fn set_insert_mode(&mut self, mode: InsertMode);

    /// Fully reset the terminal screen to its initial default state.
    fn reset_screen(&mut self);

    /// Clears the entire terminal screen.
    fn clear_screen(&mut self);

    /// A temporary hack to allow direct writing to the backend's output stream.
    /// This is only relevant for TtyBackends.
    fn write_bytes(&mut self, bytes: &[u8]);
}


/// A terminal backend that is simply a character device TTY endpoint 
/// (a full terminal emulator) on the other side,
/// which only allows writing a stream of bytes to it.
///
/// A TTY backend doesn't support any form of random access or direct text rendering, 
/// so we can only issue regular ANSI/xterm control and escape sequences to it.
pub struct TtyBackend<Output: bare_io::Write> {
    /// The width and height of this terminal's screen.
    screen_size: ScreenSize,

    /// The actual position of the cursor on the real terminal backend screen.
    real_screen_cursor: ScreenPoint,

    /// The output stream to which bytes are written,
    /// which will be read by a TTY.terminal emulator on the other side of the stream.
    output: Output,

    insert_mode: InsertMode,
}
impl<Output: bare_io::Write> TtyBackend<Output> {
    // const FORWARDS_DELETE: &'static [u8] = &[
    //     AsciiControlCodes::Escape,
    //     b'[',
    //     b'3',
    //     b'~',
    // ];
    const ERASE_CHARACTER: &'static [u8] = &[
        AsciiControlCodes::Escape,
        b'[',
        b'1',
        b'X',
    ];
    const DELETE_CHARACTER: &'static [u8] = &[
        AsciiControlCodes::Escape,
        b'[',
        b'1',
        b'P',
    ];


    pub fn new(
        screen_size: Option<ScreenSize>,
        output_stream: Output,
    ) -> TtyBackend<Output> {
        TtyBackend {
            screen_size: screen_size.unwrap_or_default(),
            real_screen_cursor: ScreenPoint::default(),
            output: output_stream,
            insert_mode: InsertMode::Overwrite,
        }
        // TODO: here, query the backend for the real cursor location,
        //       which could be anywhere, e.g., if we connected to an existing terminal.
        //       For now we just assume it's at the origin point of `(0,0)`.
    }
    

    /// Deletes the contents on screen from the given `screen_start` point (inclusive) 
    /// to the given `screen_end` point (exclusive).
    fn delete(&mut self, screen_start: ScreenPoint, screen_end: ScreenPoint) -> ScreenPoint {
        let forward_delete = screen_start < screen_end;
        debug!("Deleting {} from {:?} to {:?}", if forward_delete { "forwards" } else { "backwards" }, screen_start, screen_end);
        let wrap = WrapLine::Yes;

        if screen_start.row != screen_end.row {
            todo!("TtyBackend::delete() doesn't yet support multiple rows");
        }
        
        // TODO: move the cursor to `screen_start`
        if self.real_screen_cursor != screen_start {
            warn!("TtyBackend::delete(): Skipping required screen cursor movement from {:?} to {:?}", self.real_screen_cursor, screen_start);
        }

        let mut current = screen_start;

        if forward_delete {
            while current < screen_end {
                // Forward-delete the current character unit, but do not move the real_screen_cursor, 
                // because the backend terminal emulator will shift everything in the current line to the left.
                self.output.write(Self::DELETE_CHARACTER).unwrap();
                current = increment_screen_cursor(current, Column(1), self.screen_size /* , wrap */).0;
            }
        } 
        else {
            while current > screen_end {
                // Backward-delete a character by moving the previous character unit
                // and then issuing a delete command, upon which the backend terminal emulator 
                // will shift everything in the current line to the left.
                let (new_screen_cursor, _scroll) = decrement_screen_cursor(self.real_screen_cursor , Column(1), self.screen_size, wrap);
                // self.real_screen_cursor = new_screen_cursor;
                
                let actual_screen_cursor = self.move_cursor_by(-1, 0);
                self.output.write(Self::DELETE_CHARACTER).unwrap();
                current = decrement_screen_cursor(current, Column(1), self.screen_size, wrap).0;
            }
        }

        self.real_screen_cursor
    }

    /// Sets the cursor position directly using a `(1,1)`-based coordinate system.
    ///
    /// This is needed because terminal backends use a different coordinate system than we do,
    /// in which the origin point at the upper-left corner is `(1,1)`,
    /// instead of our coordinate system of an origin at `(0,0)`. 
    fn set_cursor_internal(&mut self, cursor: ScreenPoint) {
        write!(&mut self.output,
            "\x1B[{};{}H", 
            cursor.row.0 + 1,
            cursor.column.0 + 1,
        ).unwrap();
        self.real_screen_cursor = cursor;
    }
}
impl<Output: bare_io::Write> TerminalBackend for TtyBackend<Output> {
    type DisplayError = bare_io::Error;

    #[inline(always)]
    fn screen_size(&self) -> ScreenSize {
        self.screen_size
    }

    fn update_screen_size(&mut self, new_size: ScreenSize) {
        self.screen_size = new_size;
        warn!("NOTE: reflow upon a screen size update is not yet implemented");
    }

    fn display(
        &mut self,
        display_action: DisplayAction,
        scrollback_buffer: &ScrollbackBuffer,
        previous_style: Option<Style>,
    ) -> Result<ScreenPoint, Self::DisplayError> {
        // debug!("DisplayAction::{:?}\nScrollback Buffer: {:?}", display_action, scrollback_buffer);

        let mut char_encode_buf = [0u8; 4];
        let mut bytes_written = 0;
        let mut previous_style = previous_style.unwrap_or_default();

        let (scrollback_start, scrollback_end, screen_start) = match display_action {
            DisplayAction::Insert { scrollback_start, scrollback_end, screen_start } |
            DisplayAction::Replace { scrollback_start, scrollback_end, screen_start } => {
                (scrollback_start, scrollback_end, screen_start)
            }
            DisplayAction::Delete { screen_start, screen_end } => {
                return Ok(self.delete(screen_start, screen_end));
            }
        };

        if self.real_screen_cursor != screen_start {
            warn!("Unimplemented: need to move screen cursor from {:?} to {:?}", self.real_screen_cursor, screen_start);
            // TODO: issue a command to move the screen cursor to `screen_start`
        }

        // For now, we just assume that the backend output stream is a linear "file"
        // that can't adjust its position, so we just write directly to it 
        // whilst ignoring the `screen_start` parameter.
        let mut start_unit = scrollback_start.unit_idx; 
        for line_idx in scrollback_start.line_idx.0 ..= scrollback_end.line_idx.0 {
            let line_idx = LineIndex(line_idx);
            let line = &scrollback_buffer[line_idx];
            
            // Write the requested part of this line, up to the entire line.
            let end = if scrollback_end.line_idx == line_idx {
                scrollback_end.unit_idx.0
            } else {
                line.units.len() - 1
            };
            
            // debug!("Looking at line {}, units {}..{}: {:?}", line_idx.0, start_unit.0, end, line);

            for unit in &line.units[start_unit.0 .. end] {
                // First, write out the escape sequences for the difference in style.
                if unit.style != previous_style {
                    let mut diff_iter = unit.style.diff(&previous_style);
                    // Only write out the escape sequences if there is at least one style difference.
                    if let Some(first_code) = diff_iter.next() {
                        bytes_written += self.output.write(AnsiStyleCodes::ESCAPE_PREFIX)?;
                        bytes_written += self.output.write(first_code.to_escape_code().as_bytes())?;
                        for code in diff_iter {
                            bytes_written += self.output.write(AnsiStyleCodes::ESCAPE_DELIM)?;
                            bytes_written += self.output.write(code.to_escape_code().as_bytes())?;
                        }
                        bytes_written += self.output.write(AnsiStyleCodes::ESCAPE_SUFFIX)?;
                    }
                }
                previous_style = unit.style;
    
                // Second, write out the actual character(s).
                bytes_written += self.output.write(match unit.character {
                    Character::Single(ref ch) => ch.encode_utf8(&mut char_encode_buf[..]).as_bytes(),
                    Character::Multi(ref s) => s.as_bytes(),
                })?;

                // Adjust the screen cursor based on what we just printed to the screen.
                let unit_width = match unit.displayable_width() {
                    0 => 4, // TODO: use tab_width
                    w => w,
                };
                let (new_screen_cursor, _scroll_action) = increment_screen_cursor(self.real_screen_cursor, Column(unit_width), self.screen_size);
                self.real_screen_cursor = new_screen_cursor;
            }
            // If we wrote out the entire `Line`, write out a newline character.
            if line_idx < scrollback_end.line_idx {
                bytes_written += self.output.write(b"\n")?;
                // TODO: test for scroll action needed.
                self.real_screen_cursor.row.0 += 1;
                self.real_screen_cursor.column.0 = 0;
            } 

            start_unit = UnitIndex(0);
        }

        Ok(self.real_screen_cursor) 
    }

    fn move_cursor_to(&mut self, new_position: ScreenPoint) -> ScreenPoint {
        let cursor_bounded = ScreenPoint {
            column: min(new_position.column, self.screen_size.num_columns),
            row:    min(new_position.row,    self.screen_size.num_rows),
        };
        self.set_cursor_internal(cursor_bounded);
        self.real_screen_cursor
    }

    fn move_cursor_by(
        &mut self,
        num_cols: i32,
        num_rows: i32,
    ) -> ScreenPoint {
        let new_col = self.real_screen_cursor.column.0 as i32 + num_cols;
        let col_bounded = if new_col <= 0 {
            0
        } else if new_col >= self.screen_size.num_columns.0 as i32 {
            self.screen_size.num_columns.0 - 1
        } else {
            new_col as u16
        };

        let new_row = self.real_screen_cursor.row.0 as i32 + num_rows;
        let row_bounded = if new_row <= 0 {
            0
        } else if new_row >= self.screen_size.num_rows.0 as i32 {
            self.screen_size.num_rows.0 - 1
        } else {
            new_row as u16
        };

        let cursor_bounded = ScreenPoint {
            column: Column(col_bounded),
            row:    Row(row_bounded),
        };
        debug!("move_cursor_by({},{}): moving cursor to {:?}", num_cols, num_rows, cursor_bounded);
        self.set_cursor_internal(cursor_bounded);
        self.real_screen_cursor
    }

    fn set_insert_mode(&mut self, mode: InsertMode) {
        if self.insert_mode != mode {
            self.output.write(&[
                AsciiControlCodes::Escape,
                b'[',
                ModeSwitch::InsertMode,
                match mode {
                    InsertMode::Insert => ModeSwitch::SET_SUFFIX,
                    InsertMode::Overwrite => ModeSwitch::RESET_SUFFIX,
                },
            ]).expect("failed to write bytes for insert mode");
            self.insert_mode = mode;
        }
    }

    fn reset_screen(&mut self) {
        self.real_screen_cursor = Default::default();
        self.output.write(&[
            AsciiControlCodes::Escape,
            b'c',
        ]).expect("failed to write bytes for reset screen");
    }

    fn clear_screen(&mut self) {
        self.real_screen_cursor = Default::default();
        self.output.write(&[
            AsciiControlCodes::Escape,
            b'[',
            b'2',
            b'J',
        ]).expect("failed to write bytes for clear screen");
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        self.output.write(bytes).unwrap();
    }
}



/// A pending action to display content from the terminal's scrollback buffer on the screen.
///
/// See the [`TerminalBackend::display()`] for more information on how this type is used.
#[must_use = "`DisplayAction`s must be used to ensure the display action 
is actually handled and processed."]
#[derive(Debug)]
pub enum DisplayAction {
    /// Delete the contents displayed on the screen in the given range of on-screen coordinates,
    /// setting the units to blank space of the default style.
    ///
    /// A delete action can occur in both the forwards and backwards direction:
    /// * If `screen_start` is less than `screen_end`, a forward delete should be performed.
    /// * If `screen_start` is greater than `screen_end`, a backwards delete should be performed.
    ///
    /// The direction of the delete action matters for the following reasons:
    /// * A [`TerminalBackend`] may use it to optimize which action occurs, and
    /// * It also dictates where the screen cursor will end up after the delete action occurs.
    ///
    /// For example, if a user presses the "Backspace" key, they expect a backwards deletion
    /// in which the cursor is moved backwards to the previous unit and that unit is deleted.
    /// If they press the "Delete" key, they expect a forwards deletion
    /// in which the cursor is unchanged and the current unit is deleted.
    ///
    /// The `screen_start` bound is inclusive; the `screen_end` bound is exclusive.
    Delete {
        screen_start: ScreenPoint,
        screen_end:   ScreenPoint,
    },
    /// Replace the contents displayed on the screen starting at the given on-screen coordinate
    /// with the contents of the scrollback buffer.
    ///
    /// The `scrollback_start` bound is inclusive; the `scrollback_end` bound is exclusive;
    /// the `screen_start` bound is also inclusive.
    Replace {
        scrollback_start: ScrollbackBufferPoint,
        scrollback_end:   ScrollbackBufferPoint,
        screen_start:     ScreenPoint,
    },
    /// Inserts the content from the given range in the scrollback buffer
    /// into the screen, starting at the given on-screen coordinate.
    /// After the content from the scrollback buffer is inserted,
    /// all other content currently on the screen will be shifted to the right
    /// and reflowed such that nothing else is lost. 
    ///
    /// The `scrollback_start` bound is inclusive; the `scrollback_end` bound is exclusive;
    /// the `screen_start` bound is also inclusive.
    Insert {
        scrollback_start: ScrollbackBufferPoint,
        scrollback_end:   ScrollbackBufferPoint,
        screen_start:     ScreenPoint,
    },
}
// impl Drop for DisplayAction {
//     fn drop(&mut self) {
//         warn!("{:?} was dropped without being handled!", self);
//     }
    
// }

/// A pending action to scroll the screen up or down by a number of rows.
#[must_use = "`ScrollAction`s must be used to ensure the scroll action 
is actually handled and processed."]
#[derive(Debug)]
pub enum ScrollAction {
    /// Do nothing, do not scroll the screen.
    None,
    /// Scroll the screen up by the included number of lines.
    Up(usize),
    /// Scroll the screen down by the included number of lines.
    Down(usize),
}

// impl Drop for ScrollAction {
//     fn drop(&mut self) {
//         match self {
//             Self::None => { }
//             _ => warn!("{:?} was dropped without being handled!", self),
//         }
//     }
// }


/// Whether or not to wrap text to the next line/row when it extends
/// past the column limit of the screen.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WrapLine {
    Yes,
    No,
}

/// Whether text characters printed to the terminal will be inserted
/// before other characters or will replace/overwrite existing characters.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InsertMode {
    /// Characters will be inserted at the current cursor,
    /// preserving all existing characters by shifting them to the right.
    Insert,
    /// Characters will be overwritten in place.
    /// Sometimes called "replace mode".
    Overwrite,
}

/// Whether the screen cursor is visible.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ShowCursor {
    Visible,
    Hidden,
}

/// Whether a Carriage Return subsequently issues a Line Feed (newline / new line).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CarriageReturnSendsLineFeed {
    Yes,
    No,
}

/// Whether a Line Feed (newline / new line) subsequently issues a Carriage Return.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LineFeedSendsCarriageReturn {
    Yes,
    No,
}

/// The set of options that determine terminal behavior.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TerminalMode {
    insert:      InsertMode,
    show_cursor: ShowCursor,
    cr_sends_lf: CarriageReturnSendsLineFeed,
    lf_sends_cr: LineFeedSendsCarriageReturn,
}
impl Default for TerminalMode {
    fn default() -> Self {
        TerminalMode {
            insert: InsertMode::Overwrite,
            show_cursor: ShowCursor::Visible,
            cr_sends_lf: CarriageReturnSendsLineFeed::Yes,
            lf_sends_cr: LineFeedSendsCarriageReturn::Yes,
        }
    }
}

/// The kinds of Units and how they correspond to previous Units in the same Line.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum WideDisplayedUnit {
    /// (Default) This Unit is not part of any wide-displayed character sequence,
    /// and is completely standlone with no relationship to other adjacent Units.
    /// Thus, the character sequence in this `Unit` is guaranteed to display within one Column.
    None,
    /// This Unit is the beginning of a tab character.
    /// As such, this unit contains a [`Character::Single`]`('\t')`.
    TabStart,
    /// This Unit is a continuance for a previously-existing tab character.
    TabFill,
    /// This Unit is the beginning of a wide non-tab character, e.g., emoji, Chinese character, etc.
    /// As such, this unit contains a [`Character::Multi`]`(String)` that holds the entire
    /// wide character's multi-byte sequence.
    MultiStart,
    /// This Unit is a continuance for a previously-existing wide non-tab character
    ///, e.g., emoji, Chinese character, etc.
    MultiFill,
}
impl Default for WideDisplayedUnit {
    fn default() -> Self {
        WideDisplayedUnit::None
    }
}
impl WideDisplayedUnit {
    /// A continuance Unit is one that occupies space as a continuance
    /// of a previous character Unit, i.e., `TabFill` or `MultiFill`.
    #[inline(always)]
    fn is_continuance(&self) -> bool {
        match self {
            &Self::TabFill | &Self::MultiFill => true,
            _ => false,
        }
    }
}
