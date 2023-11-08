use zerocopy::FromBytes;

pub trait Pixel: FromBytes + Copy {}

#[derive(Clone, Copy, FromBytes)]
pub struct AlphaPixel {}

impl Pixel for AlphaPixel {}
