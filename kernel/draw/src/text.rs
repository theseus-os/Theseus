// TODO: Move `font` crate to libs

use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH};
use geometry::Coordinates;
use graphics::{Framebuffer, Pixel};

use crate::{Drawable, Settings};

pub struct Text<T>
where
    T: AsRef<str>,
{
    pub coordinates: Coordinates,
    pub inner: T,
}

impl<T> Text<T>
where
    T: AsRef<str>,
{
    pub const fn grid_width<P>(framebuffer: &Framebuffer<P>) -> usize
    where
        P: Pixel,
    {
        framebuffer.width() / CHARACTER_WIDTH
    }

    pub const fn grid_height<P>(framebuffer: &Framebuffer<P>) -> usize
    where
        P: Pixel,
    {
        framebuffer.height() / CHARACTER_HEIGHT
    }
}

impl<T> Text<T>
where
    T: AsRef<str>,
{
    pub fn new(coordinates: Coordinates, inner: T) -> Self {
        Self { coordinates, inner }
    }

    pub fn next_grid_position<P>(&self, framebuffer: &Framebuffer<P>) -> (usize, usize)
    where
        P: Pixel,
    {
        let grid_width = Self::grid_width(framebuffer);
        let grid_height = Self::grid_height(framebuffer);

        let mut column = 0;
        let mut row = 0;

        for c in self.inner.as_ref().chars() {
            if c == '\n' {
                column = 0;
                row += 1;
            } else {
                column += 1;

                if column == grid_width {
                    column = 0;
                    row += 1;
                }
            }

            if row == grid_height {
                break;
            }
        }

        (column, row)
    }
}

impl<T> Drawable for Text<T>
where
    T: AsRef<str>,
{
    fn draw<P>(&self, framebuffer: &mut Framebuffer<P>, settings: &Settings<P>)
    where
        P: Pixel,
    {
        // IDEA: Some potential extensions: https://en.wikipedia.org/wiki/Font_rasterization

        fn draw_character<P>(
            framebuffer: &mut Framebuffer<P>,
            character: char,
            coordinates: Coordinates,
            settings: &Settings<P>,
        ) where
            P: Pixel,
        {
            fn is_set(character: char, coordinates: Coordinates) -> bool {
                font::FONT_BASIC[character as usize][coordinates.y] & (0x80 >> coordinates.x) != 0
            }

            // TODO: Optimise

            for row in 0..CHARACTER_HEIGHT {
                for col in 0..CHARACTER_HEIGHT {
                    // The coordinates relative to the top left of the charter.
                    let relative = Coordinates::new(col, row);

                    if is_set(character, relative) {
                        framebuffer.set(coordinates + relative, settings.foreground);
                    } else if let Some(background) = settings.background {
                        framebuffer.set(coordinates + relative, background);
                    }
                }
            }
        }

        let grid_width = Self::grid_width(framebuffer);
        let grid_height = Self::grid_height(framebuffer);

        let s = self.inner.as_ref();
        // FIXME
        assert!(s.is_ascii());

        let mut row = 0;
        let mut column = 0;

        for c in s.chars() {
            if c == '\n' {
                column = 0;
                row += 1;
            } else {
                let coordinates = self.coordinates
                    + Coordinates::new(column * CHARACTER_WIDTH, row * CHARACTER_HEIGHT);
                draw_character(framebuffer, c, coordinates, settings);

                column += 1;

                if column == grid_width {
                    column = 0;
                    row += 1;
                }
            }

            if row == grid_height {
                break;
            }
        }
    }
}
