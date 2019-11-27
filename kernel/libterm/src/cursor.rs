use super::*;

/// The generic cursor structure used in the primitive display subsystem.
/// A cursor is a special symbol shown in the text box of a terminal. It indicates the position of character where the next input would be put or the delete operation works on.
/// Terminal invokes its `display` method in a loop to let a cursor blink.
pub struct Cursor {
    /// Whether the cursor is enabled in the terminal.
    enabled: bool,
    /// The blinking frequency.
    freq: u64,
    /// The last time it blinks.
    time: TscTicks,
    /// The current blinking state show/hidden
    show: bool,
    /// The color of the cursor
    color: u32,
    /// The position of the cursor relative to the end of terminal text in units of number of characters.
    pub offset_from_end: usize,
    /// The underlying character at the position of the cursor.
    /// It is shown when the cursor is unseen.
    pub underlying_char: u8,
}

impl Cursor {
    /// Create a new cursor object which is initially enabled. The `blink_interval` is initialized as `DEFAULT_CURSOR_FREQ` however one can change this at any time. `time` is set to current time.
    pub fn new() -> Cursor {
        Cursor {
            enabled: true,
            freq: DEFAULT_CURSOR_FREQ,
            time: tsc_ticks(),
            show: true,
            color: FONT_COLOR,
            offset_from_end: 0,
            underlying_char: 0,
        }
    }

    pub fn reset(&mut self) {
        self.show = true;
        self.time = tsc_ticks();
    }

    pub fn enable(&mut self) {
        self.enabled = true;
        self.reset();
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    pub fn blink(&mut self) -> bool {
        if self.enabled {
            let time = tsc_ticks();
            if let Some(duration) = time.sub(&(self.time)) {
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

    pub fn show(&self) -> bool {
        self.enabled && self.show
    }

    pub fn display(
        &mut self,
        coordinate: Coord,
        column: usize,
        line: usize,
        framebuffer: &mut dyn FrameBuffer,
    ) -> Result<RectArea, &'static str> {
        if self.blink() {
            if self.show() {
                frame_buffer_drawer::fill_rectangle(
                    framebuffer,
                    coordinate
                        + (
                            (column * CHARACTER_WIDTH) as isize,
                            (line * CHARACTER_HEIGHT) as isize,
                        )
                        + (0, 1),
                    CHARACTER_WIDTH,
                    CHARACTER_HEIGHT - 2,
                    self.color,
                );
            } else {
                frame_buffer_printer::print_ascii_character(
                    framebuffer,
                    self.underlying_char,
                    FONT_COLOR,
                    BACKGROUND_COLOR,
                    coordinate,
                    column,
                    line,
                )
            }
        }

        let start = coordinate + (
            (column * CHARACTER_WIDTH) as isize, 
            (line * CHARACTER_HEIGHT) as isize
        );
        let update_area = RectArea {
            start: start,
            end: start + (CHARACTER_WIDTH as isize, CHARACTER_HEIGHT as isize)
        };

        Ok(update_area)
    }

    pub fn set_offset_from_end(&mut self, offset: usize) {
        self.offset_from_end = offset;
    }

    pub fn offset_from_end(&self) -> usize {
        self.offset_from_end
    }

    pub fn set_underlying_char(&mut self, c: u8) {
        self.underlying_char = c;
    }

    pub fn underlying_char(&self) -> u8 {
        self.underlying_char
    }
}
