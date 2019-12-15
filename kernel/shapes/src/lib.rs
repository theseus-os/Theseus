//! This crate defines the `FrameBuffer` trait and maintains the final framebuffer.
//! A `FrameBuffer` contains fundamental display interfaces including displaying a single pixel and copying a buffer of pixels.

#![no_std]

use core::ops::{Add, Sub};
use core::cmp::{Ord, Ordering};

/// The coordinate of a pixel.
/// In the display subsystem, the origin of an area is its top-left point.
#[derive(Clone, Copy, PartialEq, Debug, Hash)]
pub struct Coord {
    /// The x coordinate
    pub x: isize,
    /// The y coordinate
    pub y: isize,
}

impl Coord {
    /// Creates a new coordinate.
    pub fn new(x: isize, y: isize) -> Coord {
        Coord { x: x, y: y }
    }
}

impl Add<(isize, isize)> for Coord {
    type Output = Coord;

    fn add(self, rhs: (isize, isize)) -> Coord {
        Coord { x: self.x + rhs.0, y: self.y + rhs.1 }
    }
}

impl Sub<(isize, isize)> for Coord {
    type Output = Coord;

    fn sub(self, rhs: (isize, isize)) -> Coord {
        Coord { x: self.x - rhs.0, y: self.y - rhs.1 }
    }
}

impl Add<Coord> for Coord {
    type Output = Coord;

    fn add(self, rhs: Coord) -> Coord {
        Coord {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl Sub<Coord> for Coord {
    type Output = Coord;

    fn sub(self, rhs: Coord) -> Coord {
        Coord {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl Ord for Coord {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.y > other.y {
            return Ordering::Greater;
        } else if self.y < other.y {
            return Ordering::Less;
        } else {
            return self.x.cmp(&other.x);
        }

    }
}

impl PartialOrd for Coord {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for Coord { }


/// a rectangle region
#[derive(Clone, Copy, PartialEq, Debug, Hash)]
pub struct Rectangle {
    /// The top-left point
    pub top_left: Coord,
    /// The bottom-right point
    pub bottom_right: Coord,
}

impl Add<Coord> for Rectangle {
    type Output = Rectangle;

    fn add(self, rhs: Coord) -> Rectangle {
        Rectangle {
            top_left: self.top_left + rhs,
            bottom_right: self.bottom_right + rhs,
        }
    }
}

impl Sub<Coord> for Rectangle {
    type Output = Rectangle;

    fn sub(self, rhs: Coord) -> Rectangle {
        Rectangle {
            top_left: self.top_left - rhs,
            bottom_right: self.bottom_right - rhs,
        }
    }
}