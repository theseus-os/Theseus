use super::*;

/// A cursor is able to blink at a position in a container. In current Theseus, the container can be either a `TextDisplayable` or a `FrameBuffer`.
pub trait Cursor: Send {
    /// Reset the cursor by setting `show` to true and `time` to current time. This doesn't effect `enabled` variable. This will not effect the display unless terminal application refresh the textarea by using the status of `Cursor` object
    fn reset(&mut self);

    /// Enable a cursor and call `reset` internally to make sure the behavior is the same after enable it (same initial state and same interval to change)
    fn enable(&mut self);

    /// Disable a cursor by setting `enabled` to false
    fn disable(&mut self);

    /// Changes the blink state show/hidden of a cursor based on its frequency.
    /// It returns whether the cursor should be re-display. If the cursor is enabled, it returns whether the show/hidden state has been changed. Otherwise it returns true because the cursor is disabled and should refresh.
    fn blink(&mut self) -> bool;

    /// Checks if the cursor should be shown.
    fn show(&self) -> bool;

    /// Displays a cursor in a text block onto a frame buffer. An application calls this function in a loop to make it blinking.
    /// # Arguments
    /// * `coordinate`: the left top coordinate of the text block relative to the origin(top-left point) of the frame buffer.
    /// * `(col, line)`: the location of the cursor in the text block in units of characters.
    /// * `area`: the container to display in. It can be either a `FrameBuffer` or a `TextDisplayable`.
    fn display(
        &mut self,
        coordinate: Coord,
        col: usize,
        line: usize,
        area: CursorArea,
    ) -> Result<(), &'static str>;

    /// Set the position of the cursor. `offset` is the distance to the end of the text in units of characters.
    fn set_offset_from_end(&mut self, offset: usize);

    /// Return the offset of the cursor to the end of text in units of characters.
    fn offset_from_end(&self) -> usize;

    /// Set the underlying character at the position of the cursor when it's unseen.
    fn set_underlying_char(&mut self, c: u8);

    /// Return the underlying character at the position of the cursor when it's unseen.
    fn underlying_char(&self) -> u8;
}

/// The cursor structure used in the `window_manager_alpha` display subsystem. It is mainly a timer for cursor to blink properly, which also has multiple status recorded.
/// When `enabled` is false, it should remain the original word. When `enabled` is true and `show` is false, it should display blank character, only when `enabled` is true, and `show` is true, it should display cursor character.
pub struct CursorComponent {
    /// Terminal will set this variable to enable blink or not. When this is not enabled, function `blink` will always return `false` which means do not refresh the cursor
    enabled: bool,
    /// The time of blinking interval. Initially set to `DEFAULT_CURSOR_FREQ`, however, can be changed during run-time
    freq: u64,
    /// Record the time of last blink state change. This variable is updated when `reset` is called or `blink` is called and the time duration is larger than `DEFAULT_CURSOR_FREQ`
    time: TscTicks,
    /// If function `blink` returns true, then this variable indicates whether display the cursor or not. To fully determine whether to display the cursor, user should call `is_show` function
    show: bool,
    /// The position of the cursor relative to the end of text in units of characters.
    offset_from_end: usize,
    /// The underlying character when the cursor is unseen.
    underlying_char: u8,
}

impl CursorComponent {
    /// Creates a new cursor object which is initially enabled. The `blink_interval` is initialized as `DEFAULT_CURSOR_FREQ` however one can change this at any time. `time` is set to current time.
    pub fn new() -> CursorComponent {
        CursorComponent {
            enabled: true,
            freq: DEFAULT_CURSOR_FREQ,
            time: tsc_ticks(),
            show: true,
            offset_from_end: 0,
            underlying_char: 0,
        }
    }
}

impl Cursor for CursorComponent {
    fn reset(&mut self) {
        self.show = true;
        self.time = tsc_ticks();
    }

    fn enable(&mut self) {
        self.enabled = true;
        self.reset();
    }

    fn disable(&mut self) {
        self.enabled = false;
    }

    fn blink(&mut self) -> bool {
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

    fn show(&self) -> bool {
        self.enabled && self.show
    }

    fn display(
        &mut self,
        _coordinate: Coord,
        col: usize,
        line: usize,
        area: CursorArea
    ) -> Result<(), &'static str> {
        let textarea = match area {
            CursorArea::Text(textarea) => { 
                textarea.downcast_mut::<text_area::TextArea>().ok_or("The text displayable is not a TextArea")?
            },
            CursorArea::Frame(_) => {
                return Err("The cursor should display in a text area");
            }
        };
        if self.blink() {
            if self.show() {
                textarea.set_char(col, line, 219)?;
            } else {
                textarea.set_char(col, line, self.underlying_char)?;
            }
            window_manager_alpha::render(None)?
        }
        Ok(())
    }

    fn set_offset_from_end(&mut self, offset: usize) {
        self.offset_from_end = offset;
    }

    fn offset_from_end(&self) -> usize {
        self.offset_from_end
    }

    fn set_underlying_char(&mut self, c: u8) {
        self.underlying_char = c;
    }

    fn underlying_char(&self) -> u8 {
        self.underlying_char
    }
}

/// The generic cursor structure used in the primitive display subsystem.
/// A cursor is a special symbol shown in the text box of a terminal. It indicates the position of character where the next input would be put or the delete operation works on.
/// Terminal invokes its `display` method in a loop to let a cursor blink.
pub struct CursorGeneric {
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

impl CursorGeneric {
    /// Create a new cursor object which is initially enabled. The `blink_interval` is initialized as `DEFAULT_CURSOR_FREQ` however one can change this at any time. `time` is set to current time.
    pub fn new() -> CursorGeneric {
        CursorGeneric {
            enabled: true,
            freq: DEFAULT_CURSOR_FREQ,
            time: tsc_ticks(),
            show: true,
            color: FONT_COLOR,
            offset_from_end: 0,
            underlying_char: 0,
        }
    }
}

impl Cursor for CursorGeneric {
    fn reset(&mut self) {
        self.show = true;
        self.time = tsc_ticks();
    }

    fn enable(&mut self) {
        self.enabled = true;
        self.reset();
    }

    fn disable(&mut self) {
        self.enabled = false;
    }

    fn blink(&mut self) -> bool {
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

    fn show(&self) -> bool {
        self.enabled && self.show
    }

    fn display(
        &mut self,
        coordinate: Coord,
        column: usize,
        line: usize,
        area: CursorArea,
    ) -> Result<(), &'static str> {
        let framebuffer = match area {
            CursorArea::Text(_) => { return Err("The cursor should display in a framebuffer") },
            CursorArea::Frame(fb) => fb
        };
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

        Ok(())
    }

    fn set_offset_from_end(&mut self, offset: usize) {
        self.offset_from_end = offset;
    }

    fn offset_from_end(&self) -> usize {
        self.offset_from_end
    }

    fn set_underlying_char(&mut self, c: u8) {
        self.underlying_char = c;
    }

    fn underlying_char(&self) -> u8 {
        self.underlying_char
    }
}

/// An area in which a cursor can display in. It can be a `TextDisplayable` or a `FrameBuffer` for different display subsystem.
pub enum CursorArea<'a> {
    Text(&'a mut dyn TextDisplayable),
    Frame(&'a mut dyn FrameBuffer),
}
