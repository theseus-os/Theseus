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

use core::cmp::max;
use core::ops::DerefMut;
use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use event_types::Event;



/// A whole unbroken line of characters, inclusive of control/escape sequences and newline characters. 
/// 
struct Line {
    /// Inclusive of the actual newline character at the end.
    /// Thus, an empy line of 
    s: String,
    /// The number of character spaces required to display this entire `Line`,
    /// i.e., the size of this `Line` in characters excluding 
    displayed_size: usize,
}


/// A text-based terminal that supports the ANSI, xterm, VT100, and other standards. 
pub struct TextTerminal {
    /// The buffer of all content that is currently displayed or has been previously displayed
    /// on this terminal's screen, including in-band control and escape sequences.
    /// This is what should be written out directly to the terminal backend.
    ///
    /// Because this includes control/escape sequences in addition to regular characters,
    /// the size of this scrollback buffer cannot be used to calculate line wrap lengths or scroll/cursor positions.
    scrollback_buffer: Vec<Line>,

    units: Vec<Unit>,

    /// The width of this terminal's screen, i.e. how many columns of characters it can display. 
    screen_width: u16,
    /// The height of this terminal's screen, i.e. how many rows of characters it can display. 
    screen_height: u16,

    /// Indicates whether the text display is displaying the last part of the scrollback buffer slice
    is_scroll_end: bool,
    /// The starting index of the scrollback buffer string slice that is currently being displayed on the text display
    scroll_start_idx: usize,
    // /// The cursor of the terminal.
    // cursor: Cursor,

    // backend: TerminalBackend,
}

impl TextTerminal {
    pub fn new() -> TextTerminal {
        unimplemented!()
    }

    /// Resizes this terminal's screen to be `width` columns and `height` rows (lines),
    /// in units of *number of characters*.
    ///
    /// This does not automatically flush the terminal, redisplay its output, or recalculate its cursor position.
    ///
    /// Note: the minimum width and height is `2`. 
    /// Values lower than that will be bumped up to `2`.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.screen_width = max(2, width);
        self.screen_height = max(2, height);
    }

    /// Returns the size `(width, height)` of this terminal's screen, in number of characters. 
    pub fn size(&self) -> (u16, u16) {
        (self.screen_width, self.screen_height)
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
enum Character {
    Single(char),
    Multi(String),
}


/// A `Unit` is a single character block displayed in the terminal.
///
/// Some terminal emulators call this structure a `cell`, but this is different from the concept of a `cell`
/// because a `Unit` will **always** represent
///
/// The terminal's text buffer (scrollback buffer) is simply a sequence of `Unit`s,
/// which is stored as a vector but logically represented as a 2-D matrix of `Unit`s:
/// ```ignore
/// [[Unit; SCREEN_WIDTH]; SCREEN_HEIGHT]]
/// ```
/// This representation is needed to support dynamically-resizable screens of terminal text. 
///
/// Displayable control/escape sequences, i.e., those that affect text style,
/// **DO** exist as `Unit`s and are combined into a single `Unit` with the next non-escape/control character,
/// such as a regular ASCII character. 
/// 
/// Non-displayable control/escape sequences, i.e., bells, backspace, delete, etc,
/// are **NOT** saved as `Unit`s in the terminal's scrollback buffer,
/// as they cannot be displayed and are simply transient actions.
pub struct Unit {
    /// The displayable character(s) held in this `Unit`.
    character: Character,
    format_flags: FormatFlags,
    color_foreground: Color,
    color_background: Color,
}




/// The set of all possible ANSI escape codes for setting text style.
///
/// Note that Theseus's terminal emulator(s) may not support all of these style codes.
///
/// List of style codes here: <https://en.wikipedia.org/wiki/ANSI_escape_code>
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
    /// The text will blink slowly, under 150 blinks per minute.
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
            // 56 unknown
            // 57 unknown
            Self::UnderlinedColor(ulc)     => ulc.to_escape_code(), // Covers "58"
            Self::DefaultUnderlinedColor   => "59".into(),
            Self::IdeogramUnderlined       => "60".into(),
            Self::IdeogramDoubleUnderlined => "61".into(),
            Self::IdeogramOverlined        => "62".into(),
            Self::IdeogramDoubleOverlined  => "63".into(),
            Self::IdeogramStressMarking    => "64".into(),
            Self::NoIdeogram               => "65".into(),
            // 66 unknown
            // 67 unknown
            // 68 unknown
            // 69 unknown
            // 70 unknown
            // 71 unknown
            // 72 unknown
            Self::Superscript              => "73".into(),
            Self::Subscript                => "74".into(),
            Self::NoSuperOrSubscript       => "75".into(),

        }
    }
}


