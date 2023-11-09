use color::Color;
use zerocopy::FromBytes;

pub trait Pixel: FromBytes + Copy {}

#[derive(Clone, Copy, FromBytes)]
// This is necessary right?
#[repr(C)]
pub struct AlphaPixel {
    alpha: u8,
    red: u8,
    green: u8,
    blue: u8,
}

impl Pixel for AlphaPixel {}

// TODO: Constify
impl From<Color> for AlphaPixel {
    fn from(color: Color) -> Self {
        AlphaPixel {
            alpha: color.transparency(),
            red: color.red(),
            green: color.green(),
            blue: color.blue(),
        }
    }
}
