//! Definitions for ANSI color codes used in terminals.
//!
//! This defines dedicated types for the foreground colors, background colors,
//! and underline colors used in terminals.
//!
//! Includes supports the following color sets:
//! * 3-bit colors: Black, Red, Green, Yellow, Blue, Magenta, Cyan, White.
//! * 4-bit colors: the "bright" versions of the above 3-bit colors.
//! * 8-bit colors: the 256 colors defined by the `xterm-256color` standard.
//! * 24-bit "TrueColor": full RGB colors, in which each channel has 8 bits.
//!

use alloc::borrow::Cow;

/// The set of colors that can be displayed by a terminal emulator. 
/// 
/// * The first 8 variants are 3-bit colors, supported on every terminal emulator. 
/// * The next 8 variants are 4-bit colors, which are brightened (or bold) versions of the first 8.
/// * After that, the 8-bit color variant accepts any value from 0 to 256, 
///   in which values of 0-15 are the same as the first 16 variants of this enum
/// * Finally, the 24-bit color variant accepts standard RGB values. 
///
/// See here for the set of colors: <https://en.wikipedia.org/wiki/ANSI_escape_code#Colors>
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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
    /// True pure white. 
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

    /// The default color, which is generally unspecified
    /// and depends on the context in which it is used.
    Default,
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
impl Default for Color {
    fn default() -> Self {
        Color::Default
    }
}


/// A wrapper type around [`Color`] that is used in [`crate::AnsiStyleCodes`]
/// to set the foreground color (for displayed text).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
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
            // "38" is used by 8-bit and 24-bit colors
            Color::Default                          => "39".into(),
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
impl From<Color> for ForegroundColor {
    fn from(c: Color) -> Self {
        ForegroundColor(c)
    }
}

/// A wrapper type around [`Color`] that is used in [`crate::AnsiStyleCodes`]
/// to set the background color (behind displayed text).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
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
            // "48" is used by 8-bit and 24-bit colors
            Color::Default                          => "49".into(),
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
impl From<Color> for BackgroundColor {
    fn from(c: Color) -> Self {
        BackgroundColor(c)
    }
}


/// A wrapper type around [`Color`] that is used in [`crate::AnsiStyleCodes`]
/// to set the color of the underline for underlined text.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct UnderlinedColor(pub Color);
impl UnderlinedColor {
    const ANSI_ESCAPE_UDERLINED_COLOR: &'static str = "58";
    
    pub fn to_escape_code(self) -> Cow<'static, str> {
        match self.0 {
            Color::Default => "59".into(),
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
impl From<Color> for UnderlinedColor {
    fn from(c: Color) -> Self {
        UnderlinedColor(c)
    }
}


const ANSI_ESCAPE_8_BIT_COLOR: &str = "5";
const ANSI_ESCAPE_24_BIT_COLOR: &str = "2";