bitflags! {
    ///
    /// Note: the order of the flags is the same as the standard ANSI escape codes,
    ///       but the values are not the same because this is a bitfield. 
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

/*
impl FormatFlags {
    pub fn to_escape_sequence(self) -> String {
        let capacity = self.bits().count_ones() * 2;
        let mut seq = String::with_capacity(capacity as usize);
        if self.contains(Self::BRIGHT) { seq.push(AnsiFormatCodes::Bright) };
        seq
        /*
        let mut empty = true;

        if self.contains(Self::BRIGHT) { 
            args = format_args!("{}", args);
        } else {
            panic!("")
        }
        args
        // format_args!("{}{}{}{}{}{}{}{}{}",
        //     self.contains(other)
        // )
        */
    }
}
*/

///
/// See here for the set of colors: <https://gist.github.com/fnky/458719343aabd01cfb17a3a4f7296797>
///
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
    /// 8-bit color, used in xterm.
    /// This is sometimes referred to as a Palette color.
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
            Color::Color8Bit(c_8bit)                => format!(
                "{};{};{}",
                Self::ANSI_ESCAPE_UDERLINED_COLOR, ANSI_ESCAPE_8_BIT_COLOR, c_8bit
            ).into(),
            Color::RGB { red, green, blue }         => format!(
                "{};{};{};{};{}",
                Self::ANSI_ESCAPE_UDERLINED_COLOR, ANSI_ESCAPE_24_BIT_COLOR, red, green, blue
            ).into(),
            
            // This mode only supports parameters of 8-bit and 24-bit (RBG) colors,
            // so we must convert 4-bit colors into 8-bit colors first.
            Color::Black                            => Self(Color::Color8Bit(0)).to_escape_code(),
            Color::Red                              => Self(Color::Color8Bit(1)).to_escape_code(),
            Color::Green                            => Self(Color::Color8Bit(2)).to_escape_code(),
            Color::Yellow                           => Self(Color::Color8Bit(3)).to_escape_code(),
            Color::Blue                             => Self(Color::Color8Bit(4)).to_escape_code(),
            Color::Magenta                          => Self(Color::Color8Bit(5)).to_escape_code(),
            Color::Cyan                             => Self(Color::Color8Bit(6)).to_escape_code(),
            Color::White                            => Self(Color::Color8Bit(7)).to_escape_code(),
            Color::BrightBlack                      => Self(Color::Color8Bit(8)).to_escape_code(),
            Color::BrightRed                        => Self(Color::Color8Bit(9)).to_escape_code(),
            Color::BrightGreen                      => Self(Color::Color8Bit(10)).to_escape_code(),
            Color::BrightYellow                     => Self(Color::Color8Bit(11)).to_escape_code(),
            Color::BrightBlue                       => Self(Color::Color8Bit(12)).to_escape_code(),
            Color::BrightMagenta                    => Self(Color::Color8Bit(13)).to_escape_code(),
            Color::BrightCyan                       => Self(Color::Color8Bit(14)).to_escape_code(),
            Color::BrightWhite                      => Self(Color::Color8Bit(15)).to_escape_code(),
        }
    }
}

const ANSI_ESCAPE_8_BIT_COLOR: &'static str = "5";
const ANSI_ESCAPE_24_BIT_COLOR: &'static str = "2";
