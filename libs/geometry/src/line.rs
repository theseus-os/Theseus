use core::cmp::{max, min};

use crate::{Coordinates, Horizontal, Vertical};

#[derive(Clone, Copy, PartialEq, Debug, Hash)]
pub struct Line {
    pub start: Coordinates,
    pub end: Coordinates,
}

impl Line {
    pub fn x(&self, horizontal: Horizontal) -> usize {
        let f = match horizontal {
            Horizontal::Left => min,
            Horizontal::Right => max,
        };
        f(self.start.x, self.end.x)
    }

    pub fn y(&self, vertical: Vertical) -> usize {
        let f = match vertical {
            Vertical::Top => min,
            Vertical::Bottom => max,
        };
        f(self.start.y, self.end.y)
    }
}
