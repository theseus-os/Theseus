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
#[macro_use] extern crate bitflags;
extern crate event_types;
extern crate unicode_width;
extern crate bare_io;

#[cfg(test)]
#[macro_use] extern crate std;


use core::cmp::max;
use core::mem::size_of;
use core::ops::{Deref, DerefMut};
use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use bare_io::Write;
use event_types::Event;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};



/// The position ("viewport") that the terminal is currently scrolled to. 
/// 
/// By default, the terminal starts at the `Bottom`, 
/// such that it will auto-scroll upon new characters being displayed.
pub enum ScrollPosition {
    /// The terminal is scrolled all the way up.
    ///
    /// In this position, the terminal screen "viewport" is locked
    /// and will **NOT** auto-scroll down to show any newly-outputted text.
    Top,
    /// The terminal is scrolled to a specific point,
    /// for which the starting position is given by the `Unit`
    /// located at the specified `line` and `column`:
    /// * `line`: the index into the terminal's `scrollback_buffer`,
    /// * `column`: the index into that `Line`. 
    ///
    /// In this position, the terminal screen "viewport" is locked
    /// and will **NOT** auto-scroll down to show any newly-outputted text.
    UnitIndex { line: usize, column: usize },
    /// The terminal position is scrolled all the way down.
    ///
    /// In this position, the terminal screen "viewport" is **NOT** locked
    /// and will auto-scroll down to show any newly-outputted text.
    Bottom,
}
impl Default for ScrollPosition {
    fn default() -> Self {
        ScrollPosition::Bottom
    }
}


