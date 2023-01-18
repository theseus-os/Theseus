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
    pub fn display<P: Pixel>(
        &mut self,
        coordinate: Coord,
        column: usize,
        line: usize,
        framebuffer: &mut Framebuffer<P>,
    ) -> Result<Rectangle, &'static str> where Color: Into<P> {
        if self.blink() {
            if self.show() {
                framebuffer_drawer::fill_rectangle(
                    framebuffer,
                    coordinate
                        + (
                            (column * CHARACTER_WIDTH) as isize,
                            (line * CHARACTER_HEIGHT) as isize,
                        )
                        + (0, 1),
                    CHARACTER_WIDTH,
                    CHARACTER_HEIGHT - 2,
                    self.color.into(),
                );
            } else {
                framebuffer_printer::print_ascii_character(
                    framebuffer,
                    self.underlying_char,
                    FONT_FOREGROUND_COLOR.into(),
                    FONT_BACKGROUND_COLOR.into(),
                    coordinate,
                    column,
                    line,
                )
            }
        }

        let top_left = coordinate
            + (
                (column * CHARACTER_WIDTH) as isize,
                (line * CHARACTER_HEIGHT) as isize,
            );
        let bounding_box = Rectangle {
            top_left,
            bottom_right: top_left + (CHARACTER_WIDTH as isize, CHARACTER_HEIGHT as isize),
        };

        Ok(bounding_box)
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

impl Default for Cursor  {
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

