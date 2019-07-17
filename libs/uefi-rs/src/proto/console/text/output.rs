use crate::prelude::*;
use crate::proto::Protocol;
use crate::{unsafe_guid, CStr16, Char16, Completion, Result, Status};
use core::fmt;

/// Interface for text-based output devices.
///
/// It implements the fmt::Write trait, so you can use it to print text with
/// standard Rust constructs like the `write!()` and `writeln!()` macros.
#[repr(C)]
#[unsafe_guid("387477c2-69c7-11d2-8e39-00a0c969723b")]
#[derive(Protocol)]
pub struct Output<'boot> {
    reset: extern "win64" fn(this: &Output, extended: bool) -> Status,
    output_string: unsafe extern "win64" fn(this: &Output, string: *const Char16) -> Status,
    test_string: unsafe extern "win64" fn(this: &Output, string: *const Char16) -> Status,
    query_mode: extern "win64" fn(
        this: &Output,
        mode: usize,
        columns: &mut usize,
        rows: &mut usize,
    ) -> Status,
    set_mode: extern "win64" fn(this: &mut Output, mode: usize) -> Status,
    set_attribute: extern "win64" fn(this: &mut Output, attribute: usize) -> Status,
    clear_screen: extern "win64" fn(this: &mut Output) -> Status,
    set_cursor_position: extern "win64" fn(this: &mut Output, column: usize, row: usize) -> Status,
    enable_cursor: extern "win64" fn(this: &mut Output, visible: bool) -> Status,
    data: &'boot OutputData,
}

impl<'boot> Output<'boot> {
    /// Resets and clears the text output device hardware.
    pub fn reset(&mut self, extended: bool) -> Result {
        (self.reset)(self, extended).into()
    }

    /// Clears the output screen.
    ///
    /// The background is set to the current background color.
    /// The cursor is moved to (0, 0).
    pub fn clear(&mut self) -> Result {
        (self.clear_screen)(self).into()
    }

    /// Writes a string to the output device.
    pub fn output_string(&mut self, string: &CStr16) -> Result {
        unsafe { (self.output_string)(self, string.as_ptr()) }.into()
    }

    /// Checks if a string contains only supported characters.
    ///
    /// UEFI applications are encouraged to try to print a string even if it contains
    /// some unsupported characters.
    pub fn test_string(&mut self, string: &CStr16) -> Result<bool> {
        match unsafe { (self.test_string)(self, string.as_ptr()) } {
            Status::UNSUPPORTED => Ok(false.into()),
            other => other.into_with_val(|| true),
        }
    }

    /// Returns an iterator of all supported text modes.
    // TODO: Bring back impl Trait once the story around bounds improves
    pub fn modes<'out>(&'out mut self) -> OutputModeIter<'out, 'boot> {
        let max = self.data.max_mode as usize;
        OutputModeIter {
            output: self,
            current: 0,
            max,
        }
    }

    /// Returns the width (column count) and height (row count) of a text mode.
    ///
    /// Devices are required to support at least an 80x25 text mode and to
    /// assign index 0 to it. If 80x50 is supported, then it will be mode 1,
    /// otherwise querying for mode 1 will return the `Unsupported` error.
    /// Modes 2+ will describe other text modes supported by the device.
    ///
    /// If you want to iterate over all text modes supported by the device,
    /// consider using the iterator produced by `modes()` as a more ergonomic
    /// alternative to this method.
    fn query_mode(&self, index: usize) -> Result<(usize, usize)> {
        let (mut columns, mut rows) = (0, 0);
        (self.query_mode)(self, index, &mut columns, &mut rows).into_with_val(|| (columns, rows))
    }

    /// Returns the the current text mode.
    pub fn current_mode(&self) -> Result<Option<OutputMode>> {
        match self.data.mode {
            -1 => Ok(None.into()),
            n if n > 0 => {
                let index = n as usize;
                self.query_mode(index)
                    .map_inner(|dims| Some(OutputMode { index, dims }))
            }
            _ => unreachable!(),
        }
    }

    /// Sets a mode as current.
    pub fn set_mode(&mut self, mode: OutputMode) -> Result {
        (self.set_mode)(self, mode.index).into()
    }

    /// Returns whether the cursor is currently shown or not.
    pub fn cursor_visible(&self) -> bool {
        self.data.cursor_visible
    }

    /// Make the cursor visible or invisible.
    ///
    /// The output device may not support this operation, in which case an
    /// `Unsupported` error will be returned.
    pub fn enable_cursor(&mut self, visible: bool) -> Result {
        (self.enable_cursor)(self, visible).into()
    }

    /// Returns the column and row of the cursor.
    pub fn cursor_position(&self) -> (usize, usize) {
        let column = self.data.cursor_column;
        let row = self.data.cursor_row;
        (column as usize, row as usize)
    }

    /// Sets the cursor's position, relative to the top-left corner, which is (0, 0).
    ///
    /// This function will fail if the cursor's new position would exceed the screen's bounds.
    pub fn set_cursor_position(&mut self, column: usize, row: usize) -> Result {
        (self.set_cursor_position)(self, column, row).into()
    }

    /// Sets the text and background colors for the console.
    ///
    /// Note that for the foreground color you can choose any color.
    /// The background must be one of the first 8 colors.
    pub fn set_color(&mut self, foreground: Color, background: Color) -> Result {
        let fgc = foreground as usize;
        let bgc = background as usize;

        assert!(bgc < 8, "An invalid background color was requested");

        let attr = ((bgc & 0x7) << 4) | (fgc & 0xF);
        (self.set_attribute)(self, attr).into()
    }
}

