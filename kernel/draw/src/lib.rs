#![no_std]

mod character;
mod text;

pub use geometry::{Circle, Coordinates, Line, Rectangle};
use geometry::{Horizontal, Vertical};
use graphics::{Framebuffer, Pixel};

pub use crate::{character::Char, text::Text};

pub struct Settings<P>
where
    P: Pixel,
{
    pub foreground: P,
    pub background: Option<P>,
}

pub trait Drawable {
    fn draw<P>(&self, framebuffer: &mut Framebuffer<P>, settings: &Settings<P>) -> Rectangle
    where
        P: Pixel;
}

impl Drawable for Circle {
    #[inline]
    fn draw<P>(&self, _framebuffer: &mut Framebuffer<P>, _settings: &Settings<P>) -> Rectangle
    where
        P: Pixel,
    {
        todo!();
    }
}

impl Drawable for Line {
    #[inline]
    fn draw<P>(&self, framebuffer: &mut Framebuffer<P>, settings: &Settings<P>) -> Rectangle
    where
        P: Pixel,
    {
        // TODO: Antialiasing

        let refresh_area = Rectangle::new(
            Coordinates {
                x: self.x(Horizontal::Left),
                y: self.y(Vertical::Top),
            },
            self.x(Horizontal::Right) - self.x(Horizontal::Left) + 1,
            self.y(Vertical::Bottom) - self.y(Vertical::Top) + 1,
        );

        if self.start.y == self.end.y {
            // Horizontal line optimisation

            let left = self.x(Horizontal::Left);
            let right = self.x(Horizontal::Right);

            // TODO: Unwrap?
            framebuffer.rows_mut().nth(self.start.y).unwrap()[left..=right]
                .fill(settings.foreground);
        } else if self.start.x == self.end.x {
            // Vertical line optimisation

            let top = self.y(Vertical::Top);
            let bottom = self.y(Vertical::Bottom);
            let num_rows = bottom - top + 1;

            for row in framebuffer.rows_mut().skip(top).take(num_rows) {
                row[self.start.x] = settings.foreground;
            }
        } else {
            // Taken from: https://en.wikipedia.org/wiki/Bresenham%27s_line_algorithm
            // License: https://en.wikipedia.org/wiki/Wikipedia:Text_of_the_Creative_Commons_Attribution-ShareAlike_4.0_International_License

            #[inline]
            fn draw_line_low<P>(
                framebuffer: &mut Framebuffer<P>,
                c0: Coordinates,
                c1: Coordinates,
                settings: &Settings<P>,
            ) where
                P: Pixel,
            {
                let dx = c1.x as isize - c0.x as isize;
                let mut dy = c1.y as isize - c0.y as isize;
                let yi: isize = if dy < 0 {
                    dy = -dy;
                    -1
                } else {
                    1
                };
                let mut d = 2 * dy - dx;
                let mut y = c0.y;

                for x in c0.x..=c1.x {
                    framebuffer.set(Coordinates { x, y }, settings.foreground);
                    if d > 0 {
                        y = ((y as isize) + yi) as usize;
                        d += 2 * (dy - dx);
                    } else {
                        d += 2 * dy;
                    }
                }
            }

            #[inline]
            fn draw_line_high<P>(
                framebuffer: &mut Framebuffer<P>,
                c0: Coordinates,
                c1: Coordinates,
                settings: &Settings<P>,
            ) where
                P: Pixel,
            {
                let mut dx = c1.x as isize - c0.x as isize;
                let dy = c1.y as isize - c0.y as isize;
                let xi: isize = if dx < 0 {
                    dx = -dx;
                    -1
                } else {
                    1
                };
                let mut d = 2 * dx - dy;
                let mut x = c0.x;

                for y in c0.y..=c1.y {
                    framebuffer.set(Coordinates { x, y }, settings.foreground);
                    if d > 0 {
                        x = ((x as isize) + xi) as usize;
                        d += 2 * (dx - dy);
                    } else {
                        d += 2 * dx;
                    }
                }
            }

            let diff = self.end.abs_diff(self.start);
            #[allow(clippy::collapsible_else_if)]
            if diff.y < diff.x {
                if self.start.x > self.end.x {
                    draw_line_low(framebuffer, self.end, self.start, settings);
                } else {
                    draw_line_low(framebuffer, self.start, self.end, settings);
                }
            } else {
                if self.start.y > self.end.y {
                    draw_line_high(framebuffer, self.end, self.start, settings);
                } else {
                    draw_line_high(framebuffer, self.start, self.end, settings);
                }
            }
        }

        refresh_area
    }
}

impl Drawable for Rectangle {
    #[inline]
    fn draw<P>(&self, framebuffer: &mut Framebuffer<P>, settings: &Settings<P>) -> Rectangle
    where
        P: Pixel,
    {
        let top = self.y(Vertical::Top);
        let bottom = self.y(Vertical::Bottom);

        let left = self.x(Horizontal::Left);
        let right = self.x(Horizontal::Right);

        let num_rows = bottom - top + 1;

        for row in framebuffer.rows_mut().skip(top).take(num_rows) {
            row[left..right].fill(settings.foreground);
        }

        *self
    }
}