/// An entire unbroken line of characters (`Unit`s) that has been written to a terminal.
///
/// `Line`s *only* end at an actual line break, i.e., a newline character `'\n'`.
///
/// Note that when displaying a `Line`
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
impl Line {
    /// Writes this entire `Line` to the given `writer` output stream.
    ///
    /// Returns the total number of bytes written.
    fn write_line_to<W: Write>(&self, writer: &mut W, mut previous_style: Style) -> bare_io::Result<usize> {
        let mut char_encode_buf = [0u8; 4];
        let mut bytes_written = 0;

        for unit in &self.units {
            /*
            TODO: implement the `diff` function for `Style`
            let diff = unit.style.diff(previous_style);
            bytes_written += diff.write_diff_to(writer)?;
            */
            bytes_written += writer.write(match unit.character {
                Character::Single(ref ch) => ch.encode_utf8(&mut char_encode_buf[..]).as_bytes(),
                Character::Multi(ref s) => s.as_bytes(),
            })?;
        }
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
pub struct TextTerminal<Output: bare_io::Write> {
    /// The buffer of all content that is currently displayed or has been previously displayed
    /// on this terminal's screen, including in-band control and escape sequences.
    /// This is what should be written out directly to the terminal backend.
    ///
    /// Because this includes control/escape sequences in addition to regular characters,
    /// the size of this scrollback buffer cannot be used to calculate line wrap lengths or scroll/cursor positions.
    scrollback_buffer: Vec<Line>,

    /// The width of this terminal's screen, i.e. how many columns of characters it can display. 
    columns: u16,
    /// The height of this terminal's screen, i.e. how many rows of characters it can display. 
    rows: u16,

    /// The starting index of the scrollback buffer string slice that is currently being displayed on the text display
    scroll_position: ScrollPosition,

    // /// The cursor of the terminal.
    // cursor: Cursor,

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
        TextTerminal {
            scrollback_buffer: Vec::new(),
            columns: width,
            rows: height,
            scroll_position: ScrollPosition::default(),
            backend,
        }
    }

    /// Resizes this terminal's screen to be `width` columns and `height` rows (lines),
    /// in units of *number of characters*.
    ///
    /// This does not automatically flush the terminal, redisplay its output, or recalculate its cursor position.
    ///
    /// Note: values will be adjusted to the minimum width and height of `2`. 
    pub fn resize(&mut self, width: u16, height: u16) {
        self.columns = max(2, width);
        self.rows = max(2, height);
    }

    /// Returns the size `(columns, rows)` of this terminal's screen, 
    /// in units of displayable characters.
    pub fn size(&self) -> (u16, u16) {
        (self.columns, self.rows)
    }


    /// Flushes the entire viewable region of the terminal's screen
    /// to the backend output stream.
    ///
    /// No caching or performance optimizations are used. 
    pub fn flush(&mut self) -> bare_io::Result<usize> {

        // self.backend.write(buf)
        unimplemented!()
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
pub struct Unit {
    /// The displayable character(s) held in this `Unit`.
    character: Character,
    style: Style,
}
impl Deref for Unit {
    type Target = Character;
    fn deref(&self) -> &Self::Target {
        &self.character
    }
}

pub struct Style {
    format_flags: FormatFlags,
    color_foreground: Color,
    color_background: Color,
}
impl Style {
    fn diff(&self, other: &Style) -> StyleDiff {
        unimplemented!()

    }
}

struct StyleDiff<'old, 'new> {
    old: &'old Style,
    new: &'new Style,
}


#[test]
fn test_size() {
    println!("Unit: {}", std::mem::size_of::<Unit>());
    println!("      Character: {}", std::mem::size_of::<Character>());
    println!("      Style: {}", std::mem::size_of::<Style>());
    println!("      FormatFlags: {}", std::mem::size_of::<FormatFlags>());
    println!("      Color: {}", std::mem::size_of::<Color>());
}





/// The set of all possible ANSI escape codes for setting text style.
///
/// This is also referred to as Select Graphic Rendition (SGR) parameters or Display Attributes.
///
/// Note that terminal emulators may not support all of these codes.
///
/// See a list of all such parameters here:
/// <https://en.wikipedia.org/wiki/ANSI_escape_code#SGR_(Select_Graphic_Rendition)_parameters>
pub enum AnsiStyleCodes {
    /// Resets or clears all styles.
    Reset,
    /// Bright or bold text.
    Bright,
    /// Dim or faint text.
    Dim,
    /// Italicized text.
    Italic,
    /// Underlined text. 
    Underlined,
    /// The text will blink at slower rate, under 150 blinks per minute.
    Blink,
    /// The text will blink rapidly at a fast rate, over 150 blinks per minute.
    BlinkRapid,
    /// The foreground and background colors will be swapped. 
    /// The text will be displayed using the background color,
    /// while the background will be displayed using the foreground color. 
    Inverse,
    /// The text will be concealed/invisible and not displayed at all.
    /// Only the solid background color will be displayed. 
    Hidden,
    /// The text will be striked through, i.e., crossed out with a line through it.
    Strikethrough,
    /// Sets the font to the primary default font.
    PrimaryFont,
    /// Sets the font to an alternate font. 
    /// There are 10 available choices, from `0` to `9`,
    /// in which `0` is the same as [`Self::PrimaryFont`].
    AlternateFont(u8),
    /// Sets the font to be a blackletter font, which is a calligraphic/angular font 
    /// in a gothic German style rather than the typical Roman/Latin style. 
    /// Example: <https://git.enlightenment.org/apps/terminology.git/commit/?id=02856cbdec511e08cf579b08e906499d9583f018>
    Fraktur,
    /// The text will be underlined twice.
    /// Note: on some terminals, this disables bold text.
    DoubleUnderlined,
    /// Disables Bright or Dim.
    NormalIntensity, 
    /// Disables Italic or Fraktur.
    NormalFont,
    /// Disables Underline or DoubleUnderline.
    NotUnderlined,
    /// Disables Blink or BlinkRapid.
    NotBlink, 
    /// Proportional spacing, which sets the Teletex character set: <https://en.wikipedia.org/wiki/ITU_T.61>.
    /// This is a different text encoding that is not used and has no effect on terminals. 
    _ProportionalSpacing,
    /// Disables Inverse: foreground colors and background colors are used as normal.
    NotInverse,
    /// Disables Hidden: text is displayed as normal. Sometimes called reveal.
    NotHidden, 
    /// Disables Strikethrough: text is not crossed out.
    NotStrikethrough,
    /// Set the foreground color: the color the text will be displayed in.
    ForegroundColor(ForegroundColor),
    /// Sets the foreground color to the default.
    DefaultForegroundColor,
    /// Set the background color: the color displayed behind the text.
    BackgroundColor(ForegroundColor),
    /// Sets the background color to the default.
    DefaultBackgroundColor,
    /// Disables ProportionalSpacing.
    _NotProportionalSpacing,
    /// The text will be displayed with a rectangular box surrounding it.
    Framed,
    /// The text will be displayed with a circle or oval surrounding it.
    Circled,
    /// The text will be overlined: displayed with a line on top (like underlined).
    Overlined,
    /// Disables Framed or Circled.
    NotFramedOrCircled,
    /// Disabled Overlined.
    NotOverlined,
    UnderlinedColor(UnderlinedColor),
    DefaultUnderlinedColor,
    IdeogramUnderlined,
    IdeogramDoubleUnderlined,
    IdeogramOverlined,
    IdeogramDoubleOverlined,
    IdeogramStressMarking,
    /// Disables all Ideogram styles.
    NoIdeogram,
    Superscript,
    Subscript,
    /// Disables Superscript or Subscript.
    NoSuperOrSubscript,
}

impl AnsiStyleCodes {
    pub fn to_escape_code(self) -> Cow<'static, str> {
        match self {
            Self::Reset                    => "0".into(),
            Self::Bright                   => "1".into(),
            Self::Dim                      => "2".into(),
            Self::Italic                   => "3".into(),
            Self::Underlined               => "4".into(),
            Self::Blink                    => "5".into(),
            Self::BlinkRapid               => "6".into(),
            Self::Inverse                  => "7".into(),
            Self::Hidden                   => "8".into(),
            Self::Strikethrough            => "9".into(),
            Self::PrimaryFont              => "10".into(),
            Self::AlternateFont(0)         => "10".into(),
            Self::AlternateFont(1)         => "11".into(),
            Self::AlternateFont(2)         => "12".into(),
            Self::AlternateFont(3)         => "13".into(),
            Self::AlternateFont(4)         => "14".into(),
            Self::AlternateFont(5)         => "15".into(),
            Self::AlternateFont(6)         => "16".into(),
            Self::AlternateFont(7)         => "17".into(),
            Self::AlternateFont(8)         => "18".into(),
            Self::AlternateFont(_over_9)   => "19".into(),
            Self::Fraktur                  => "20".into(),
            Self::DoubleUnderlined         => "21".into(),
            Self::NormalIntensity          => "22".into(),
            Self::NormalFont               => "23".into(),
            Self::NotUnderlined            => "24".into(),
            Self::NotBlink                 => "25".into(),
            Self::_ProportionalSpacing     => "26".into(),
            Self::NotInverse               => "27".into(),
            Self::NotHidden                => "28".into(),
            Self::NotStrikethrough         => "29".into(),
            Self::ForegroundColor(fgc)     => fgc.to_escape_code(), // Covers "30"-"38" and "90"-"97"
            Self::DefaultForegroundColor   => "39".into(),           
            Self::BackgroundColor(bgc)     => bgc.to_escape_code(), // Covers "40"-"48" and "100"-"107"
            Self::DefaultBackgroundColor   => "49".into(),
            Self::_NotProportionalSpacing  => "50".into(),
            Self::Framed                   => "51".into(),
            Self::Circled                  => "52".into(),
            Self::Overlined                => "53".into(),
            Self::NotFramedOrCircled       => "54".into(),
            Self::NotOverlined             => "55".into(),
            // 56 and 57 are unknown
            Self::UnderlinedColor(ulc)     => ulc.to_escape_code(), // Covers "58"
            Self::DefaultUnderlinedColor   => "59".into(),
            Self::IdeogramUnderlined       => "60".into(),
            Self::IdeogramDoubleUnderlined => "61".into(),
            Self::IdeogramOverlined        => "62".into(),
            Self::IdeogramDoubleOverlined  => "63".into(),
            Self::IdeogramStressMarking    => "64".into(),
            Self::NoIdeogram               => "65".into(),
            // 66 - 72 are unknown
            Self::Superscript              => "73".into(),
            Self::Subscript                => "74".into(),
            Self::NoSuperOrSubscript       => "75".into(),
            // 76 - 89 are unknown
            // 90 - 97 
        }
    }
}


bitflags! {
    /// The flags that describe the formatting of a given text character.
    ///
    /// This set of flags is completely self-contained within each `Unit`
    /// and does not need to reference any previous `Unit`'s flag as an anchor.
    ///
    /// Note: the order of the flags is the same as the standard ANSI escape codes,
    ///       but the values are not the same because this is a bitfield. 
    #[derive(Default)]
    pub struct FormatFlags: u8 {
        /// If set, this character is displayed in a bright color, which is sometimes called "bold".
        const BRIGHT                    = 1 << 0;
        /// If set, this character is displayed using a dim or faint color, the opposite of `BRIGHT`.
        const DIM                       = 1 << 1;
        /// If set, this character is displayed in italics.
        const ITALIC                    = 1 << 2;
        /// If set, this character is displayed with a single underline.
        const UNDERLINE                 = 1 << 3;
        /// If set, the unit box where this character is displayed will blink.
        const BLINK                     = 1 << 4;
        /// If set, this character is displayed with inversed/reversed colors:
        /// the foreground character text is displayed using the background color,
        /// while the background is displayed using the foreground color.
        const INVERSED                  = 1 << 5;
        /// If set, this character is not displayed at all,
        /// only a blank box (in the specified background color) will be displayed.
        const HIDDEN                    = 1 << 6;
        /// If set, this character is displayed with a strike-through, i.e.,
        /// with a line crossing it out.
        const STRIKETHROUGH             = 1 << 7;

        // const INVERSE                   = 0b0000_0000_0000_0001;
        // const BOLD                      = 0b0000_0000_0000_0010;
        // const ITALIC                    = 0b0000_0000_0000_0100;
        // const BOLD_ITALIC               = 0b0000_0000_0000_0110;
        // const UNDERLINE                 = 0b0000_0000_0000_1000;
        // const WRAPLINE                  = 0b0000_0000_0001_0000;
        // const WIDE_CHAR                 = 0b0000_0000_0010_0000;
        // const WIDE_CHAR_SPACER          = 0b0000_0000_0100_0000;
        // const DIM                       = 0b0000_0000_1000_0000;
        // const DIM_BOLD                  = 0b0000_0000_1000_0010;
        // const HIDDEN                    = 0b0000_0001_0000_0000;
        // const STRIKETHROUGH             = 0b0000_0010_0000_0000;
        // const LEADING_WIDE_CHAR_SPACER  = 0b0000_0100_0000_0000;
        // const DOUBLE_UNDERLINE          = 0b0000_1000_0000_0000;
    }
}

#[test]
fn test_bit_diff1() {
    let old = FormatFlags::BRIGHT | FormatFlags::UNDERLINE | FormatFlags::HIDDEN;
    let new = FormatFlags::STRIKETHROUGH;
    println!("old: {:?}", old);
    println!("new: {:?}", new);
    println!("old - new: {:?}", old-new);
    println!("new - old: {:?}", new-old);
}

impl FormatFlags {
    /// The max bit index of the `FormatFlags` type.
    const MAX_BIT: u32 = 8;
    
    /// Returns an [`Iterator`] that yields all of the escape sequence parameters
    /// needed to transform the terminal's text format (excluding colors)
    /// from this `FormatFlags` (`self`) to the given `new` `FormatFlags`. 
    pub fn diff(self, new: FormatFlags) -> FormatFlagsDiff {
        FormatFlagsDiff::new(self, new)
    }

    fn bits_that_differ(self, other: FormatFlags) -> FormatFlags {
        self ^ other
    }




}

pub struct FormatFlagsDiff {
    old: FormatFlags,
    new: FormatFlags,
    next_bit: u32,
    num_different_bits: u32,
    reset_issued: bool,
}
impl FormatFlagsDiff {
    fn new(old: FormatFlags, new: FormatFlags) -> Self {
        FormatFlagsDiff {
            old,
            new,
            next_bit: 0,
            num_different_bits: old.bits_that_differ(new).bits().count_ones(),
            reset_issued: false,
        }
    }
}
impl Iterator for FormatFlagsDiff {
    type Item = Cow<'static, str>;
    fn next(&mut self) -> Option<Self::Item> {
        // Optimization: the set of new flags is empty, so we only need to issue one `Reset`. 
        if self.new.is_empty() && !self.old.is_empty() {
            self.next_bit = FormatFlags::MAX_BIT;
            self.reset_issued = true;
            return Some(AnsiStyleCodes::Reset.to_escape_code());
        }

        // If there are more bits that differ than bits that are the same,
        // then it's faster to emit a full `Reset` followed by the parameter for each bit set in `new`.
        if !self.reset_issued && self.num_different_bits >= FormatFlags::MAX_BIT / 2 {
            self.reset_issued = true;
            return Some(AnsiStyleCodes::Reset.to_escape_code());
        }


        TODO FIXME: use the bits_that_differ() function (which is just XOR)
        to determine which bits need to change.
        Then, for those bits, just emit the parameter code that corresponds
        to its value in the `new` bitset

        // If a `Reset` has already been issued (as in the above conditional),
        // then all we have to do is go through each bit that is set in `new`
        // and emit that bit's parameter.
        if self.reset_issued {
            while self.next_bit < FormatFlags::MAX_BIT {
                if self.new.bits() & (1 << self.next_bit) != 0 {


                }
            }

        }
        // If a `Reset` hasn't been issued, we need to go through each bit
        // that differs between `old` to `new` and issue the proper 
        else {

        }
        
        None

    }
}

/// The set of colors that can be displayed by a terminal emulator. 
/// 
/// The first 8 variants are 3-bit colors, supported on every terminal emulator. 
/// The next 8 variants are 4-bit colors, which are brightened (or bold) versions of the first 8.
/// After that, the 8-bit color variant accepts any value from 0 to 256, 
/// in which values of 0-15 are the same as the first 16 variants of this enum
/// Finally, the 24-bit color variant accepts standard RGB values. 
///
/// See here for the set of colors: <https://en.wikipedia.org/wiki/ANSI_escape_code#Colors>
pub enum Color {
    /////////////////////// 2-bit Colors //////////////////////
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    /// More of a light gray/grey. Use `BrightWhite` for true white.
    White,

    /////////////////////// 4-bit Colors //////////////////////
    /// Gray/grey.
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    /// True white. 
    BrightWhite,

    /////////////////////// 8-bit Colors //////////////////////
    /// 8-bit color, as introduced in xterm.
    /// 
    /// * Values of `0` through `15` are identical to the above 16 color variants. 
    /// * The next 216 colors `16` through `231` are arranged into a 6 x 6 x 6 color cube,
    ///   as shown here: <https://en.wikipedia.org/wiki/ANSI_escape_code#8-bit>.
    /// * The last 24 colors `232` through `255` are grayscale steps from dark gray to light. 
    ///
    /// This is sometimes referred to as a Palette color lookup table.
    Color8Bit(u8),

    /////////////////////// 24-bit Colors //////////////////////
    /// True 24-bit RGB color, with 8 bits for each of the red, green, and blue channels.
    RGB { red: u8, green: u8, blue: u8 },
}
impl From<u8> for Color {
    fn from(value: u8) -> Self {
        match value {
            0  => Self::Black,
            1  => Self::Red,
            2  => Self::Green,
            3  => Self::Yellow,
            4  => Self::Blue,
            5  => Self::Magenta,
            6  => Self::Cyan,
            7  => Self::White,
            8  => Self::BrightBlack,
            9  => Self::BrightRed,
            10 => Self::BrightGreen,
            11 => Self::BrightYellow,
            12 => Self::BrightBlue,
            13 => Self::BrightMagenta,
            14 => Self::BrightCyan,
            15 => Self::BrightWhite,
            x  => Self::Color8Bit(x),
        }
    }
}


pub struct ForegroundColor(pub Color);
impl ForegroundColor {
    const ANSI_ESCAPE_FOREGROUND_COLOR: &'static str = "38";

    pub fn to_escape_code(self) -> Cow<'static, str> {
        match self.0 {
            Color::Black                            => "30".into(),
            Color::Red                              => "31".into(),
            Color::Green                            => "32".into(),
            Color::Yellow                           => "33".into(),
            Color::Blue                             => "34".into(),
            Color::Magenta                          => "35".into(),
            Color::Cyan                             => "36".into(),
            Color::White                            => "37".into(),
            Color::BrightBlack                      => "90".into(),
            Color::BrightRed                        => "91".into(),
            Color::BrightGreen                      => "92".into(),
            Color::BrightYellow                     => "93".into(),
            Color::BrightBlue                       => "94".into(),
            Color::BrightMagenta                    => "95".into(),
            Color::BrightCyan                       => "96".into(),
            Color::BrightWhite                      => "97".into(),
            // For better compatibility, reduce 8-bit color codes to their 4-bit representation if possible.
            Color::Color8Bit(c_4bit) if c_4bit < 16 => Self(Color::from(c_4bit)).to_escape_code(),
            Color::Color8Bit(c_8bit)                => format!(
                "{};{};{}",
                Self::ANSI_ESCAPE_FOREGROUND_COLOR, ANSI_ESCAPE_8_BIT_COLOR, c_8bit
            ).into(),
            Color::RGB { red, green, blue }         => format!(
                "{};{};{};{};{}",
                Self::ANSI_ESCAPE_FOREGROUND_COLOR, ANSI_ESCAPE_24_BIT_COLOR, red, green, blue
            ).into(),
        }
    }
}

pub struct BackgroundColor(pub Color);
impl BackgroundColor {
    const ANSI_ESCAPE_BACKGROUND_COLOR: &'static str = "48";
    
