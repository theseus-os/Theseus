//! Style and formatting of text displayed in a terminal,
//! following the ANSI, VT100, and xterm standards.

use core::{convert::TryFrom, fmt};
use alloc::borrow::Cow;
use crate::{BackgroundColor, ForegroundColor, ScreenPoint, ScrollbackBufferPoint, UnderlinedColor};

/// The style of text, including formatting and color choice, 
/// for the character(s) displayed in a `Unit`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    /// The color of the text itself.
    color_foreground: ForegroundColor,
    /// The color behind the text.
    color_background: BackgroundColor,
    format_flags: FormatFlags,
}
impl Style {
    pub fn diff<'old, 'new>(&'old self, other: &'new Style) -> StyleDiff<'old, 'new> {
        StyleDiff::new(self, other)
    }
}


/// A representation of the difference between two [`Style`]s.
/// 
/// This implements an [`Iterator`] that successively returns the set of 
/// [`AnsiStyleCodes`] needed to change a terminal emulator from displaying 
/// the `old` style to the `new` style. 
///
/// It first iterates over the complex differences in the two [`FormatFlags`],
/// and then iterates over the remaining differences in color.
pub struct StyleDiff<'old, 'new> {
    stage: u8,
    format_flags_diff: FormatFlagsDiff,
    old: &'old Style,
    new: &'new Style,
}
impl<'old, 'new> StyleDiff<'old, 'new> {
    fn new(old: &'old Style, new: &'new Style) -> Self {
        let mut format_flags_diff = old.format_flags.diff(new.format_flags);
        let fg_color_changed = old.color_foreground != new.color_foreground;
        let bg_color_changed = old.color_background != new.color_background;
        // Adjust the `FormatFlagsDiff` to account for the 2 possible color diffs.
        format_flags_diff.max_differences += 2;
        format_flags_diff.num_differences += fg_color_changed as u32 + bg_color_changed as u32;
        
        StyleDiff {
            stage: 0,
            format_flags_diff,
            old,
            new,
        }
    }
}
impl<'old, 'new> Iterator for StyleDiff<'old, 'new> {
    type Item = AnsiStyleCodes;
    fn next(&mut self) -> Option<Self::Item> {
        // Stage 0: return all the diffs for the format flags.
        if self.stage == 0 {
            if let Some(diff) = self.format_flags_diff.next() {
                return Some(diff);
            } else {
                self.stage = 1;
            }
        }

        // Stage 1: return foreground color diff.
        if self.stage == 1 {
            self.stage = 2;
            if self.format_flags_diff.reset_issued {
                // Optimization: no need to issue a default color parameter after a reset.
                if self.new.color_foreground != ForegroundColor::default() {
                    return Some(AnsiStyleCodes::ForegroundColor(self.new.color_foreground));
                }
            } else {
                // No reset issued, so manually set the new foreground color.
                if self.old.color_foreground != self.new.color_foreground {
                    return Some(AnsiStyleCodes::ForegroundColor(self.new.color_foreground));
                }
            }
        }

        // Stage 2: return background color diff.
        if self.stage == 2 {
            self.stage = 3;
            if self.format_flags_diff.reset_issued {
                // Optimization: no need to issue a default color parameter after a reset.
                if self.new.color_background != BackgroundColor::default() {
                    return Some(AnsiStyleCodes::BackgroundColor(self.new.color_background));
                }
            } else {
                // No reset issued, so manually set the new background color.
                if self.old.color_background != self.new.color_background {
                    return Some(AnsiStyleCodes::BackgroundColor(self.new.color_background));
                }
            }
        }

        // Add new stages here, for handling future additions to `Style`

        None
    }
}

/// A representation of the difference between two [`FormatFlags`].
/// 
/// This implements an [`Iterator`] that successively returns the set of 
/// [`AnsiStyleCodes`] needed to change a terminal emulator from displaying 
/// the `old` format to the `new` format. 
struct FormatFlagsDiff {
    old: FormatFlags,
    new: FormatFlags,
    /// The bit that should be compared in the next iteration.
    next_bit: u32,
    /// A bitmask of the bits that differ between `old` and `new`.
    bits_that_differ: FormatFlags,
    /// The number of differences between `old` and `new`.
    num_differences: u32,
    /// The maximum number of differences that can possibly exist
    /// between `old` and `new`.
    max_differences: u32,
    /// Whether a `Reset` command has already been issued.
    reset_issued: bool,
}
impl FormatFlagsDiff {
    fn new(old: FormatFlags, new: FormatFlags) -> Self {
        let bits_that_differ = old.bits_that_differ(new);
        let num_different_bits = bits_that_differ.bits().count_ones();
        FormatFlagsDiff {
            old,
            new,
            next_bit: 0,
            bits_that_differ,
            num_differences: num_different_bits,
            max_differences: FormatFlags::MAX_BIT,
            reset_issued: false,
        }
    }
}

