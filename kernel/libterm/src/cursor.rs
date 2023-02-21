
use super::*;

/// The cursor structure used in the terminal.
/// A cursor is a special symbol shown in the text box of a terminal. It indicates the position of character where the next input would be put or the delete operation works on.
/// Terminal invokes its `display` method in a loop to let a cursor blink.
pub struct Cursor {
    /// Whether the cursor is enabled in the terminal.
    enabled: bool,
    /// The blinking frequency.
    freq: u128,
    /// The last time it blinks.
    time: TscTicks,
    /// The current blinking state show/hidden
    show: bool,
    /// The color of the cursor
    color: Color,
    /// The position of the cursor relative to the end of terminal text in number of characters.
    pub offset_from_end: usize,
    /// The underlying character at the position of the cursor.
    /// It is shown when the cursor is unseen.
    pub underlying_char: u8,
}

impl Cursor {
    /// Reset the state of the cursor as unseen
    pub fn reset(&mut self) {
        self.show = true;
        self.time = tsc_ticks();
    }

    /// Enable a cursor
    pub fn enable(&mut self) {
        self.enabled = true;
        self.reset();
    }

    /// Disable a cursor
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Let a cursor blink. It is invoked in a loop.
    pub fn blink(&mut self) -> bool {
        if self.enabled {
            let time = tsc_ticks();
            if let Some(duration) = time.sub(&self.time) {
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

    /// Whether a cursor is seen
    pub fn show(&self) -> bool {
        self.enabled && self.show
    }

    /// Display a cursor in a framebuffer
    /// # Arguments
    /// * `coordinate`: the start point of a textarea in the framebuffer.
    /// * `column`: the column of the cursor in the textarea.
    /// * `line`: the line of the cursor in the textarea.
    /// * `framebuffer`: the framebuffer to display the cursor in.
    ///
    /// Returns a bounding box which wraps the cursor.
    pub fn display(
        &mut self,
        relative_pos: RelativePos,
        column: usize,
        line: usize,
        window: &mut Window,
    ) -> Result<(), &'static str> {
        if self.blink() {
            let mut relative_pos = relative_pos;
            relative_pos.x += (column * CHARACTER_WIDTH) as u32;
            relative_pos.y += (line * CHARACTER_HEIGHT) as u32;
            if self.show() {
                let mut rect =
                    Rect::new(CHARACTER_WIDTH -1, CHARACTER_HEIGHT - 1, relative_pos.x as isize, relative_pos.y as isize);
                window.fill_rectangle(&mut rect, 0xF4F333);
            } else {
                window.print_string_line(
                    &relative_pos,
                    (self.underlying_char as char).to_string().as_str(),
                    0xFBF1C7,
                    0x3C3836,
                )?;
            }
        }
        Ok(())
    }

    /// Sets the position of the cursor relative to the end of the command
    pub fn set_offset_from_end(&mut self, offset: usize) {
        self.offset_from_end = offset;
    }

    /// Gets the position of the cursor relative to the end of the command
    pub fn offset_from_end(&self) -> usize {
        self.offset_from_end
    }

    /// Sets the character at the position of the cursor
    pub fn set_underlying_char(&mut self, c: u8) {
        self.underlying_char = c;
    }

    /// Gets the character at the position of the cursor
    pub fn underlying_char(&self) -> u8 {
        self.underlying_char
    }
}

impl Default for Cursor {
    fn default() -> Self {
        Cursor {
            enabled: true,
            freq: DEFAULT_CURSOR_FREQ,
            time: tsc_ticks(),
            show: true,
            color: FONT_FOREGROUND_COLOR,
            offset_from_end: 0,
            underlying_char: 0,
        }
    }
}