impl<'boot> fmt::Write for Output<'boot> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // Allocate a small buffer on the stack.
        const BUF_SIZE: usize = 128;
        // Add 1 extra character for the null terminator.
        let mut buf = [0u16; BUF_SIZE + 1];

        let mut i = 0;

        // This closure writes the local buffer to the output and resets the buffer.
        let mut flush_buffer = |buf: &mut [u16], i: &mut usize| {
            buf[*i] = 0;
            let codes = &buf[..=*i];
            *i = 0;

            let text = CStr16::from_u16_with_nul(codes).map_err(|_| fmt::Error)?;

            self.output_string(text)
                .warning_as_error()
                .map_err(|_| fmt::Error)
        };

        // This closure converts a character to UCS-2 and adds it to the buffer,
        // flushing it as necessary.
        let mut add_char = |ch| {
            // UEFI only supports UCS-2 characters, not UTF-16,
            // so there are no multibyte characters.
            buf[i] = ch;
            i += 1;

            if i == BUF_SIZE {
                flush_buffer(&mut buf, &mut i).map_err(|_| ucs2::Error::BufferOverflow)
            } else {
                Ok(())
            }
        };

        // This one converts Rust line feeds to UEFI line feeds beforehand
        let add_ch = |ch| {
            if ch == '\n' as u16 {
                add_char('\r' as u16)?;
            }
            add_char(ch)
        };

        // Translate and write the input string, flushing the buffer when needed
        ucs2::encode_with(s, add_ch).map_err(|_| fmt::Error)?;

        // Flush the remainder of the buffer
        flush_buffer(&mut buf, &mut i)
    }
}

/// The text mode (resolution) of the output device.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct OutputMode {
    index: usize,
    dims: (usize, usize),
}

impl OutputMode {
    /// Returns the index of this mode.
    #[inline]
    pub fn index(&self) -> usize {
        self.index
    }

    /// Returns the width in columns.
    #[inline]
    pub fn columns(&self) -> usize {
        self.dims.0
    }

    /// Returns the height in rows.
    #[inline]
    pub fn rows(&self) -> usize {
        self.dims.1
    }
}

/// An iterator of the text modes (possibly) supported by a device.
pub struct OutputModeIter<'out, 'boot: 'out> {
    output: &'out mut Output<'boot>,
    current: usize,
    max: usize,
}

impl<'out, 'boot> Iterator for OutputModeIter<'out, 'boot> {
    type Item = Completion<OutputMode>;

    fn next(&mut self) -> Option<Self::Item> {
        let index = self.current;
        if index < self.max {
            self.current += 1;

            if let Ok(dims_completion) = self.output.query_mode(index) {
                Some(dims_completion.map(|dims| OutputMode { index, dims }))
            } else {
                self.next()
            }
        } else {
            None
        }
    }
}

/// Additional data of the output device.
#[derive(Debug)]
#[repr(C)]
struct OutputData {
    /// The number of modes supported by the device.
    max_mode: i32,
    /// The current output mode.
    /// Negative index -1 is used to notify that no valid mode is configured
    mode: i32,
    /// The current character output attribute.
    attribute: i32,
    /// The cursor’s column.
    cursor_column: i32,
    /// The cursor’s row.
    cursor_row: i32,
    /// Whether the cursor is currently visible or not.
    cursor_visible: bool,
}

/// Colors for the UEFI console.
///
/// All colors can be used as foreground colors.
/// The first 8 colors can also be used as background colors.
#[allow(missing_docs)]
#[derive(Debug, Copy, Clone)]
pub enum Color {
    Black = 0,
    Blue,
    Green,
    Cyan,
    Red,
    Magenta,
    Brown,
    LightGray,
    DarkGray,
    LightBlue,
    LightGreen,
    LightCyan,
    LightRed,
    LightMagenta,
    Yellow,
    White,
}
