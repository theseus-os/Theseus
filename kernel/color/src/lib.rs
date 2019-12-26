#![no_std]
extern crate frame_buffer;

use frame_buffer::{RGBPixel, AlphaPixel};

/// This structure represents an RGBA color value. It can turn into an alpha pixel or a pixel for framebuffers that does not support the alpha channel.
#[derive(Clone, Copy)]
pub struct Color {
    /// 0 is opaque and 0xFF is transparent
    alpha: u8,
    red: u8,
    green: u8,
    blue: u8
}

impl Color {
    /// Sets the transparency value of the color. 0 means the color is opaque
    pub fn set_transparency(&mut self, trans: u8) {
        self.alpha = trans;
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

pub const fn new_color(color: u32) -> Color {
    Color {
        alpha: (color >> 24) as u8,
        red: (color >> 16) as u8,
        green: (color >> 8) as u8,
        blue: color as u8,
    }
}

pub const BLACK: Color = new_color(0x000000);
pub const BLUE: Color = new_color(0x0000FF);
pub const GREEN: Color = new_color(0x00FF00);
pub const CYAN: Color = new_color(0x00FFFF);
pub const RED: Color = new_color(0xFF0000);
pub const MAGENTA: Color = new_color(0xFF00FF);
pub const BROWN: Color = new_color(0xA52A2A);
pub const LIGHTGRAY: Color = new_color(0xD3D3D3);
pub const DARKGRAY: Color = new_color(0xA9A9A9);
pub const LIGHTBLUE: Color = new_color(0xADD8E6);
pub const LIGHTGREEN: Color = new_color(0x90EE90);
pub const LIGHTCYAN: Color = new_color(0xE0FFFF);
pub const PINK: Color = new_color(0xFFC0CB);
pub const YELLOW: Color = new_color(0xFFFF00);
pub const WHITE: Color = new_color(0xFFFFFF);
pub const TRANSPARENT: Color = new_color(0xFF000000);

impl From<Color> for RGBPixel {
    fn from(color: Color) -> Self {
        RGBPixel {
            _channel: 0,
            red: color.red,
            green: color.green,
            blue: color.blue
        }
    }
}

impl From<Color> for AlphaPixel {
    fn from(color: Color) -> Self {
        AlphaPixel {
            alpha: color.alpha,
            red: color.red,
            green: color.green,
            blue: color.blue
        }
    }
}
