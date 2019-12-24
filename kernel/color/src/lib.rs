#![no_std]
extern crate spin;
extern crate frame_buffer;

use frame_buffer::{RGBPixel, AlphaPixel};

/// This structure represents an RGBA color value. It can turn into an alpha pixel or a pixel for framebuffers that does not support the alpha channel.
#[derive(Clone, Copy)]
pub struct Color {
    /// 0 is opaque while 0xFF is transparent
    pub alpha: u8,
    pub red: u8,
    pub green: u8,
    pub blue: u8
}

impl Color {
    /// Sets the transparency value of the color. 0 means the color is opaque
    pub fn set_transparency(&mut self, trans: u8) {
        self.alpha = trans;
    }

    /// Gets the tranparency value of the color. 0 means the color of opaque
    pub fn get_transparency(&self) -> u8 {
        self.alpha
    }
}

pub enum ColorName {
    Black = 0x000000,
    Blue = 0x0000FF,
    Green = 0x00FF00,
    Cyan = 0x00FFFF,
    Red = 0xFF0000,
    Magenta = 0xFF00FF,
    Brown = 0xA52A2A,
    LightGray = 0xD3D3D3,
    DarkGray = 0xA9A9A9,
    LightBlue = 0xADD8E6,
    LightGreen = 0x90EE90,
    LightCyan = 0xE0FFFF,
    Pink = 0xFFC0CB,
    Yellow = 0xFFFF00,
    White = 0xFFFFFF,
    Transparent = 0xFF000000,
}

pub const fn new_color(color: u32) -> Color {
    Color {
        alpha: (color >> 24) as u8,
        red: (color >> 16) as u8,
        green: (color >> 8) as u8,
        blue: color as u8,
    }
}

impl From<ColorName> for Color {
    fn from(name: ColorName) -> Color {
        new_color(name as u32)
    }
}

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
