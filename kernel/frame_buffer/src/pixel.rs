use super::*;

pub type PixelColor = u32;
/// Every pixel is of `Pixel` type, which is 4 byte as defined in `Pixel`
pub const PIXEL_SIZE: usize = 4;//core::mem::size_of::<Pixel>();


/// Used in reseting the alpha channel of RGB pixel.
const RGB_PIXEL_MASK: u32 = 0x00FFFFFF;
/// predefined opaque black
pub const BLACK: u32 = 0;
/// predefined opaque white
pub const WHITE: u32 = 0x00FFFFFF;

/// A pixel provides methods to mix two pixels
pub trait Pixel: Sized + From<PixelColor> + Copy + Hash {
    fn composite_buffer(src: &[Self], dest: &mut[Self]);
    
    // fn color(&self) -> PixelColor;

    /// mix two color using alpha channel composition, supposing `self` is on the top of `other` pixel.
    fn mix(self, other: Self) -> Self;

    /// mix two color linearly with weights, as `mix` for `self` and (1-`mix`) for `other`. It returns black if mix is outside range of [0, 1].
    fn weight_mix(self, other: Self, mix: f32) -> Self;

    // /// Gets the alpha channel of the pixel
    // fn get_alpha(&self) -> u8;

    // /// Gets the red byte of the pixel
    // fn get_red(&self) -> u8;

    // /// Gets the green byte of the pixel
    // fn get_green(&self) -> u8;

    // /// Gets the blue byte of the pixel
    // fn get_blue(&self) -> u8;
}

#[repr(C, packed)]
#[derive(Hash, Debug, Clone, Copy)]
pub struct RGBPixel {
    pub blue: u8,
    pub green: u8,
    pub red: u8,
    pub channel: u8,
}

impl From<PixelColor> for RGBPixel {
    fn from(color: PixelColor) -> Self {
        RGBPixel {
            channel: 0,
            red: (color >> 16) as u8,
            green: (color >> 8) as u8,
            blue: color as u8
        }
    }
}

#[repr(C, packed)]
#[derive(Hash, Debug, Clone, Copy)]
pub struct AlphaPixel {
    pub blue: u8,
    pub green: u8,
    pub red: u8,
    pub alpha: u8
}

impl From<PixelColor> for AlphaPixel {
    fn from(color: PixelColor) -> Self {
        AlphaPixel {
            alpha: (color >> 24) as u8,
            red: (color >> 16) as u8,
            green: (color >> 8) as u8,
            blue: color as u8
        }
    }
}

impl Pixel for RGBPixel {
    // #[inline]
    // fn color(&self) -> PixelColor {
    //     self.0
    // }

    #[inline]
    fn composite_buffer(src: &[Self], dest: &mut[Self]) {
        dest.copy_from_slice(src)
    }
    
    #[inline]
    fn mix(self, other: Self) -> Self {
        self
    }

    #[inline]
    fn weight_mix(self, other: Self, mix: f32) -> Self {
        self
    }

}

impl Pixel for AlphaPixel {   
    fn composite_buffer(src: &[Self], dest: &mut[Self]) {
        for i in 0..src.len() {
            dest[i] = AlphaPixel::from(src[i]).mix(AlphaPixel::from(dest[i])).into();
        }
    }

    fn mix(self, other: Self) -> Self {
        let alpha = self.alpha as u16;
        let red = self.red;
        let green = self.green;
        let blue = self.blue;
        // let ori_alpha = other.alpha;
        let ori_red = other.red;
        let ori_green = other.green;
        let ori_blue = other.blue;
        // let new_alpha = (((alpha as u16) * (255 - alpha) + (ori_alpha as u16) * alpha) / 255) as u8;
        let new_red = (((red as u16) * (255 - alpha) + (ori_red as u16) * alpha) / 255) as u8;
        let new_green = (((green as u16) * (255 - alpha) + (ori_green as u16) * alpha) / 255) as u8;
        let new_blue = (((blue as u16) * (255 - alpha) + (ori_blue as u16) * alpha) / 255) as u8;
        AlphaPixel {
            alpha: alpha as u8,
            red: new_red,
            green: new_green,
            blue: new_blue
        }
    }

    fn weight_mix(self, other: Self, mix: f32) -> Self {
        if mix < 0f32 || mix > 1f32 {
            return AlphaPixel::from(BLACK);
        }
        let new_alpha =
            ((self.alpha as f32) * mix + (other.alpha as f32) * (1f32 - mix)) as u8;
        let new_red =
            ((self.red as f32) * mix + (other.red as f32) * (1f32 - mix)) as u8;
        let new_green =
            ((self.green as f32) * mix + (other.green as f32) * (1f32 - mix)) as u8;
        let new_blue =
            ((self.blue as f32) * mix + (other.blue as f32) * (1f32 - mix)) as u8;
        AlphaPixel {
            alpha: new_alpha, 
            red: new_red, 
            green: new_green, 
            blue: new_blue
        }
    }

}