    pub fn to_escape_code(self) -> Cow<'static, str> {
        match self.0 {
            Color::Black                            => "40".into(),
            Color::Red                              => "41".into(),
            Color::Green                            => "42".into(),
            Color::Yellow                           => "43".into(),
            Color::Blue                             => "44".into(),
            Color::Magenta                          => "45".into(),
            Color::Cyan                             => "46".into(),
            Color::White                            => "47".into(),
            Color::BrightBlack                      => "100".into(),
            Color::BrightRed                        => "101".into(),
            Color::BrightGreen                      => "102".into(),
            Color::BrightYellow                     => "103".into(),
            Color::BrightBlue                       => "104".into(),
            Color::BrightMagenta                    => "105".into(),
            Color::BrightCyan                       => "106".into(),
            Color::BrightWhite                      => "107".into(),
            // For better compatibility, reduce 8-bit color codes to their 4-bit representation if possible.
            Color::Color8Bit(c_4bit) if c_4bit < 16 => Self(Color::from(c_4bit)).to_escape_code(),
            Color::Color8Bit(c_8bit)                => format!(
                "{};{};{}",
                Self::ANSI_ESCAPE_BACKGROUND_COLOR, ANSI_ESCAPE_8_BIT_COLOR, c_8bit
            ).into(),
            Color::RGB { red, green, blue }         => format!(
                "{};{};{};{};{}",
                Self::ANSI_ESCAPE_BACKGROUND_COLOR, ANSI_ESCAPE_24_BIT_COLOR, red, green, blue
            ).into(),
        }
    }
}