impl Iterator for FormatFlagsDiff {
    type Item = AnsiStyleCodes;
    fn next(&mut self) -> Option<Self::Item> {
        if self.next_bit >= FormatFlags::MAX_BIT { return None; }

        // Optimization: the set of new flags is empty, so we only need to issue one `Reset`. 
        if !self.reset_issued && self.new.is_empty() && !self.old.is_empty() {
            self.next_bit = FormatFlags::MAX_BIT;
            self.reset_issued = true;
            return Some(AnsiStyleCodes::Reset);
        }

        // If there are more bits that differ than bits that are the same,
        // then it's faster to emit a full `Reset` followed by the parameter for each bit set in `new`.
        if !self.reset_issued && self.num_differences >= self.max_differences / 2 {
            self.reset_issued = true;
            return Some(AnsiStyleCodes::Reset);
        }

        // The regular case: iterate to the `next_bit` and return the style code for that bit. 
        // There are two main cases here:
        //  1. `Reset` has been issued, so we go through all of the bits that are set in `new`
        //      and emit an "enabled" style code for each one. 
        //  2. `Reset` has NOT been issued, so we go through all of the `bits_that_differ`
        //      and emit a style code for each one. 
        //      The style code should be "enabled" if the bit is set in `new`, or "disabled" if not.
        while self.next_bit < FormatFlags::MAX_BIT {
            let bit_mask = FormatFlags::from_bits_truncate(1 << self.next_bit);
            if self.reset_issued && self.new.intersects(bit_mask) {
                // Case 1: `Reset` was issued.
                self.next_bit += 1;
                return bit_mask.to_style_code(true);
            }
            if !self.reset_issued && self.bits_that_differ.intersects(bit_mask) {
                // Case 2: `Reset` was NOT issued.
                self.next_bit += 1;
                let bit_is_set_in_new = self.new.intersects(bit_mask);
                return bit_mask.to_style_code(bit_is_set_in_new);
            }

            self.next_bit += 1;
        }

        None
    }
}

#[test]
fn test_bit_diff1() {
    let old = FormatFlags::BRIGHT | FormatFlags::UNDERLINE | FormatFlags::INVERSE | FormatFlags::HIDDEN;
    let new = FormatFlags::STRIKETHROUGH | FormatFlags::INVERSE; // | FormatFlags::BRIGHT;
    println!("old: {:?}", old);
    println!("new: {:?}", new);
    println!("old - new: {:?}", old-new);
    println!("new - old: {:?}", new-old);
    println!("old XOR new: {:?}", old.bits_that_differ(new));
    for code in old.diff(new) {
        println!("\t{:?}", code);
    }
}

#[test]
fn test_style_diff1() {
    let old_ff = FormatFlags::BRIGHT | FormatFlags::UNDERLINE | FormatFlags::INVERSE | FormatFlags::HIDDEN;
    let old = Style { 
        format_flags: old_ff,
        color_foreground: ForegroundColor(crate::Color::BrightCyan),
        color_background: BackgroundColor(crate::Color::Cyan),
    };

    let new_ff = FormatFlags::empty() | FormatFlags::STRIKETHROUGH;
    // let new_ff = FormatFlags::BRIGHT | FormatFlags::STRIKETHROUGH | FormatFlags::INVERSE;
    let new = Style { 
        format_flags: new_ff,
        color_foreground: Default::default(),
        // color_foreground: ForegroundColor(crate::Color::BrightMagenta),
        color_background: BackgroundColor(crate::Color::Red),
    };

    println!("old: {:?}", old);
    println!("new: {:?}", new);
    println!("");
    for code in old.diff(&new) {
        println!("\t{:?}", code);
    }
}

#[test]
fn test_size() {
    println!("Unit: {}", std::mem::size_of::<crate::Unit>());
    println!("      Character: {}", std::mem::size_of::<crate::Character>());
    println!("      Style: {}", std::mem::size_of::<Style>());
    println!("      FormatFlags: {}", std::mem::size_of::<FormatFlags>());
    println!("      Color: {}", std::mem::size_of::<crate::Color>());
}




