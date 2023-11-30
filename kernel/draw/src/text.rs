// TODO: Move `font` crate to libs

use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH};
use geometry::{Coordinates, Rectangle};
use graphics::{Framebuffer, Pixel};

use crate::{Char, Drawable, Settings};

pub struct Text<T>
where
    T: AsRef<str>,
{
    pub inner: T,
    pub coordinates: Coordinates,
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
    pub fn new(inner: T, coordinates: Coordinates) -> Self {
        Self { inner, coordinates }
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
    fn draw<P>(&self, framebuffer: &mut Framebuffer<P>, settings: &Settings<P>) -> Rectangle
    where
        P: Pixel,
    {
        // IDEA: Some potential extensions: https://en.wikipedia.org/wiki/Font_rasterization

        let grid_width = Self::grid_width(framebuffer);
        let grid_height = Self::grid_height(framebuffer);

        let s = self.inner.as_ref();
        // FIXME
        assert!(s.is_ascii());

        let mut row = 0;
        let mut column = 0;

        let mut bounding_box = Rectangle::new(self.coordinates, 0, 0);

        for c in s.chars() {
            if c == '\n' {
                column = 0;
                row += 1;
            } else {
                let coordinates = self.coordinates
                    + Coordinates::new(column * CHARACTER_WIDTH, row * CHARACTER_HEIGHT);
                let char_bounding_area = Char {
                    coordinates,
                    inner: c,
                }
                .draw(framebuffer, settings);

                bounding_box = bounding_box.merge(&char_bounding_area);

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

        bounding_box
    }
}
