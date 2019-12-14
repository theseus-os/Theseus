use super::*;

pub type PixelColor = u32;
#[derive(Copy, Clone)]
pub struct RGBPixel(PixelColor);
#[derive(Copy, Clone)]
pub struct AlphaPixel(PixelColor);
/// Every pixel is of `Pixel` type, which is 4 byte as defined in `Pixel`
pub const PIXEL_SIZE: usize = 4;//core::mem::size_of::<Pixel>();

/// Used in reseting the alpha channel of RGB pixel.
const RGB_PIXEL_MASK: u32 = 0x00FFFFFF;
/// predefined opaque black
pub const BLACK: u32 = 0;
/// predefined opaque white
pub const WHITE: u32 = 0x00FFFFFF;

/// A pixel provides methods to mix two pixels
pub trait Pixel {
    fn from(pixel: PixelColor) -> Self;

    fn composite_buffer(src: &[PixelColor], dest: &mut[PixelColor]);
    
    fn color(&self) -> PixelColor;

    /// mix two color using alpha channel composition, supposing `self` is on the top of `other` pixel.
    fn alpha_mix(self, other: Self) -> Self;

    /// mix two color linearly with weights, as `mix` for `self` and (1-`mix`) for `other`. It returns black if mix is outside range of [0, 1].
    fn color_mix(self, other: Self, mix: f32) -> Self;

    /// Gets the alpha channel of the pixel
    fn get_alpha(&self) -> u8;

    /// Gets the red byte of the pixel
    fn get_red(&self) -> u8;

    /// Gets the green byte of the pixel
    fn get_green(&self) -> u8;

    /// Gets the blue byte of the pixel
    fn get_blue(&self) -> u8;
}

impl Pixel for RGBPixel {
    #[inline]
    fn from(pixel: PixelColor) -> RGBPixel {
        RGBPixel(pixel)
    }

    #[inline]
    fn color(&self) -> PixelColor {
        self.0
    }

    #[inline]
    fn composite_buffer(src: &[PixelColor], dest: &mut[PixelColor]) {
        dest.copy_from_slice(src)
    }
    
    #[inline]
    fn alpha_mix(self, other: Self) -> Self {
        self
    }

    fn color_mix(self, other: Self, mix: f32) -> Self {
        if mix < 0f32 || mix > 1f32 {
            return RGBPixel(BLACK);
        }
        let new_alpha =
            ((self.get_alpha() as f32) * mix + (other.get_alpha() as f32) * (1f32 - mix)) as u8;
        let new_red =
            ((self.get_red() as f32) * mix + (other.get_red() as f32) * (1f32 - mix)) as u8;
        let new_green =
            ((self.get_green() as f32) * mix + (other.get_green() as f32) * (1f32 - mix)) as u8;
        let new_blue =
            ((self.get_blue() as f32) * mix + (other.get_blue() as f32) * (1f32 - mix)) as u8;
        return RGBPixel(new_alpha_pixel(new_alpha, new_red, new_green, new_blue).0);
    }

    fn get_alpha(&self) -> u8 {
        (self.0 >> 24) as u8
    }

    fn get_red(&self) -> u8 {
        (self.0 >> 16) as u8
    }

    fn get_green(&self) -> u8 {
        (self.0 >> 8) as u8
    }

    fn get_blue(&self) -> u8 {
        self.0 as u8
    }
}

/// Create a new Pixel from `alpha`, `red`, `green` and `blue` bytes.
pub fn new_alpha_pixel(alpha: u8, red: u8, green: u8, blue: u8) -> AlphaPixel {
    AlphaPixel(
        ((alpha as u32) << 24) + ((red as u32) << 16) + ((green as u32) << 8) + (blue as u32)
    )
}

// Wenqiu: TODO draw pixel for alpha framebuffer
impl Pixel for AlphaPixel {
    #[inline]
    fn from(pixel: PixelColor) -> AlphaPixel {
        AlphaPixel(pixel)
    }

    fn color(&self) -> PixelColor {
        self.0
    }
    
    fn composite_buffer(src: &[PixelColor], dest: &mut[PixelColor]) {
        for i in 0..src.len() {
            dest[i] = AlphaPixel(src[i]).alpha_mix(AlphaPixel(dest[i])).color();
        }
    }

    fn alpha_mix(self, other: Self) -> Self {
        let alpha = self.get_alpha() as u16;
        let red = self.get_red();
        let green = self.get_green();
        let blue = self.get_blue();
        // let ori_alpha = other.get_alpha();
        let ori_red = other.get_red();
        let ori_green = other.get_green();
        let ori_blue = other.get_blue();
        // let new_alpha = (((alpha as u16) * (255 - alpha) + (ori_alpha as u16) * alpha) / 255) as u8;
        let new_red = (((red as u16) * (255 - alpha) + (ori_red as u16) * alpha) / 255) as u8;
        let new_green = (((green as u16) * (255 - alpha) + (ori_green as u16) * alpha) / 255) as u8;
        let new_blue = (((blue as u16) * (255 - alpha) + (ori_blue as u16) * alpha) / 255) as u8;
        return new_alpha_pixel(alpha as u8, new_red, new_green, new_blue);
    }

    fn color_mix(self, other: Self, mix: f32) -> Self {
        if mix < 0f32 || mix > 1f32 {
            return AlphaPixel(BLACK);
        }
        let new_alpha =
            ((self.get_alpha() as f32) * mix + (other.get_alpha() as f32) * (1f32 - mix)) as u8;
        let new_red =
            ((self.get_red() as f32) * mix + (other.get_red() as f32) * (1f32 - mix)) as u8;
        let new_green =
            ((self.get_green() as f32) * mix + (other.get_green() as f32) * (1f32 - mix)) as u8;
        let new_blue =
            ((self.get_blue() as f32) * mix + (other.get_blue() as f32) * (1f32 - mix)) as u8;
        return new_alpha_pixel(new_alpha, new_red, new_green, new_blue);
    }

    fn get_alpha(&self) -> u8 {
        (self.0 >> 24) as u8
    }

    fn get_red(&self) -> u8 {
        (self.0 >> 16) as u8
    }

    fn get_green(&self) -> u8 {
        (self.0 >> 8) as u8
    }

    fn get_blue(&self) -> u8 {
        self.0 as u8
    }
}