/// The set of all possible ANSI escape codes for setting text style.
///
/// This is also referred to as Select Graphic Rendition (SGR) parameters or Display Attributes.
///
/// Note that terminal emulators may not support all of these codes;
/// in general, only the first 9 codes are supported (and their `Not*` variants that disable them).
///
/// See a list of all such parameters here:
/// <https://en.wikipedia.org/wiki/ANSI_escape_code#SGR_(Select_Graphic_Rendition)_parameters>
#[derive(Debug)]
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
    /// The text will be underlined twice, or, depending on the terminal,
    /// this will disable `Bright`. 
    ///
    /// Note: it is better to use `NotBrightNorDim` to disable `Bright` 
    ///       as that escape code is more widely supported than this one.
    DoulbeUnderlinedOrNotBright,
    /// Normal font intensity: Disables `Bright` or `Dim`.
    NotBrightNorDim, 
    /// Normal font sytle: Disables `Italic` or `Fraktur`.
    NotItalicNorFraktur,
    /// Disables `Underline` or `DoubleUnderline`.
    NotUnderlined,
    /// Disables `Blink` or `BlinkRapid`.
    NotBlink, 
    /// Proportional spacing, which sets the Teletex character set: <https://en.wikipedia.org/wiki/ITU_T.61>.
    /// This is a different text encoding that is not used and has no effect on terminals. 
    _ProportionalSpacing,
    /// Disables `Inverse`: foreground colors and background colors are used as normal.
    NotInverse,
    /// Disables Hidden``: text is displayed as normal. Sometimes called reveal.
    NotHidden, 
    /// Disables `Strikethrough`: text is not crossed out.
    NotStrikethrough,
    /// Set the foreground color: the color the text will be displayed in.
    /// To set it back to the default, use `ForegroundColor(Color::Default)`.
    ForegroundColor(ForegroundColor),
    /// Set the background color: the color displayed behind the text.
    /// To set it back to the default, use `BackgroundColor(Color::Default)`.
    BackgroundColor(BackgroundColor),
    /// Disables `_ProportionalSpacing`.
    _NotProportionalSpacing,
    /// The text will be displayed with a rectangular box surrounding it.
    Framed,
    /// The text will be displayed with a circle or oval surrounding it.
    Circled,
    /// The text will be overlined: displayed with a line on top (like underlined).
    Overlined,
    /// Disables `Framed` or `Circled`.
    NotFramedOrCircled,
    /// Disabled `Overlined`.
    NotOverlined,
    /// Sets the underline color. 
    /// Without this, the underline color will be the same as the text color.
    /// To set it back to the default, use `UnderlinedColor(Color::Default)`.
    UnderlinedColor(UnderlinedColor),
    _IdeogramUnderlined,
    _IdeogramDoubleUnderlined,
    _IdeogramOverlined,
    _IdeogramDoubleOverlined,
    _IdeogramStressMarking,
    /// Disables all Ideogram styles.
    _NotIdeogram,
    Superscript,
    Subscript,
    /// Disables `Superscript` or `Subscript`.
    NotSuperOrSubscript,
}

impl AnsiStyleCodes {
    pub const ESCAPE_PREFIX: &'static [u8] = b"\x1B[";
    pub const ESCAPE_DELIM:  &'static [u8] = b";";
    pub const ESCAPE_SUFFIX: &'static [u8] = b"m";

