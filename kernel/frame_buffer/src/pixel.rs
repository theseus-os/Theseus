use core::hash::Hash;

/// The size of a pixel.
pub const PIXEL_SIZE: usize = core::mem::size_of::<u32>();


/// predefined black
pub const BLACK: u32 = 0;
/// predefined white
pub const WHITE: u32 = 0x00FFFFFF;

/// User uses this type to provide pixel value to the framebuffer.
#[derive(Clone, Copy)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl From<u32> for Color {
    fn from(color: u32) -> Color {
        Color {
            red: (color >> 16) as u8,
            green: (color >> 8) as u8,
            blue: color as u8,
        }
    }
}

/// This structure has a color and a transparency value. It can turn into an alpha pixel or a pixel for framebuffers that does not support the alpha channel.
#[derive(Clone, Copy)]
pub struct AlphaColor {
    /// 0 is non-transparent while 0xFF is totally transparent
    pub transparency: u8,
    pub color: Color
}

impl From<u32> for AlphaColor {
    fn from(acolor: u32) -> AlphaColor {
        AlphaColor {
            transparency: (acolor >> 24) as u8,
            color: acolor.into(),
        }
    }
}

/// 0 represents non-transparent and 0xFF represents totally transparent.
pub type Transparency = u8;
/// A pixel provides methods to mix with others.
pub trait Pixel: From<AlphaColor> + Copy + Hash {
    /// Composites the `src` pixel slice to the `dest` pixel slice.
    fn composite_buffer(src: &[Self], dest: &mut[Self]);
    
    /// mix with another pixel considering their extra channel.
    fn mix(self, other: Self) -> Self;

    /// Mix two pixels linearly with weights, as `mix` for `origin` and (1-`mix`) for `other`. It returns black if mix is outside range of [0, 1].
    fn weight_mix(origin: Self, other: Self, mix: f32) -> Self;
}

#[repr(C, packed)]
#[derive(Hash, Debug, Clone, Copy)]
/// An RGB Pixel is a pixel with no extra channel.
pub struct RGBPixel {
    pub blue: u8,
    pub green: u8,
    pub red: u8,
    pub _channel: u8,
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

impl From<AlphaColor> for RGBPixel {
    fn from(acolor: AlphaColor) -> Self {
        acolor.color.into()
    }
}

#[repr(C, packed)]
#[derive(Hash, Debug, Clone, Copy)]
/// An Alpha Pixel is a pixel with an alpha channel
pub struct AlphaPixel {
    pub blue: u8,
    pub green: u8,
    pub red: u8,
    pub alpha: u8
}

impl From<AlphaColor> for AlphaPixel {
    fn from(acolor: AlphaColor) -> Self {
        AlphaPixel {
            alpha: acolor.transparency,
            red: acolor.color.red,
            green: acolor.color.green,
            blue: acolor.color.blue
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
    fn mix(self, _other: Self) -> Self {
        self
    }

    fn weight_mix(origin: Self, other: Self, mix: f32) -> Self {
        let mix = if mix < 0f32 {
            0f32
        } else if mix > 1f32 {
            1f32
        } else {
            mix
        };

        let new_red =
            ((origin.red as f32) * mix + (other.red as f32) * (1f32 - mix)) as u8;
        let new_green =
            ((origin.green as f32) * mix + (other.green as f32) * (1f32 - mix)) as u8;
        let new_blue =
            ((origin.blue as f32) * mix + (other.blue as f32) * (1f32 - mix)) as u8;
        
        RGBPixel{
            _channel: 0,
            red: new_red,
            green: new_green,
            blue: new_blue
        }
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

    fn weight_mix(origin: Self, other: Self, mix: f32) -> Self {
        let mix = if mix < 0f32 {
            0f32
        } else if mix > 1f32 {
            1f32
        } else {
            mix
        };

        let new_channel =
            ((origin.alpha as f32) * mix + (other.alpha as f32) * (1f32 - mix)) as u8;
        let new_red =
            ((origin.red as f32) * mix + (other.red as f32) * (1f32 - mix)) as u8;
        let new_green =
            ((origin.green as f32) * mix + (other.green as f32) * (1f32 - mix)) as u8;
        let new_blue =
            ((origin.blue as f32) * mix + (other.blue as f32) * (1f32 - mix)) as u8;
        AlphaPixel {
            alpha: new_channel,
            red: new_red,
            green: new_green,
            blue: new_blue
        }
    }
}