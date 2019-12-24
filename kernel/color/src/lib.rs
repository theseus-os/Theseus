#![no_std]
extern crate spin;
extern crate frame_buffer;

use frame_buffer::{RGBPixel, AlphaPixel};


/// predefined black
pub const BLACK: Color = rgba_color(0);
/// predefined white
pub const WHITE: Color = rgba_color(0x00FFFFFF);

/// This structure represents an RGBA color value. It can turn into an alpha pixel or a pixel for framebuffers that does not support the alpha channel.
#[derive(Clone, Copy)]
pub struct Color {
    /// 0 is opaque while 0xFF is transparent
    pub alpha: u8,
    pub red: u8,
    pub green: u8,
    pub blue: u8
}

pub const fn rgba_color(color: u32) -> Color {
    Color {
        alpha: (color >> 24) as u8,
        red: (color >> 16) as u8,
        green: (color >> 8) as u8,
        blue: color as u8,
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