    pub fn to_escape_code(&self) -> Cow<'static, str> {
        match self {
            Self::Reset                     => "0".into(),
            Self::Bright                    => "1".into(),
            Self::Dim                       => "2".into(),
            Self::Italic                    => "3".into(),
            Self::Underlined                => "4".into(),
            Self::Blink                     => "5".into(),
            Self::BlinkRapid                => "6".into(),
            Self::Inverse                   => "7".into(),
            Self::Hidden                    => "8".into(),
            Self::Strikethrough             => "9".into(),
            Self::PrimaryFont               => "10".into(),
            Self::AlternateFont(0)          => "10".into(),
            Self::AlternateFont(1)          => "11".into(),
            Self::AlternateFont(2)          => "12".into(),
            Self::AlternateFont(3)          => "13".into(),
            Self::AlternateFont(4)          => "14".into(),
            Self::AlternateFont(5)          => "15".into(),
            Self::AlternateFont(6)          => "16".into(),
            Self::AlternateFont(7)          => "17".into(),
            Self::AlternateFont(8)          => "18".into(),
            Self::AlternateFont(_9_and_up)  => "19".into(),
            Self::Fraktur                   => "20".into(),
            Self::DoulbeUnderlinedOrNotBright => "21".into(),
            Self::NotBrightNorDim           => "22".into(),
            Self::NotItalicNorFraktur       => "23".into(),
            Self::NotUnderlined             => "24".into(),
            Self::NotBlink                  => "25".into(),
            Self::_ProportionalSpacing      => "26".into(),
            Self::NotInverse                => "27".into(),
            Self::NotHidden                 => "28".into(),
            Self::NotStrikethrough          => "29".into(),
            Self::ForegroundColor(fgc)      => fgc.to_escape_code(), // Covers "30"-"39" and "90"-"97"
            Self::BackgroundColor(bgc)      => bgc.to_escape_code(), // Covers "40"-"49" and "100"-"107"
            Self::_NotProportionalSpacing   => "50".into(),
            Self::Framed                    => "51".into(),
            Self::Circled                   => "52".into(),
            Self::Overlined                 => "53".into(),
            Self::NotFramedOrCircled        => "54".into(),
            Self::NotOverlined              => "55".into(),
            // 56 - 57 are unknown
            Self::UnderlinedColor(ulc)      => ulc.to_escape_code(), // Covers "58"-"59"
            Self::_IdeogramUnderlined       => "60".into(),
            Self::_IdeogramDoubleUnderlined => "61".into(),
            Self::_IdeogramOverlined        => "62".into(),
            Self::_IdeogramDoubleOverlined  => "63".into(),
            Self::_IdeogramStressMarking    => "64".into(),
            Self::_NotIdeogram              => "65".into(),
            // 66 - 72 are unknown
            Self::Superscript               => "73".into(),
            Self::Subscript                 => "74".into(),
            Self::NotSuperOrSubscript       => "75".into(),
            // 76 - 89 are unknown
            // 90 - 97 are covered by `Self::ForegroundColor`
            // 98 - 99 are unknown
            // 100 - 107 are covered by `Self::BackgroundColor`
            // 108 and up are unknown
        }
    }
}

impl fmt::Display for AnsiStyleCodes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_escape_code())
    }
}


bitflags! {
    /// The flags that describe the formatting of a given text character.
    ///
    /// This set of flags is completely self-contained within each `Unit`
    /// and does not need to reference any previous `Unit`'s flag as an anchor.
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
        const INVERSE                   = 1 << 5;
        /// If set, this character is not displayed at all,
        /// only a blank box (in the specified background color) will be displayed.
        const HIDDEN                    = 1 << 6;
        /// If set, this character is displayed with a strike-through, i.e.,
        /// with a line crossing it out.
        const STRIKETHROUGH             = 1 << 7;
    }
}

impl FormatFlags {
    /// The max bit index of the `FormatFlags` type.
    const MAX_BIT: u32 = 8;
    
    /// Returns a bit mask of the bits that differ between `self` and `other`,
    /// in which the bit is set if that bit was different across `self` vs `other`.
    ///
    /// This is merely a bit-wise logical XOR of `self` and `other`. 
    pub fn bits_that_differ(self, other: FormatFlags) -> FormatFlags {
        self ^ other
    }

    fn diff(self, other: FormatFlags) -> FormatFlagsDiff {
        FormatFlagsDiff::new(self, other)
    }

    /// Returns the style code required to enable or disable the style
    /// given by the single bit that is set in this `Format`Flags` (`self`). 
    /// 
    /// Returns `None` if more than one bit is set in this `FormatFlags` (`self`).
    fn to_style_code(self, enable: bool) -> Option<AnsiStyleCodes> {
        let code = match (self, enable) {
            (FormatFlags::BRIGHT, true)            => AnsiStyleCodes::Bright,
            (FormatFlags::BRIGHT, false)           => AnsiStyleCodes::NotBrightNorDim,
            (FormatFlags::DIM, true)               => AnsiStyleCodes::Dim,
            (FormatFlags::DIM, false)              => AnsiStyleCodes::NotBrightNorDim,
            (FormatFlags::ITALIC, true)            => AnsiStyleCodes::Italic,
            (FormatFlags::ITALIC, false)           => AnsiStyleCodes::NotItalicNorFraktur,
            (FormatFlags::UNDERLINE, true)         => AnsiStyleCodes::Underlined,
            (FormatFlags::UNDERLINE, false)        => AnsiStyleCodes::NotUnderlined,
            (FormatFlags::BLINK, true)             => AnsiStyleCodes::Blink,
            (FormatFlags::BLINK, false)            => AnsiStyleCodes::NotBlink,
            (FormatFlags::INVERSE, true)           => AnsiStyleCodes::Inverse,
            (FormatFlags::INVERSE, false)          => AnsiStyleCodes::NotInverse,
            (FormatFlags::HIDDEN, true)            => AnsiStyleCodes::Hidden,
            (FormatFlags::HIDDEN, false)           => AnsiStyleCodes::NotHidden,
            (FormatFlags::STRIKETHROUGH, true)     => AnsiStyleCodes::Strikethrough,
            (FormatFlags::STRIKETHROUGH, false)    => AnsiStyleCodes::NotStrikethrough,
            // When more bit flags exist, add those cases here.
            _ => return None,
        };
        Some(code)
    }
}


