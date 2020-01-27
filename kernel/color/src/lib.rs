//! A simple representation of the standard RGB color model.
#![no_std]


pub const BLACK: Color = Color::new(0x000000);
pub const BLUE: Color = Color::new(0x0000FF);
pub const GREEN: Color = Color::new(0x00FF00);
pub const CYAN: Color = Color::new(0x00FFFF);
pub const RED: Color = Color::new(0xFF0000);
pub const MAGENTA: Color = Color::new(0xFF00FF);
pub const BROWN: Color = Color::new(0xA52A2A);
pub const LIGHT_GRAY: Color = Color::new(0xD3D3D3);
pub const GRAY: Color = Color::new(0x808080);
pub const DARK_GRAY: Color = Color::new(0xA9A9A9);
pub const LIGHT_BLUE: Color = Color::new(0xADD8E6);
pub const LIGHT_GREEN: Color = Color::new(0x90EE90);
pub const LIGHT_CYAN: Color = Color::new(0xE0FFFF);
pub const PINK: Color = Color::new(0xFFC0CB);
pub const YELLOW: Color = Color::new(0xFFFF00);
pub const WHITE: Color = Color::new(0xFFFFFF);
pub const TRANSPARENT: Color = Color::new(0xFF000000);


/// This structure represents a color value in the standard RGB color model,
/// comprised of red, blue, green, and transparency components.
#[derive(Debug, Clone, Copy)]
pub struct Color {
    /// 0 is opaque and 0xFF is transparent
    alpha: u8,
    red: u8,
    green: u8,
    blue: u8
}

impl Color {
    /// Creates a new `Color` structure from a 4 bytes ARGB representation.
    pub const fn new(color: u32) -> Color {
        Color {
            alpha: (color >> 24) as u8,
            red: (color >> 16) as u8,
            green: (color >> 8) as u8,
            blue: color as u8,
        }
    }

    /// Sets the transparency of the color, in which `0` is opaque and `0xFF` is transparent.
    #[inline(always)]
    pub fn set_transparency(&mut self, alpha: u8) {
        self.alpha = alpha;
    }

    /// Returns the transparency component of this `Color` as a `u8` value.
    #[inline(always)]
    pub fn transparency(&self) -> u8 {
        self.alpha
    }

    /// Returns the red component of this `Color` as a `u8` value.
    #[inline(always)]
    pub fn red(&self) -> u8 {
        self.red
    }

    /// Returns the blue component of this `Color` as a `u8` value.
    #[inline(always)]
    pub fn blue(&self) -> u8 {
        self.blue
    }

    /// Returns the green component of this `Color` as a `u8` value.
    #[inline(always)]
    pub fn green(&self) -> u8 {
        self.green
    }
}

impl PartialEq for Color {
    fn eq(&self, other: &Color) -> bool {
        self.alpha == other.alpha &&
            self.red == other.red &&
            self.green == other.green &&
            self.blue == other.blue
    }
}

impl Eq for Color { }