pub struct UnderlinedColor(pub Color);
impl UnderlinedColor {
    const ANSI_ESCAPE_UDERLINED_COLOR: &'static str = "58";
    
    pub fn to_escape_code(self) -> Cow<'static, str> {
        match self.0 {
            Color::Color8Bit(c_8bit) => format!(
                "{};{};{}",
                Self::ANSI_ESCAPE_UDERLINED_COLOR, ANSI_ESCAPE_8_BIT_COLOR, c_8bit
            ).into(),
            Color::RGB { red, green, blue } => format!(
                "{};{};{};{};{}",
                Self::ANSI_ESCAPE_UDERLINED_COLOR, ANSI_ESCAPE_24_BIT_COLOR, red, green, blue
            ).into(),
            
            // This mode only supports parameters of 8-bit and 24-bit (RBG) colors,
            // so we must convert 4-bit colors into 8-bit colors first.
            Color::Black         => Self(Color::Color8Bit(0)).to_escape_code(),
            Color::Red           => Self(Color::Color8Bit(1)).to_escape_code(),
            Color::Green         => Self(Color::Color8Bit(2)).to_escape_code(),
            Color::Yellow        => Self(Color::Color8Bit(3)).to_escape_code(),
            Color::Blue          => Self(Color::Color8Bit(4)).to_escape_code(),
            Color::Magenta       => Self(Color::Color8Bit(5)).to_escape_code(),
            Color::Cyan          => Self(Color::Color8Bit(6)).to_escape_code(),
            Color::White         => Self(Color::Color8Bit(7)).to_escape_code(),
            Color::BrightBlack   => Self(Color::Color8Bit(8)).to_escape_code(),
            Color::BrightRed     => Self(Color::Color8Bit(9)).to_escape_code(),
            Color::BrightGreen   => Self(Color::Color8Bit(10)).to_escape_code(),
            Color::BrightYellow  => Self(Color::Color8Bit(11)).to_escape_code(),
            Color::BrightBlue    => Self(Color::Color8Bit(12)).to_escape_code(),
            Color::BrightMagenta => Self(Color::Color8Bit(13)).to_escape_code(),
            Color::BrightCyan    => Self(Color::Color8Bit(14)).to_escape_code(),
            Color::BrightWhite   => Self(Color::Color8Bit(15)).to_escape_code(),
        }
    }
}

