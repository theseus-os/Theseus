use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH};
use geometry::{Coordinates, Rectangle};
use graphics::{Framebuffer, Pixel};

use crate::{Drawable, Settings};

pub struct Char {
    pub inner: char,
    pub coordinates: Coordinates,
}

impl Char {
    pub fn new(inner: char, coordinates: Coordinates) -> Self {
        Self {
            inner,
            coordinates,
        }
    }
}

impl Drawable for Char {
    fn draw<P>(&self, framebuffer: &mut Framebuffer<P>, settings: &Settings<P>) -> Rectangle
    where
        P: Pixel,
    {
        fn is_set(character: char, coordinates: Coordinates) -> bool {
            font::FONT_BASIC[character as usize][coordinates.y] & (0x80 >> coordinates.x) != 0
        }

        // TODO: Optimise

        for row in 0..CHARACTER_HEIGHT {
            for col in 0..CHARACTER_WIDTH {
                // The coordinates relative to the top left of the character.
                let relative = Coordinates::new(col, row);
                if col == 0 {
                    if let Some(background) = settings.background {
                        framebuffer.set(self.coordinates + relative, background)
                    }
                } else {
                    let offset_coordinates = Coordinates::new(col - 1, row);
                    if is_set(self.inner, offset_coordinates) {
                        framebuffer.set(self.coordinates + relative, settings.foreground)
                    } else if let Some(background) = settings.background {
                        framebuffer.set(self.coordinates + relative, background)
                    }
                }
            }
        }

        Rectangle::new(self.coordinates, CHARACTER_WIDTH, CHARACTER_HEIGHT)
    }
}
