use core::ops::{Add, AddAssign, Sub, SubAssign};

use crate::{Containable, ConvexPolygon};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Coordinates {
    pub x: usize,
    pub y: usize,
}

impl Coordinates {
    pub const ZERO: Self = Self { x: 0, y: 0 };

    pub const MAX: Self = Self {
        x: usize::MAX,
        y: usize::MAX,
    };

    pub fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }

    pub fn abs_diff(self, other: Coordinates) -> Self {
        Self {
            x: self.x.abs_diff(other.x),
            y: self.y.abs_diff(other.y),
        }
    }
}

impl Add<Coordinates> for Coordinates {
    type Output = Coordinates;

    fn add(self, rhs: Coordinates) -> Self::Output {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl AddAssign<Coordinates> for Coordinates {
    fn add_assign(&mut self, rhs: Coordinates) {
        *self = *self + rhs;
    }
}

impl Sub<Coordinates> for Coordinates {
    type Output = Coordinates;

    fn sub(self, rhs: Coordinates) -> Self::Output {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl SubAssign<Coordinates> for Coordinates {
    fn sub_assign(&mut self, rhs: Coordinates) {
        *self = *self - rhs;
    }
}

unsafe impl ConvexPolygon for Coordinates {}

impl Containable for Coordinates {
    type I = core::iter::Once<Coordinates>;

    fn vertices(&self) -> Self::I {
        core::iter::once(*self)
    }
}