const ANSI_ESCAPE_8_BIT_COLOR: &'static str = "5";
const ANSI_ESCAPE_24_BIT_COLOR: &'static str = "2";


/// The set of ASCII values that are non-printable characters 
/// and require special handling by a terminal emulator. 
pub enum AsciiControlCodes {
    /// (BEL) Plays a terminal bell or beep.
    /// `Ctrl + G`, or `'\a'`.
    Bell         = 0x07,
    /// (BS) Backspaces over the previous character before (to the left of) the cursor.
    /// `Ctrl + H`, or `'\b'`.
    Backspace    = 0x08,
    /// (HT) Inserts a horizontal tab.
    /// `Ctrl + I`, or `'\t'`.
    Tab          = 0x09,
    /// (LF) Moves the cursor to the next line, i.e., Line feed.
    /// `Ctrl + J`, or `'\n'`.
    NewLine      = 0x0A,
    /// (VT) Inserts a vertical tab.
    /// `Ctrl + K`, or `'\v'`.
    VerticalTab  = 0x0B,
    /// (FF) Inserts a page break (form feed) to move the cursor/prompt to the beginning of a new page (screen).
    /// `Ctrl + K`, or `'\v'`.
    NewPage  = 0x0C,
    /// (CR) Moves the cursor to the beginning of the line, i.e., carriage return.
    /// `Ctrl + M`, or `'\r'`.
    CarriageReturn  = 0x0D,
    /// (ESC) The escape character.
    /// `ESC`, or `'\e'`.
    Escape = 0x1B,
    /// (DEL) Deletes the next character after (to the right of) the cursor.
    /// `DEL`.
    Delete = 0x7F,
}