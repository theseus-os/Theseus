use crate::{Containable, Coordinates, Horizontal, Vertical};

#[derive(Clone, Copy, PartialEq, Debug, Hash)]
pub struct Rectangle {
    /// The top left vertex of the rectangle.
    pub coordinates: Coordinates,
    width: usize,
    height: usize,
}

impl Rectangle {
    pub const MAX: Self = Self::new(Coordinates::ZERO, usize::MAX, usize::MAX);

    pub const fn new(coordinates: Coordinates, width: usize, height: usize) -> Self {
        assert!(width > 0, "rectangle must have width greater than 0");
        assert!(height > 0, "rectangle must have height greater than 0");
        Self {
            coordinates,
            width,
            height,
        }
    }

    pub const fn width(&self) -> usize {
        self.width
    }

    pub const fn height(&self) -> usize {
        self.height
    }

    pub const fn x(&self, horizontal: Horizontal) -> usize {
        match horizontal {
            Horizontal::Left => self.coordinates.x,
            Horizontal::Right => self.coordinates.x + self.width - 1,
        }
    }

    pub const fn y(&self, vertical: Vertical) -> usize {
        match vertical {
            Vertical::Top => self.coordinates.y,
            Vertical::Bottom => self.coordinates.y + self.height - 1,
        }
    }

    pub const fn vertex(&self, vertical: Vertical, horizontal: Horizontal) -> Coordinates {
        Coordinates {
            x: self.x(horizontal),
            y: self.y(vertical),
        }
    }

    pub fn contains<T>(&self, containable: T) -> bool
    where
        T: Containable,
    {
        for coordinates in containable.coordinates() {
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

impl Containable for Rectangle {
    type I = core::array::IntoIter<Coordinates, 4>;

    fn coordinates(&self) -> Self::I {
        [
            self.vertex(Vertical::Top, Horizontal::Left),
            self.vertex(Vertical::Top, Horizontal::Right),
            self.vertex(Vertical::Bottom, Horizontal::Right),
            self.vertex(Vertical::Bottom, Horizontal::Left),
        ]
        .into_iter()
    }
}