/// The set of ASCII values that are non-printable characters 
/// and require special handling by a terminal emulator. 
pub struct AsciiControlCodes;

// Use associated consts instead of an enum for easy matching.
#[allow(non_upper_case_globals)]
impl AsciiControlCodes {
    /// (BEL) Plays a terminal bell or beep.
    ///
    /// `Ctrl + G`, or `'\a'`.
    pub const Bell: u8 = 0x07;
    /// (BS) Moves the cursor backwards by one unit/character, but does not remove it.
    /// Note that this is different than the typical behavior of the "Backspace" key on a keyboard.
    ///
    /// `Ctrl + H`, or `'\b'`.
    pub const Backspace: u8 = 0x08;
    /// (HT) Inserts a horizontal tab.
    ///
    /// `Ctrl + I`, or `'\t'`.
    pub const Tab: u8 = 0x09;
    /// (LF) Moves the cursor to the next line, i.e., Line feed, or new line / newline.
    ///
    /// `Ctrl + J`, or `'\n'`.
    pub const LineFeed: u8 = 0x0A;
    /// (VT) Inserts a vertical tab.
    ///
    /// `Ctrl + K`, or `'\v'`.
    pub const VerticalTab: u8 = 0x0B;
    /// (FF) Inserts a page break (form feed) to move the cursor/prompt to the beginning of a new page (screen).
    ///
    /// `Ctrl + L`, or `'\f'`.
    pub const PageBreak: u8 = 0x0C;
    /// (CR) Moves the cursor to the beginning of the line, i.e., carriage return.
    ///
    /// `Ctrl + M`, or `'\r'`.
    pub const CarriageReturn: u8 = 0x0D;
    /// (ESC) The escape character.
    ///
    /// `ESC`, or `'\e'`.
    pub const Escape: u8 = 0x1B;
    /// (DEL) Backwards-deletes the character before (to the left of) the cursor.
    /// This is equivalent to what the Backspace key on a keyboard typically does.
    ///
    /// `DEL`.
    pub const BackwardsDelete: u8 = 0x7F;
}


/// The set of "frequently-supported" commands to switch terminal modes.
///
/// These are sometimes referred to as "ECMA-48" modes or commands.
pub struct ModeSwitch;
#[allow(non_upper_case_globals)]
impl ModeSwitch {
    /// (DECCRM) Display control characters.
    /// This is off by default.
    pub const DisplayControlChars: u8 = b'3';

    /// (DECIM) Set insert mode.
    /// This is off by default, meaning the terminal is in replace mode.
    pub const InsertMode: u8 = b'4';

    /// (LF/NL) Automatically follow a Line Feed (LF), Vertical Tab (VT),
    /// and Form Feed (FF) with a Carriage Return (CR).
    /// This is off by default.
    pub const AutomaticCarriageReturn: &'static [u8; 2] = b"20";

    /// If this value comes after one of the above command values,
    /// it means that the mode should be set, replacing the default value.
    pub const SET_SUFFIX: u8 = b'h';

    /// If this value comes after one of the above command values,
    /// it means that the mode should be "unset" or "reset" to the default.
    pub const RESET_SUFFIX: u8 = b'l';
}

pub struct StatusReportCommands;
#[allow(non_upper_case_globals)]
impl StatusReportCommands {
    /// (DSR) Queries the terminal device for its status.
    /// A reply of `"ESC [ 0 n"` indicates the terminal is okay.
    pub const DeviceStatusRequest: u8 = b'5';

    /// The response to a [`DeviceStatusRequest`] indicating the terminal device is Ok.
    pub const DeviceStatusOk: u8 = b'0';

    /// (CSR) Queries the terminal device for a cursor position report.
    /// A reply will be `"ESC [ y ; x R"`, in which `(x,y)` is the cursor position.
    pub const CursosPositionReport: u8 = b'6';

    /// The value that comes after one of the above command values.
    pub const SUFFIX: u8 = b'n';
}
