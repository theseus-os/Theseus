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

    /// Gets the tranparency value of the color. 0 means the color of opaque
    pub fn get_transparency(&self) -> u8 {
        self.alpha
    }
}

#[derive(Clone, Copy)]
pub enum ColorName {
    Black,
    Blue,
    Green,
    Cyan,
    Red,
    Magenta,
    Brown,
    LightGray,
    DarkGray,
    LightBlue,
    LightGreen,
    LightCyan,
    Pink,
    Yellow,
    White,
    Transparent,
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
        match name {
            ColorName::Black => new_color(0x000000),
            ColorName::Blue => new_color(0x0000FF),
            ColorName::Green => new_color(0x00FF00),
            ColorName::Cyan => new_color(0x00FFFF),
            ColorName::Red => new_color(0xFF0000),
            ColorName::Magenta => new_color(0xFF00FF),
            ColorName::Brown => new_color(0xA52A2A),
            ColorName::LightGray => new_color(0xD3D3D3),
            ColorName::DarkGray => new_color(0xA9A9A9),
            ColorName::LightBlue => new_color(0xADD8E6),
            ColorName::LightGreen => new_color(0x90EE90),
            ColorName::LightCyan => new_color(0xE0FFFF),
            ColorName::Pink => new_color(0xFFC0CB),
            ColorName::Yellow => new_color(0xFFFF00),
            ColorName::White => new_color(0xFFFFFF),
            ColorName::Transparent => new_color(0xFF000000),
        }
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
