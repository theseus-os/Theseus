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
extern crate core2;
extern crate vte;
extern crate util;
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
use core2::io::{Read, Write};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use vte::{Parser, Perform};


/// The position ("viewport") that the terminal is currently scrolled to. 
/// 
/// By default, the terminal starts at the `Bottom`, 
/// such that it will auto-scroll upon new characters being displayed.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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
    /// In this position, the terminal screen "viewport" is locked
    /// and will **NOT** auto-scroll down to show any newly-outputted lines of text.
    AtUnit(ScrollbackBufferPoint),
    /// The terminal position is scrolled all the way down.
    ///
    /// In this position, the terminal screen "viewport" is **NOT** locked
    /// and will auto-scroll down to show any newly-outputted lines of text.
    ///
    /// For convenience in calculating the screen viewport,
    /// the contained fields are the same as in the `AtUnit` variant.
    ///
    /// In this mode, the contained point must be updated whenever the screen is 
    /// scrolled down by virtue of a new line being displayed at the bottom.
    /// the screen viewport is scrolled up or down.
    Bottom(ScrollbackBufferPoint),
}
impl Default for ScrollPosition {
    fn default() -> Self {
        ScrollPosition::Bottom(ScrollbackBufferPoint::default())
    }
}
impl ScrollPosition {
    /// Returns the `ScrollbackBufferPoint` at which the screen viewport starts,
    /// i.e., the coordinate in the scrollback buffer that maps to `ScreenPoint(0, 0)`. 
    fn start_point(&self) -> ScrollbackBufferPoint {
        match self {
            ScrollPosition::Top => ScrollbackBufferPoint::default(), // (0,0)
            ScrollPosition::AtUnit(point) => *point,
            ScrollPosition::Bottom(point) => *point,
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

    /// Returns the `UnitIndex` of the last `Unit` in this `Line`,
    /// effectively the length of this `Line` minus 1.
    #[inline(always)]
    fn last_unit(&self) -> UnitIndex {
        UnitIndex(self.units.len().saturating_sub(1))
    }

    /// Iterates over the `Unit`s in this `Line` to find the next non-continuance `Unit`.
    ///
    /// Returns the `UnitIndex` of that `Unit`.
    #[inline(always)]
    fn next_non_continuance_unit(&self, mut index: UnitIndex) -> UnitIndex {
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
    fn previous_non_continuance_unit(&self, mut index: UnitIndex) -> UnitIndex {
        while self.units.get(index.0).map(|u| u.wide.is_continuance()).unwrap_or_default() {
            warn!("Untested: backward skipping continuance Unit at {:?}", index);
            index -= UnitIndex(1);
        }
        index
    }

    /// Inserts the given character `c` with the given `Style` into this `Line` at the given `index`. 
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
        let index = self.next_non_continuance_unit(index);

        // If the index is beyond the existing bounds of this Line, fill it with empty Units as padding.
        if index.0 > self.units.len() {
            let range_of_empty_padding = self.units.len() .. index.0;
            let num_padding_units = range_of_empty_padding.len();
            self.units.reserve(num_padding_units + 1);
            if num_padding_units > 0 {
                warn!("Untested scenario: pushing {} empty padding character(s) to line.", num_padding_units);
            }
            for _i in range_of_empty_padding {
                self.units.push(Unit { style, ..Default::default() });
            }
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
    /// Returns a tuple of:
    /// 1. The `UnitIndex` immediately following the newly-replaced `Unit`(s),
    ///    which is where the next `Unit` would be replaced on a future operation.
    /// 2. The difference in widths when replacing the old unit(s) with the new unit(s),
    ///    i.e., `new_unit_width - old_unit_width`.
    ///    If `0`, the units are the same width or there is no existing `Unit` to replace.
    ///    If positive, the new unit is wider than the old unit.
    ///    If negative, the old unit is wider than the new unit.
    fn replace_unit(
        &mut self,
        index: UnitIndex,
        c: char,
        style: Style,
        tab_width: u16
    ) -> (UnitIndex, i32) {
        let index = self.next_non_continuance_unit(index);

        if let Some(unit_to_replace) = self.units.get_mut(index.0) {
            let character = Character::Single(c);
            let old_width = unit_to_replace.displayable_width();
            let new_width = character.displayable_width();
            let width_diff = new_width as i32 - old_width as i32;
            if old_width == new_width {
                unit_to_replace.character = character;
                unit_to_replace.style = style;
                let next_unit_index = self.next_non_continuance_unit(index + UnitIndex(1));
                (next_unit_index, width_diff)
            } else {
                // To handle the case when a new character has a different displayable width 
                // than the existing character it's replacing,
                // we simply remove the existing character's `Unit`(s) 
                // and then insert the new character. 
                self.delete_unit(index);
                let new_unit_index = self.insert_unit(index, c, style, tab_width);
                (new_unit_index, width_diff)
            }
        } else {
            // We're past the bounds of this line, so we insert a new unit at the end.
            let new_unit_index = self.insert_unit(index, c, style, tab_width);
            (new_unit_index, 0)
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
    ///
    /// Returns the number of units that were actually deleted (including continuance units).
    fn delete_unit(&mut self, index: UnitIndex) -> usize {
        let index = self.previous_non_continuance_unit(index);

        // TODO: if the scrollback cursor is at the end of the line,
        //       merge the next line into the current line.

        let _removed_unit = self.units.remove(index.0);
        let _removed_continuance_units = self.units.drain_filter(|unit| unit.wide.is_continuance());

        let num_removed_continuance_units = if false {
            _removed_continuance_units.count()
        } else {
            let mut count = 0;
            warn!("Deleted {}-width {:?}", _removed_unit.displayable_width(), _removed_unit);
            for _u in _removed_continuance_units {
                count += 1;
                warn!("\t also deleted continuance {:?}", _u);
            }
            count
        };

        1 + num_removed_continuance_units
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
    screen_cursor: ScreenCursor,

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
impl ScrollbackBuffer {
    /// Returns the `LineIndex` of the last `Line` in this `ScrollbackBuffer`.
    #[inline(always)]
    fn last_line(&self) -> LineIndex {
        LineIndex(self.0.len().saturating_sub(1))
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
            screen_cursor: ScreenCursor::default(),
            mode: TerminalMode::default(),
            backend,
            parser: Parser::new(),
        };

        // Clear the terminal backend upon start.
        terminal.backend.clear_screen();
        terminal.screen_cursor.position = terminal.backend.move_cursor_to(ScreenPoint::default());

        // By default, terminal backends typically operate in Overwrite mode, not Insert mode.
        let insert_mode = InsertMode::Overwrite;
        terminal.backend.set_insert_mode(insert_mode);
        terminal.mode.insert = insert_mode;

        // let welcome = "Welcome to Theseus's text terminal! This is a long string that should overflow lines blah blah 12345";
        let welcome = "Welcome to Theseus's text terminal! This is a long string that should overflow lines blah blah\nTesting a new line here";
        terminal.handle_input(&mut welcome.as_bytes()).expect("failed to write terminal welcome message");

        // TODO: issue a term info command to the terminal backend
        //       to obtain its size, and then resize this new `terminal` accordingly

        terminal
    }

    /// Pulls as many bytes as possible from the given [`Read`]er
    /// and handles that stream of bytes as input into this terminal.
    ///
    /// Returns the number of bytes read from the given reader.
    pub fn handle_input<R: Read>(&mut self, reader: &mut R) -> core2::io::Result<usize> {
        const READ_BATCH_SIZE: usize = 128;
        let mut total_bytes_read = 0;
        let mut buf = [0; READ_BATCH_SIZE];

        let mut handler = TerminalActionHandler { 
            scrollback_buffer: &mut self.scrollback_buffer,
            scrollback_cursor: &mut self.scrollback_cursor,
            scroll_position: &mut self.scroll_position,
            screen_cursor: &mut self.screen_cursor,
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
    pub fn flush(&mut self) -> core2::io::Result<usize> {
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
}

/// A struct that implements handlers for all terminal emulator actions, 
/// e.g., printing input characters, scrolling, moving the cursor, etc. 
struct TerminalActionHandler<'term, Backend: TerminalBackend> {
    // Note: we capture a mutable reference to each relevant field from the [`TextTerminal`] struct
    //       in order to work around Rust's inability to split multiple mutable borrows.
    scrollback_buffer: &'term mut ScrollbackBuffer,
    scrollback_cursor: &'term mut ScrollbackBufferPoint,
    scroll_position: &'term mut ScrollPosition,
    screen_cursor: &'term mut ScreenCursor,
    backend: &'term mut Backend,
    tab_width: &'term mut u16,
    mode: &'term mut TerminalMode,
}

impl<'term, Backend: TerminalBackend> Perform for TerminalActionHandler<'term, Backend> {

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
        let display_action = match self.mode.insert { 
            InsertMode::Insert => {
                self.scrollback_cursor.unit_idx = dest_line.insert_unit(
                    orig_scrollback_pos.unit_idx,
                    c,
                    style,
                    tab_width,
                );
                DisplayAction::Insert {
                    scrollback_start: orig_scrollback_pos,
                    scrollback_end:   *self.scrollback_cursor,
                    screen_start:     self.screen_cursor.position,
                }
            }
            InsertMode::Overwrite => {
                let (new_unit_index, width_diff) = dest_line.replace_unit(
                    orig_scrollback_pos.unit_idx,
                    c,
                    style,
                    tab_width,
                );
                self.scrollback_cursor.unit_idx = new_unit_index;
                DisplayAction::Overwrite {
                    scrollback_start: orig_scrollback_pos,
                    scrollback_end:   *self.scrollback_cursor,
                    screen_start:     self.screen_cursor.position,
                    width_difference: width_diff,
                }
            }
        };

        // Now that we've handled modifying the scrollback buffer, we can move refresh the display.
        let _orig_screen_cursor = self.screen_cursor.position;
        let new_screen_cursor = self.backend.display(display_action, &self.scrollback_buffer, None).unwrap();
        self.screen_cursor.position = new_screen_cursor;

        debug!("print({:?}): moved cursors from:\n\t {:?} -> {:?} \n\t {:?} -> {:?}",
            c, orig_scrollback_pos, self.scrollback_cursor, _orig_screen_cursor, self.screen_cursor.position
        );

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
                debug!("After line_feed(): {:?}, {:?}", self.scrollback_cursor, self.screen_cursor.position);
                if self.mode.lf_sends_cr == LineFeedSendsCarriageReturn::Yes {
                    self.carriage_return();
                }
            }
            AsciiControlCodes::Tab => self.print('\t'),
            AsciiControlCodes::Backspace => {
                // The backspace action simply moves the cursor back by one unit, 
                // without modifying the content or wrapping to the previous line.
                if self.screen_cursor.position.column != Column(0) {
                    self.move_left(1, Wrap::No);
                }
            }
            AsciiControlCodes::BackwardsDelete => {
                // Move to the previous "whole" unit (skipping over continuance units) in the scrollback buffer and delete it.
                let wrap = Wrap::Yes;
                let (new_scrollback_position, new_screen_position, scroll_action) = decrement_both_cursors(
                    *self.scrollback_cursor,
                    self.screen_cursor.position,
                    1,
                    &self.scrollback_buffer,
                    screen_size,
                    wrap,
                );
                *self.scrollback_cursor = new_scrollback_position;
                // Note that we don't need to separately move the screen cursor here,
                // as the below call to `display()` will automatically move it for us.

                // TODO: handle scroll_action here

                // After we've moved back one unit, delete that unit and then issue a corresponding display action.
                let num_units_removed = self.scrollback_buffer[new_scrollback_position.line_idx].delete_unit(new_scrollback_position.unit_idx);
                let display_action = DisplayAction::Delete {
                    screen_start: new_screen_position,
                    scrollback_start: new_scrollback_position,
                    num_units: num_units_removed,
                };
                let screen_cursor_after_display = self.backend.display(display_action, &self.scrollback_buffer, None).unwrap();
                debug!("After BackwardsDelete, screen cursor moved from {:?} -> {:?}", self.screen_cursor.position, screen_cursor_after_display);
                self.screen_cursor.position = screen_cursor_after_display;
                // warn!("Scrollback Buffer: {:?}", self.scrollback_buffer);
                assert_eq!(screen_cursor_after_display, new_screen_position);
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

    fn csi_dispatch(&mut self, params: &vte::Params, _intermediates: &[u8], _ignore: bool, action: char) {
        debug!("[CSI_DISPATCH]: parameters: {:?}\n\t intermediates: {:X?}\n\t ignore?: {}, action: {:?}",
            params, _intermediates, _ignore, action,
        );
        let screen_size = self.backend.screen_size();

        let mut params_iter = params.into_iter();
        let first_param = params_iter.next();
        let first_param_value = match first_param.into_iter().flatten().copied().next() {
            Some(x) if x != 0 => x,
            other => 1, // the default parameter is `1` (for when the parameter is absent or 0).
        };

        const FORWARD_DELETE_PARAM: u16 = 3;
        const HOME_KEY_PARAM: u16 = 1;
        const END_KEY_PARAM: u16 = 4;

        match (action, first_param_value) {
            ('~', FORWARD_DELETE_PARAM) => {
                // Forward delete (the "Delete" key) was pressed, so delete the current unit from the scrollback buffer.
                let wrap = Wrap::Yes;
                let scrollback_cursor = *self.scrollback_cursor;
                let num_units_removed = self.scrollback_buffer[scrollback_cursor.line_idx].delete_unit(scrollback_cursor.unit_idx);
                let display_action = DisplayAction::Delete {
                    screen_start: self.screen_cursor.position,
                    num_units: num_units_removed,
                    scrollback_start: scrollback_cursor,
                };
                let _screen_cursor_after_display = self.backend.display(display_action, &self.scrollback_buffer, None).unwrap();
                assert_eq!(self.screen_cursor.position, _screen_cursor_after_display);
            }
            ('~', HOME_KEY_PARAM) => {
                // Home key was pressed, move to the beginning of the current row.
                self.move_left(screen_size.num_columns.0 as usize, Wrap::No);
            }
            ('~', END_KEY_PARAM) => {
                // End key was pressed, move to the end (after the last unit) of the current row.
                self.move_right(screen_size.num_columns.0 as usize, Wrap::No);
            }
            ('A', num_rows) => {
                // Down arrow was pressed, move the cursor up.
                self.move_up(Row(1));
            }
            ('B', num_rows) => {
                // Down arrow was pressed, move the cursor down.
                self.move_down(Row(1));
            }
            ('C', num_units) => {
                // Right arrow was pressed, move the cursor right.
                self.move_right(num_units as usize, Wrap::Yes);
            }
            ('D', num_units) => {
                // Left arrow was pressed, move the cursor left.
                self.move_left(num_units as usize, Wrap::Yes);
            }

            (_action, _first_param) => debug!("[CSI_DISPATCH] unhandled action: {}, first param: {}", _action, _first_param),
        }

    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {
        debug!("[ESC_DISPATCH]: intermediates: {:X?}\n\t ignore?: {}, byte: {:#X}",
            _intermediates, _ignore, _byte,
        );
    }
}

impl<'term, Backend: TerminalBackend> TerminalActionHandler<'term, Backend> {
    /// Moves the screen cursor down one row.
    ///
    /// If in Insert mode, a new `Line` will be inserted into the scrollback buffer,
    /// with the remainder of the `Line` being reflowed onto the next new `Line`.
    /// The column will be set back to `Column(0)`.
    ///
    /// If in Overwrite mode, only the cursor's row position will be moved;
    /// no line will be inserted and the column position will not change.
    ///
    /// In either Insert or Overwrite mode, a new `Line` will be added if the cursor
    /// is already at the last displayable line of the scrollback buffer.
    ///
    /// As expected, this also adjusts the scrollback cursor position to the `Unit`
    /// in the scrollback buffer at the corresponding new position of the screen cursor.
    fn line_feed(&mut self) {
        let screen_size = self.backend.screen_size();
        let screen_width = screen_size.num_columns.0 as usize;
        let original_screen_cursor = self.screen_cursor.position;

        debug!("line_feed(): current position: {:?} {:?}", self.scrollback_cursor, self.screen_cursor.position);

        if self.mode.insert == InsertMode::Overwrite {
            // Adjust the scrollback cursor position to the unit displayed one row beneath it
            self.move_down(Row(1));
            debug!("line_feed(): after move_down() position: {:?} {:?}", self.scrollback_cursor, self.screen_cursor.position);

            if let Some(line) = self.scrollback_buffer.get(self.scrollback_cursor.line_idx.0) {
                // We're within the bounds of an existing Line, so there's nothing else to do.
                self.screen_cursor.underneath = line.get(self.scrollback_cursor.unit_idx.0)
                    .map(|unit| unit.clone())
                    .unwrap_or_else(|| Unit {
                        style: line.last().map(|last_unit| last_unit.style).unwrap_or_default(),
                        ..Default::default()
                    });
            } else {
                // There weren't enough existing Lines, so we need to insert them.
                let num_empty_lines_to_insert = self.scrollback_cursor.line_idx.0 - self.scrollback_buffer.len() + 1;
                debug!("line_feed(): inserting {} empty lines", num_empty_lines_to_insert);
                for _i in 0..num_empty_lines_to_insert {
                    self.scrollback_buffer.push(Line::new());
                }
            }
        }
        else {
            // If in Insert mode, we need to split the line at the current unit,
            // insert a new line, and move the remainder of that Line's content into the new line.
            let curr_line = &mut self.scrollback_buffer[self.scrollback_cursor.line_idx];
            self.scrollback_cursor.unit_idx = curr_line.previous_non_continuance_unit(self.scrollback_cursor.unit_idx);
            let new_line_units = curr_line.units.split_off(self.scrollback_cursor.unit_idx.0);
            let new_line = Line { units: new_line_units };

            // Insert the split-off new line into the scrollback buffer
            let next_line_idx = self.scrollback_cursor.line_idx + LineIndex(1);
            *self.scrollback_cursor = ScrollbackBufferPoint {
                line_idx: next_line_idx,
                unit_idx: UnitIndex(0), // could also keep the unit index as is, and insert padding into `new_line_units`
            };
            self.scrollback_buffer.insert(next_line_idx.0, new_line);

            // Actually move the screen cursor down to the next row.
            // The screen cursor's column has already been adjusted above.
            self.screen_cursor.position.column.0 = 0;
            self.screen_cursor.position.row.0 += 1;
            let scroll_action = if self.screen_cursor.position.row > screen_size.last_row() {
                let scroll_down = ScrollAction::Down(self.screen_cursor.position.row.0 as usize - screen_size.last_row().0 as usize);
                self.screen_cursor.position.row = screen_size.last_row();
                scroll_down
            } else {
                ScrollAction::None
            };
            // TODO: handle scroll action
            self.screen_cursor.position = self.backend.move_cursor_to(self.screen_cursor.position);
        }
    }

    /// Moves the screen cursor back to the beginning of the current row
    /// and adjusts the scrollback cursor position to point to that corresponding Unit.
    ///
    /// Note that a carriage return alone does not move the screen cursor down to the next row,
    /// only a line feed (new line) can do that.
    fn carriage_return(&mut self) {
        if self.screen_cursor.position.column == Column(0) { return; }

        let unit_idx = self.scrollback_cursor.unit_idx;
        let screen_width = self.backend.screen_size().num_columns.0 as usize;
        let index_of_previous_wrap = util::round_down(unit_idx.0, screen_width); 
        
        debug!("carriage_return: setting scrollback buffer at {:?} from {:?} to {:?}",
            self.scrollback_cursor.line_idx, self.scrollback_cursor.unit_idx, index_of_previous_wrap
        );
        self.scrollback_cursor.unit_idx = UnitIndex(index_of_previous_wrap);
        // self.screen_cursor.underneath = self.scrollback_buffer[*self.scrollback_cursor].clone();

        // Move the screen cursor to the beginning of the current row.
        self.screen_cursor.position.column = Column(0);
        self.screen_cursor.position = self.backend.move_cursor_to(self.screen_cursor.position);
    }


    /// Moves the screen cursor up by the given number of rows
    /// and sets the scrollback buffer position to the corresponding line and unit index.
    ///
    /// This is a free-floating move operation that **does not** align the 
    /// screen cursor to existing units in the scrollback buffer.
    /// That must be done separately if desired. 
    fn move_up(&mut self, num_rows: Row) {
        let screen_size = self.backend.screen_size();
        let orig_screen_position = self.screen_cursor.position;
        let orig_scrollback_position = *self.scrollback_cursor;
        if orig_screen_position.row == Row(0) { return; }

        // First, adjust the screen cursor up by `num_rows`.
        let num_rows = num_rows.0 as usize;
        let orig_row = orig_screen_position.row.0 as usize;
        let (new_screen_row, scroll_action) = if num_rows > orig_row {
            (Row(0), ScrollAction::Up(num_rows - orig_row))
        } else {
            (Row((orig_row - num_rows) as u16), ScrollAction::None)
        };

        self.screen_cursor.position.row = new_screen_row;
        trace!("move_up({:?}): orig_row: {:?}, new_screen_row: {:?}", num_rows, orig_row, new_screen_row);
        self.screen_cursor.position = self.backend.move_cursor_to(self.screen_cursor.position);

        // TODO: handle scroll action

        let new_scrollback_position = self.screen_cursor.position.to_scrollback_point(
            // TODO: after `to_scrollback_point()` supports backwards navigation from a point below the current screen point,
            //       then we can use the original positions. But since it doesn't support that, we must use the current scroll position.
            //
            // (orig_scrollback_position, orig_screen_position),
            (self.scroll_position.start_point(), ScreenPoint::default()),
            &self.scrollback_buffer,
            screen_size
        );
        *self.scrollback_cursor = new_scrollback_position;
    }

    /// Moves the screen cursor down by the given number of rows
    /// and sets the scrollback buffer position to the corresponding line and unit index.
    ///
    /// This is a free-floating move operation that **does not** align the 
    /// screen cursor to existing units in the scrollback buffer.
    /// That must be done separately if desired. 
    fn move_down(&mut self, num_rows: Row) {
        let screen_size = self.backend.screen_size();
        let orig_screen_position = self.screen_cursor.position;
        let orig_scrollback_position = *self.scrollback_cursor;

        // First, adjust the screen cursor down by `num_rows`.
        let orig_row = orig_screen_position.row.0 as usize;
        let last_row = screen_size.last_row().0 as usize;
        let target_row = orig_row + num_rows.0 as usize;
        let (new_screen_row, scroll_action) = if target_row > last_row {
            (screen_size.last_row(), ScrollAction::Down(target_row - last_row))
        } else {
            (Row(target_row as u16), ScrollAction::None)
        };

        self.screen_cursor.position.row = new_screen_row;
        trace!("move_down({:?}): orig_row: {:?} target_row: {:?}, new_screen_row: {:?}", num_rows, orig_row, target_row, new_screen_row);
        self.screen_cursor.position = self.backend.move_cursor_to(self.screen_cursor.position);

        // TODO: handle scroll action

        let new_scrollback_position = self.screen_cursor.position.to_scrollback_point(
            (orig_scrollback_position, orig_screen_position),
            &self.scrollback_buffer,
            screen_size
        );
        *self.scrollback_cursor = new_scrollback_position;
    }

    fn move_left(&mut self, num_units: usize, wrap: Wrap) {
        let (new_scrollback_position, new_screen_position, scroll_action) = decrement_both_cursors(
            *self.scrollback_cursor,
            self.screen_cursor.position,
            num_units,
            &self.scrollback_buffer,
            self.backend.screen_size(),
            wrap,
        );

        debug!("move_left({}, Wrap::{:?}): moving from\n\t {:?} -> {:?}\n\t {:?} -> {:?} \n\t scroll action: {:?}",
            num_units, wrap, self.scrollback_cursor, new_scrollback_position, self.screen_cursor.position, new_screen_position, scroll_action
        );

        *self.scrollback_cursor = new_scrollback_position;
        self.screen_cursor.position = self.backend.move_cursor_to(new_screen_position);

        // TODO: handle or return scroll_action here
    }

    fn move_right(&mut self, num_units: usize, wrap: Wrap) {
        let (new_scrollback_position, new_screen_position, scroll_action) = increment_both_cursors(
            *self.scrollback_cursor,
            self.screen_cursor.position,
            num_units,
            &self.scrollback_buffer,
            self.backend.screen_size(),
            wrap,
        );

        debug!("move_right({}, Wrap::{:?}): moving from\n\t {:?} -> {:?}\n\t {:?} -> {:?} \n\t scroll action: {:?}",
            num_units, wrap, self.scrollback_cursor, new_scrollback_position, self.screen_cursor.position, new_screen_position, scroll_action
        );

        *self.scrollback_cursor = new_scrollback_position;
        self.screen_cursor.position = self.backend.move_cursor_to(new_screen_position);

        // TODO: handle or return scroll_action here
    }


    #[inline]
    fn get_unit_underneath_cursor(&mut self) {

    }

}


/// Decrements the scrollback cursor by the given number of units
/// and moves the screen cursor correspondingly (in conjunction) with the scrollback cursor.
///
/// Returns the new positions of the scrollback cursor and the screen cursor,
/// as well as a `ScrollAction` describing what, if any, scrolling action needs to occur
/// based on the cursor movement.
///
/// After moving the scrollback cursor by `num_units`, its position will be auto-snapped
/// to the closest previous non-continuance unit to ensure it is validly aligned.
/// 
fn decrement_both_cursors(
    scrollback_position: ScrollbackBufferPoint,
    screen_position: ScreenPoint,
    mut num_units: usize,
    scrollback_buffer: &ScrollbackBuffer,
    screen_size: ScreenSize,
    wrap: Wrap,
) -> (ScrollbackBufferPoint, ScreenPoint, ScrollAction) {
    let screen_width = screen_size.num_columns.0 as usize;

    if wrap == Wrap::No {
        // Don't decrement past the beginning of a screen row. 
        let new_scrollback_position = max(
            util::round_down(scrollback_position.unit_idx.0, screen_width),
            scrollback_position.unit_idx.0.saturating_sub(num_units)
        );
        let new_scrollback_unit_idx = scrollback_buffer[scrollback_position.line_idx]
            .previous_non_continuance_unit(UnitIndex(new_scrollback_position));
        let units_moved = scrollback_position.unit_idx - new_scrollback_unit_idx;
        let new_screen_column = screen_position.column.0.saturating_sub(units_moved.0 as u16);
        return (
            ScrollbackBufferPoint { line_idx: scrollback_position.line_idx, unit_idx: new_scrollback_unit_idx },
            ScreenPoint { row: screen_position.row, column: Column(new_screen_column) },
            ScrollAction::None
        );
    }

    // Here, we handle the more complex case of cursor movement that may wrap backwards to a previous row/line.
    let mut scrollback_position = scrollback_position;
    let mut screen_rows_moved_up = 0;
    let mut screen_column = screen_position.column;

    while num_units > 0 {
        // Move backwards in this Line by up to `num_units`, not exceeding the bounds of this Line's units vector.  
        let original_unit_idx = scrollback_position.unit_idx.0;
        let new_unit_idx = original_unit_idx.saturating_sub(num_units);
        scrollback_position.unit_idx.0 = new_unit_idx;

        num_units = num_units.saturating_sub(original_unit_idx); // we've "handled" `original_unit_idx` units worth of movement

        // Calculate how many screen rows we just moved up when moving backwards in this Line's unit index.
        let old_row = original_unit_idx / screen_width;
        let new_row = new_unit_idx / screen_width;
        screen_rows_moved_up += old_row - new_row;
        screen_column = Column((scrollback_position.unit_idx.0 % screen_width) as u16);

        // If we still have more units to move backwards by, 
        // then we need to wrap back to the previous Line in the scrollback buffer (if there is one).
        if num_units > 0 {
            if scrollback_position.line_idx > LineIndex(0) {
                // wrap backwards to the end of the previous line
                scrollback_position.line_idx -= LineIndex(1);
                scrollback_position.unit_idx = scrollback_buffer[scrollback_position.line_idx].last_unit();
                num_units = num_units.saturating_sub(1);

                // Calculate the corresponding screen position
                screen_column = Column((scrollback_position.unit_idx.0 % screen_width) as u16);
                screen_rows_moved_up += 1;
            } else {
                // we're at the first line, so there's nowhere to wrap backwards to.
                break;
            }
        }
    }

    // Adjust the cursor to the closest previous non-continuance unit boundary.
    let original_unit_idx = scrollback_position.unit_idx;
    let new_unit_idx = scrollback_buffer[scrollback_position.line_idx]
        .previous_non_continuance_unit(scrollback_position.unit_idx);

    if new_unit_idx != original_unit_idx {
        scrollback_position.unit_idx = new_unit_idx;

        // Calculate how many screen rows we just moved up when moving to the previous non-continuance unit.
        let old_row = original_unit_idx.0 / screen_width;
        let new_row = new_unit_idx.0 / screen_width;
        screen_rows_moved_up += old_row - new_row;
        screen_column = Column((new_unit_idx.0 % screen_width) as u16);
    }

    // Finally, use `screen_rows_moved_up` to calculate the new screen cursor position
    // and whether a scroll action is necessary.
    let orig_row = screen_position.row;
    let (new_screen_row, scroll_action) = if screen_rows_moved_up > orig_row.0 as usize {
        (Row(0), ScrollAction::Up(screen_rows_moved_up - orig_row.0 as usize))
    } else {
        (Row(orig_row.0 - screen_rows_moved_up as u16), ScrollAction::None)
    };
    
    (
        scrollback_position,
        ScreenPoint {
            row:    new_screen_row,
            column: screen_column,
        },
        scroll_action,
    )
}



/// Increments the scrollback cursor by the given number of units
/// and moves the screen cursor correspondingly (in conjunction) with the scrollback cursor.
///
/// Returns the new positions of the scrollback cursor and the screen cursor,
/// as well as a `ScrollAction` describing what, if any, scrolling action needs to occur
/// based on the cursor movement.
///
/// After moving the scrollback cursor by `num_units`, its position will be auto-snapped
/// to the next closest non-continuance unit to ensure it is validly aligned.
/// 
fn increment_both_cursors(
    scrollback_position: ScrollbackBufferPoint,
    screen_position: ScreenPoint,
    mut num_units: usize,
    scrollback_buffer: &ScrollbackBuffer,
    screen_size: ScreenSize,
    wrap: Wrap,
) -> (ScrollbackBufferPoint, ScreenPoint, ScrollAction) {
    let screen_width = screen_size.num_columns.0 as usize;

    if wrap == Wrap::No {
        let line = &scrollback_buffer[scrollback_position.line_idx];
        // Don't increment past the last screen column or past the end of this line.
        let new_scrollback_unit_idx = min(min(
            util::round_down(scrollback_position.unit_idx.0, screen_width) + screen_width - 1,
            line.len()), // not `len() - 1` because we want to move the cursor to right after the last unit
            scrollback_position.unit_idx.0.saturating_add(num_units),
        );
        let new_scrollback_unit_idx = line.next_non_continuance_unit(UnitIndex(new_scrollback_unit_idx));
        let units_moved = new_scrollback_unit_idx - scrollback_position.unit_idx;
        let new_screen_column = screen_position.column.0.saturating_add(units_moved.0 as u16);
        return (
            ScrollbackBufferPoint { line_idx: scrollback_position.line_idx, unit_idx: new_scrollback_unit_idx },
            ScreenPoint { row: screen_position.row, column: Column(new_screen_column) },
            ScrollAction::None
        );
    }

    // Here, we handle the more complex case of cursor movement that may wrap forwards to a future row/line.
    let mut scrollback_position = scrollback_position;
    let mut screen_rows_moved_down = 0;
    let mut screen_column = screen_position.column;

    while num_units > 0 {
        // Move forwards in this Line by up to `num_units`, not exceeding the bounds of this Line's units vector.  
        let original_unit_idx = scrollback_position.unit_idx.0;
        let new_unit_idx = original_unit_idx.saturating_add(num_units);
        scrollback_position.unit_idx = min(UnitIndex(new_unit_idx), scrollback_buffer[scrollback_position.line_idx].last_unit());

        num_units = num_units.saturating_sub(scrollback_position.unit_idx.0 - original_unit_idx); // we've "handled" this many units worth of movement

        // Calculate how many screen rows we just moved down when moving forwards through this Line's units.
        let old_row = original_unit_idx / screen_width;
        let new_row = new_unit_idx / screen_width;
        screen_rows_moved_down += new_row - old_row;
        screen_column = Column((scrollback_position.unit_idx.0 % screen_width) as u16);

        // If we still have more units to move forwards by, 
        // then we need to wrap forwards to the next Line in the scrollback buffer (if there is one).
        if num_units > 0 {
            if scrollback_position.line_idx >= scrollback_buffer.last_line() {
                // we're at the last line, so there's nowhere to wrap forwards to.
                break;
            } else {
                // wrap forwards to the beginning of the next line
                scrollback_position.line_idx += LineIndex(1);
                scrollback_position.unit_idx = UnitIndex(0);
                num_units = num_units.saturating_sub(1);

                // Calculate the corresponding screen position
                screen_column = Column(0);
                screen_rows_moved_down += 1;
            }
        }
    }

    // Adjust the cursor to the next closest non-continuance unit boundary.
    let original_unit_idx = scrollback_position.unit_idx;
    let new_unit_idx = scrollback_buffer[scrollback_position.line_idx]
        .next_non_continuance_unit(scrollback_position.unit_idx);

    if new_unit_idx != original_unit_idx {
        scrollback_position.unit_idx = new_unit_idx;

        // Calculate how many screen rows we just moved down when moving to the next non-continuance unit.
        let old_row = original_unit_idx.0 / screen_width;
        let new_row = new_unit_idx.0 / screen_width;
        screen_rows_moved_down += new_row - old_row;
        screen_column = Column((new_unit_idx.0 % screen_width) as u16);
    }

    // Finally, use `screen_rows_moved_down` to calculate the new screen cursor position
    // and whether a scroll action is necessary.
    let orig_row = screen_position.row.0 as usize;
    let target_row = orig_row + screen_rows_moved_down;
    let last_row = screen_size.last_row().0 as usize;
    let (new_screen_row, scroll_action) = if target_row > last_row {
        (screen_size.last_row(), ScrollAction::Down(target_row - last_row))
    } else {
        (Row(target_row as u16), ScrollAction::None)
    };
    
    (
        scrollback_position,
        ScreenPoint {
            row:    new_screen_row,
            column: screen_column,
        },
        scroll_action,
    )
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
/// # 1-to-1 Relationship between Units and Columns
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
/// # What Units are Not
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
impl ScreenSize {
    /// Returns the index of the final `Row`, which is `num_rows - 1`.
    #[inline(always)]
    pub fn last_row(&self) -> Row {
        self.num_rows - Row(1)
    }

    /// Returns the index of the final `Column`, which is `num_columns - 1`.
    #[inline(always)]
    pub fn last_column(&self) -> Column {
        self.num_columns - Column(1)
    }
}

/// A 2D position value that represents a point on the screen,
/// in which `(0, 0)` represents the top-left corner.
/// Thus, a valid `ScreenPoint` must fit be the bounds of 
/// the current [`ScreenSize`].
#[derive(Copy, Clone, Default, PartialEq, Eq)]
#[derive(Add, AddAssign, Sub, SubAssign)]
pub struct ScreenPoint {
    column: Column,
    row: Row,
} 
impl Ord for ScreenPoint {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.row == other.row {
            self.column.cmp(&other.column)
        } else {
            self.row.cmp(&other.row)
        }
    }
}
impl PartialOrd for ScreenPoint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl fmt::Debug for ScreenPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({:?}, {:?})", self.column, self.row)
    }
}
impl ScreenPoint {
    /// Returns the point in the scrollback buffer that this `ScreenPoint` points to
    /// based on the given known origin point.
    ///
    /// The `origin_point` is the `ScrollbackBufferPoint` that is currently displayed at `ScreenPoint(0,0)`,
    /// i.s., the coordinate of the `Unit` in the scrollback buffer that is at the upper-left corner of the screen.
    /// Typically, this can be obtained using the current screen `ScrollPosition`,
    /// see [`ScrollPosition::start_point()`].
    ///
    /// A `Unit` may or may not exist at the returned `ScrollbackBufferPoint`.
    ///
    /// If the scrollback buffer does not have sufficient lines (rows) or units (columns),
    /// this returns a `ScrollbackBufferPoint` based on the assumption that 
    /// sufficient empty lines (that occupy one row each) and empty units (that occupy one column each) 
    /// would be inserted into the scrollback buffer.
    fn to_scrollback_point(
        &self,
        known_prior_point: (ScrollbackBufferPoint, ScreenPoint), 
        scrollback_buffer: &ScrollbackBuffer,
        screen_size: ScreenSize,
    ) -> ScrollbackBufferPoint {
        let screen_width = screen_size.num_columns.0 as usize;
        let target_row = self.row.0 as usize;
        let target_column = self.column.0 as usize;

        assert!(known_prior_point.1 <= *self); // TODO: support calculations if `self` is before the known screen point

        let mut row = known_prior_point.1.row.0 as usize;
        let ScrollbackBufferPoint { mut line_idx, mut unit_idx } = known_prior_point.0;
        
        // Iterate over the lines in the scrollback buffer starting at the current position
        // to determine how many displayed rows on screen each line takes up.
        while let Some(line) = scrollback_buffer.get(line_idx.0) {
            let start_row = unit_idx.0 / screen_width;
            let last_unit = line.last_unit().0;
            let end_row = last_unit / screen_width;
            let additional_rows = end_row.saturating_sub(start_row);
            row += additional_rows;

            trace!("to_scrollback_point(): {:?}: start_row: {:?}, last_unit: {:?}, end_row: {:?}, additional_rows: {:?}, row: {:?}, target_row: {:?}", 
                line_idx, start_row, last_unit, end_row, additional_rows, row, target_row
            );

            if row >= target_row {
                let row_overshoot = row - target_row;
                trace!("to_scrollback_point(): row_overshoot: {:?}", row_overshoot);
                unit_idx = UnitIndex(last_unit.saturating_sub(row_overshoot * screen_width));
                break;
            }

            // This `line` didn't cover enough displayed rows, so we move to the next one and keep going.
            row += 1;
            line_idx += LineIndex(1);
            unit_idx = UnitIndex(0);
        }

        trace!("to_scrollback_point(): after iterating, row: {:?}, target_row: {:?}, line_idx: {:?}", row, target_row, line_idx);

        if row < target_row {
            // The scrollback buffer didn't have enough lines.
            // Currently, `line_idx` is right after the last line.
            // We calculate the target line index as: `line_idx - 1 + (target_row - row)`.
            let target_line = line_idx.0.saturating_add(target_row).saturating_sub(row).saturating_sub(1);
            trace!("to_scrollback_point(): target_line: {:?}", target_line);
            ScrollbackBufferPoint {
                line_idx: LineIndex(target_line),
                unit_idx: UnitIndex(target_column),
            }
        } else {
            // The scrollback buffer had enough lines.
            ScrollbackBufferPoint {
                line_idx,
                unit_idx: UnitIndex(util::round_down(unit_idx.0, screen_width) + target_column),
            }
        }
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
#[derive(Copy, Clone, Default, PartialEq, Eq)]
#[derive(Add, AddAssign, Sub, SubAssign)]
pub struct ScrollbackBufferPoint {
    unit_idx: UnitIndex,
    line_idx: LineIndex,
}
impl Ord for ScrollbackBufferPoint {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.line_idx == other.line_idx {
            self.unit_idx.cmp(&other.unit_idx)
        } else {
            self.line_idx.cmp(&other.line_idx)
        }
    }
}
impl PartialOrd for ScrollbackBufferPoint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
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
struct ScreenCursor {
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



/// Advances the cursor's screen coordinate forward by the given number of columns,
/// ignoring the contents of the scrollback buffer.
///
/// Returns a tuple of:
/// 1. the new position of the screen cursor,
/// 2. a `ScrollAction` describing what kind of scrolling action needs to be taken
///    to handle this screen cursor movement.
fn increment_screen_cursor(
    mut cursor_position: ScreenPoint,
    num_columns: usize,
    screen_size: ScreenSize,
    wrap: Wrap,
) -> (ScreenPoint, ScrollAction) {
    if wrap == Wrap::No {
        cursor_position.column.0 = cursor_position.column.0.saturating_add(num_columns as u16);
        return (cursor_position, ScrollAction::None);
    }

    let screen_width = screen_size.num_columns.0 as usize;
    let last_row = screen_size.last_row().0 as usize;

    let end_column = cursor_position.column.0 as usize + num_columns;
    let rows_added = end_column / screen_width;
    let new_column = Column((end_column % screen_width) as u16);

    let current_row = cursor_position.row.0 as usize;
    let new_row = cursor_position.row.0 as usize + rows_added;
    if new_row > last_row {
        let diff = new_row - last_row;
        (ScreenPoint { column: new_column, row: Row(last_row as u16) }, ScrollAction::Down(diff))
    } else {
        (ScreenPoint { column: new_column, row: Row(new_row as u16) }, ScrollAction::None)
    }
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
    wrap: Wrap,
) -> (ScreenPoint, ScrollAction) {
    if wrap == Wrap::No {
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


/// Returns the position of the given `scrollback_cursor` moved backward by `num_units` units.
///
/// After the basic movement and line wrapping calculations, this function automatically aligns
/// the returned scrollback cursor position to the closest previous non-continuance `Unit`.
///
/// This is a pure calculation that does not modify any cursor positions.
fn decrement_scrollback_cursor(
    mut scrollback_cursor: ScrollbackBufferPoint,
    scrollback_buffer: &ScrollbackBuffer,
    mut num_units: usize,
    screen_width: Column,
    wrap_lines: Wrap,
) -> ScrollbackBufferPoint {

    if wrap_lines == Wrap::No {
        // Don't decrement past the beginning of a screen row. 
        scrollback_cursor.unit_idx.0 = max(
            util::round_down(scrollback_cursor.unit_idx.0, screen_width.0 as usize),
            scrollback_cursor.unit_idx.0.saturating_sub(num_units)
        );
    } else {
        while num_units > 0 {
            let unit_idx = scrollback_cursor.unit_idx;
            scrollback_cursor.unit_idx.0 = unit_idx.0.saturating_sub(num_units);

            if num_units > unit_idx.0 {
                // Wrap backwards to the previous line
                if scrollback_cursor.line_idx > LineIndex(0) {
                    // wrap backwards to the end of the previous line
                    scrollback_cursor.line_idx -= LineIndex(1);
                    scrollback_cursor.unit_idx = scrollback_buffer[scrollback_cursor.line_idx].last_unit();
                } else {
                    // we're at the first line, so there's nowhere to wrap backwards to.
                    break;
                }
            } else {
                // Not enough remaining units of movement to require backwards line wrapping, so we're done.
                break;
            }

            // we've handled "unit_idx" units worth of backwards movement.
            num_units = num_units.saturating_sub(unit_idx.0);
        }
    }

    scrollback_cursor.unit_idx = scrollback_buffer[scrollback_cursor.line_idx]
        .previous_non_continuance_unit(scrollback_cursor.unit_idx);

    scrollback_cursor
}


/// Computes the screen cursor movement required to move from the given `start` point 
/// to the given `end` point in the scrollback buffer.
///
/// Returns a tuple of `(num_columns, num_rows)` that the screen cursor should move.
fn relative_screen_movement(
    scrollback_buffer: &ScrollbackBuffer,
    start: ScrollbackBufferPoint,
    end: ScrollbackBufferPoint,
    screen_width: Column,
) -> (i32, i32) {
    let screen_width = screen_width.0 as usize;

    // TODO THIS IS WRONG, must use the scrollback buffer to check each line's length.

    let start_column = start.unit_idx.0 % screen_width;
    let start_row    = start.line_idx.0 + (start.unit_idx.0 / screen_width);
    let end_column   = end.unit_idx.0 % screen_width;
    let end_row      = end.line_idx.0 + (end.unit_idx.0 / screen_width); 

    let column_diff = end_column as isize - start_column as isize;
    let row_diff    = end_row as isize - start_row as isize;
    (column_diff as i32, row_diff as i32)
}


enum ScreenToScrollbackConversion {
    ExactMatch(ScrollbackBufferPoint),
    ClosestPrevious(ScrollbackBufferPoint, ScreenPoint),
}
impl ScreenToScrollbackConversion {
    fn to_option(self) -> Option<ScrollbackBufferPoint> {
        match self {
            Self::ExactMatch(sbp) => Some(sbp),
            _ => None,
        }
    }
}


/// Returns the point on the screen where the given point in the scrollback buffer
/// is displayed, based on the current position of the screen viewport.
///
/// Returns `None` if the given `scrollback_point` would be displayed beyond the screen bounds.
fn scrollback_point_to_screen_point(
    scroll_position: ScrollPosition,
    scrollback_point: ScrollbackBufferPoint,
    scrollback_buffer: &ScrollbackBuffer
) -> Option<ScreenPoint> {
    let start_point = scroll_position.start_point();
    
    unimplemented!()
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
    #[must_use]
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
pub struct TtyBackend<Output: core2::io::Write> {
    /// The width and height of this terminal's screen.
    screen_size: ScreenSize,

    /// The actual position of the cursor on the real terminal backend screen.
    real_screen_cursor: ScreenPoint,

    /// The output stream to which bytes are written,
    /// which will be read by a TTY.terminal emulator on the other side of the stream.
    output: Output,

    insert_mode: InsertMode,
}
impl<Output: core2::io::Write> TtyBackend<Output> {
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
    const INSERT_CHARACTER: &'static [u8] = &[
        AsciiControlCodes::Escape,
        b'[',
        b'1',
        b'@',
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
    

    /// Deletes the given number of units from the screen starting at the given screen coordinate.
    ///
    /// Returns the new position of the screen cursor, which should be equivalent to `screen_start`
    /// unless `screen_start` is beyond the bounds of the screen.
    /// 
    /// See [`DisplayAction::Delete`] for more information on how this works.
    fn delete(
        &mut self,
        screen_start: ScreenPoint,
        num_units_to_delete: usize,
        _scrollback_start: ScrollbackBufferPoint,
        _scrollback_buffer: &ScrollbackBuffer,
    ) -> ScreenPoint {
        debug!("Deleting {} units forwards at {:?}", num_units_to_delete, screen_start);
        let wrap = Wrap::Yes;
        
        // move the cursor to `screen_start`
        if self.real_screen_cursor != screen_start {
            warn!("TtyBackend::delete(): moving screen cursor from {:?} to {:?}", self.real_screen_cursor, screen_start);
            self.real_screen_cursor = self.move_cursor_to(screen_start);
        }

        for _i in 0..num_units_to_delete {
            // Forward-delete the current character unit, but do not move the actual screen cursor, 
            // because the backend terminal emulator will shift everything in the current line to the left.
            self.output.write(Self::DELETE_CHARACTER).unwrap();
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
impl<Output: core2::io::Write> TerminalBackend for TtyBackend<Output> {
    type DisplayError = core2::io::Error;

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

        let (scrollback_start, scrollback_end, screen_start, width_diff) = match display_action {
            DisplayAction::Insert { scrollback_start, scrollback_end, screen_start } => {
                (scrollback_start, scrollback_end, screen_start, None)
            }
            DisplayAction::Overwrite { scrollback_start, scrollback_end, screen_start, width_difference } => {
                (scrollback_start, scrollback_end, screen_start, Some(width_difference))
            }
            DisplayAction::Delete { screen_start, num_units, scrollback_start } => {
                return Ok(self.delete(screen_start, num_units, scrollback_start, scrollback_buffer));
            }
            _other => panic!("display(): unimplemented DisplayAction: {:?}", _other),
        };

        if self.real_screen_cursor != screen_start {
            error!("Unimplemented: need to move screen cursor from {:?} to {:?}", self.real_screen_cursor, screen_start);
            // TODO: issue a command to move the screen cursor to `screen_start`
        }

        // Handle the possible width difference that may occur in an Overwrite operation.
        match width_diff {
            Some(d) if d > 0 => {
                warn!("Untested: positive width diff of {} columns", d);
                for _i in 0..d {
                    self.output.write(Self::INSERT_CHARACTER).unwrap();
                }
            }
            Some(d) if d < 0 => {
                warn!("Untested: negative width diff of {} columns", d);
                for _i in d..0 {
                    self.output.write(Self::DELETE_CHARACTER).unwrap();
                }
            }
            _=> { } // do nothing
        }

        // Actually write out the contents from the requested lines of the scrollback buffer.
        let mut start_unit = scrollback_start.unit_idx; 
        for line_idx in scrollback_start.line_idx.0 ..= scrollback_end.line_idx.0 {
            let line_idx = LineIndex(line_idx);
            let line = &scrollback_buffer[line_idx];
            
            // Write the requested part of this line, up to the entire line.
            let end = if scrollback_end.line_idx == line_idx {
                scrollback_end.unit_idx.0
            } else {
                line.units.len()
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
                let (new_screen_cursor, scroll_action) = increment_screen_cursor(self.real_screen_cursor, unit_width as usize, self.screen_size, Wrap::Yes);
                debug!("display(): moving from {:?} -> {:?}", self.real_screen_cursor, new_screen_cursor);
                // If we wrapped to the next screen row, move the screen cursor.
                if new_screen_cursor.row != self.real_screen_cursor.row {
                    self.real_screen_cursor = self.move_cursor_to(new_screen_cursor);
                } else {
                    self.real_screen_cursor = new_screen_cursor;
                }

                // TODO: handle scroll action
            }

            // Once we finish writing out the whole line, if there is another line to be written out,
            // move to the beginning of the next row on screen.
            if line_idx < scrollback_end.line_idx {
                let (mut new_screen_cursor, scroll_action) = increment_screen_cursor(
                    self.real_screen_cursor,
                    self.screen_size.num_columns.0 as usize, // move one row down
                    self.screen_size,
                    Wrap::Yes
                );
                new_screen_cursor.column = Column(0);
                warn!("display(): at end of line {:?}, moving from {:?} -> {:?}", line_idx, self.real_screen_cursor, new_screen_cursor);
                self.real_screen_cursor = self.move_cursor_to(new_screen_cursor);

                // TODO: handle scroll action
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
            self.screen_size.last_column().0
        } else {
            new_col as u16
        };

        let new_row = self.real_screen_cursor.row.0 as i32 + num_rows;
        let row_bounded = if new_row <= 0 {
            0
        } else if new_row >= self.screen_size.num_rows.0 as i32 {
            self.screen_size.last_row().0
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
    /// Remove the given number of units from the screen starting at the given screen coordinate.
    ///
    /// After the delete operation, all other units coming after that point in the current `Line`
    /// are left-shifted by `num_units`.
    /// At that point, the `Unit`s starting at the given `scrollback_start` point in the scrollback buffer 
    /// should be displayed at the given `screen_start` coordinate on screen.
    ///
    /// For simplicity, this only supports a "forward" delete operation, in which 
    /// the screen cursor position does not change because only the units
    /// at or after that screen cursor position are removed.
    /// A "backwards" delete operation can be achieved by moving the cursor backwards by a few units
    /// and then issuing a regular forward delete operation.
    Delete {
        screen_start: ScreenPoint,
        num_units: usize,
        scrollback_start: ScrollbackBufferPoint,
    },
    /// Erases the contents displayed on the screen in the given range of on-screen coordinates,
    /// setting those units to blank space without changing their display style.
    ///
    /// The `screen_start` bound is inclusive; the `screen_end` bound is exclusive.
    Erase {
        screen_start: ScreenPoint,
        screen_end:   ScreenPoint,
    },
    /// Replace the contents displayed on the screen starting at the given on-screen coordinate
    /// with the contents of the scrollback buffer.
    ///
    /// The `scrollback_start` bound is inclusive; the `scrollback_end` bound is exclusive;
    /// the `screen_start` bound is also inclusive.
    ///
    /// The `width_difference` represents the difference in the displayable width of the new unit(s) 
    /// vs. the old unit(s) that existed in the scrollback buffer and were previously displayed at `screen_start`.
    /// This is effectively `new_unit_width - old_unit_width`.
    /// * If `0`, the units are the same width. 
    /// * If positive, the new unit is wider than the old unit.
    /// * If negative, the old unit is wider than the new unit.
    Overwrite {
        scrollback_start: ScrollbackBufferPoint,
        scrollback_end:   ScrollbackBufferPoint,
        screen_start:     ScreenPoint,
        width_difference: i32,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScrollAction {
    /// Do nothing, do not scroll the screen.
    None,
    /// Scroll the screen up by the included number of lines.
    Up(usize),
    /// Scroll the screen down by the included number of lines.
    Down(usize),
}

impl Drop for ScrollAction {
    fn drop(&mut self) {
        match self {
            Self::None => { }
            _ => warn!("{:?} was dropped without being handled!", self),
        }
    }
}


/// Whether or not to wrap cursor movement or text display
/// to the previous/next line or row.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Wrap {
    Yes,
    No
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
