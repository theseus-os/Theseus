use core::hash::Hash;

/// The size of a pixel.
pub const PIXEL_SIZE: usize = core::mem::size_of::<u32>();


/// predefined opaque black
pub const BLACK: u32 = 0;
/// predefined opaque white
pub const WHITE: u32 = 0x00FFFFFF;

/// A pixel provides methods to mix with others.
pub trait Pixel: From<u32> + Copy + Hash {
    /// Composites the `src` pixel slice to the `dest` pixel slice.
    fn composite_buffer(src: &[Self], dest: &mut[Self]);
    
    /// mix with another pixel considering their extra channel.
    fn mix(self, other: Self) -> Self;
}

#[repr(C, packed)]
#[derive(Hash, Debug, Clone, Copy)]
/// An RGB Pixel is a pixel with no extra channel.
pub struct RGBPixel {
    pub blue: u8,
    pub green: u8,
    pub red: u8,
    pub channel: u8,
}

impl From<u32> for RGBPixel {
    fn from(color: u32) -> Self {
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
/// An Alpha Pixel is a pixel with an alpha channel
pub struct AlphaPixel {
    pub blue: u8,
    pub green: u8,
    pub red: u8,
    pub alpha: u8
}

impl From<u32> for AlphaPixel {
    fn from(color: u32) -> Self {
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
    fn mix(self, _other: Self) -> Self {
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
}

/// Mix two pixels linearly with weights, as `mix` for `origin` and (1-`mix`) for `other`. It returns black if mix is outside range of [0, 1].
pub fn weight_mix(origin: u32, other: u32, mix: f32) -> u32 {
    if mix < 0f32 || mix > 1f32 {
        return BLACK;
    }
    let new_channel =
        (((origin >> 24) as f32) * mix + ((other >> 24) as f32) * (1f32 - mix)) as u32;
    let new_red =
        ((((origin >> 16) as u8) as f32) * mix + (((other >> 16) as u8) as f32) * (1f32 - mix)) as u32;
    let new_green =
        ((((origin >> 8) as u8) as f32) * mix + (((other >> 8) as u8) as f32) * (1f32 - mix)) as u32;
    let new_blue =
        (((origin as u8) as f32) * mix + ((other as u8) as f32) * (1f32 - mix)) as u32;
    new_channel <<24 | new_red << 16 | new_green << 8 | new_blue
}