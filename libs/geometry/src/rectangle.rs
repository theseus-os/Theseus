use crate::{Containable, ConvexPolygon, Coordinates, Horizontal, Vertical};

/// A rectangle.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Rectangle {
    /// The top left vertex of the rectangle.
    pub coordinates: Coordinates,
    width: usize,
    height: usize,
}

impl Rectangle {
    pub const MAX: Self = Self::new(Coordinates::ZERO, usize::MAX, usize::MAX);

    /// Returns a rectangle with the top left vertex at `coordinates`.
    ///
    /// # Panics
    ///
    /// Panics if `width` is 0, or `height` is 0.
    pub const fn new(coordinates: Coordinates, width: usize, height: usize) -> Self {
        assert!(width > 0, "rectangle must have width greater than 0");
        assert!(height > 0, "rectangle must have height greater than 0");
        Self {
            coordinates,
            width,
            height,
        }
    }

    /// Returns the wdith of the rectangle.
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Returns the height of the rectangle.
    pub const fn height(&self) -> usize {
        self.height
    }

    /// Returns the furthest `x` coordinate in `direction`.
    pub const fn x(&self, direction: Horizontal) -> usize {
        match direction {
            Horizontal::Left => self.coordinates.x,
            Horizontal::Right => self.coordinates.x + self.width - 1,
        }
    }

    /// Returns the furthest `y` coordinate in `direction`.
    pub const fn y(&self, direction: Vertical) -> usize {
        match direction {
            Vertical::Top => self.coordinates.y,
            Vertical::Bottom => self.coordinates.y + self.height - 1,
        }
    }

    /// Returns the vertex specified by `vertical` and `horizontal`.
    pub const fn vertex(&self, vertical: Vertical, horizontal: Horizontal) -> Coordinates {
        Coordinates {
            x: self.x(horizontal),
            y: self.y(vertical),
        }
    }

    /// Returns whether the rectangle fully contains `T`.
    pub fn contains<T>(&self, containable: T) -> bool
    where
        T: Containable,
    {
        for coordinates in containable.vertices() {
            if coordinates.x < self.x(Horizontal::Left)
                || coordinates.x > self.x(Horizontal::Right)
                || coordinates.y < self.y(Vertical::Top)
                || coordinates.y > self.y(Vertical::Bottom)
            {
                return false;
            }
        }
        true
    }
}

unsafe impl ConvexPolygon for Rectangle {}

impl Containable for Rectangle {
    type I = core::array::IntoIter<Coordinates, 4>;

    fn vertices(&self) -> Self::I {
        [
            self.vertex(Vertical::Top, Horizontal::Left),
            self.vertex(Vertical::Top, Horizontal::Right),
            self.vertex(Vertical::Bottom, Horizontal::Right),
            self.vertex(Vertical::Bottom, Horizontal::Left),
        ]
        .into_iter()
    }
}
