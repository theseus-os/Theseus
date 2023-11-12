#![no_std]

mod circle;
mod coordinates;
mod line;
mod rectangle;

pub use circle::Circle;
pub use coordinates::Coordinates;
pub use line::Line;
pub use rectangle::Rectangle;

pub enum Vertical {
    Top,
    Bottom,
}

pub enum Horizontal {
    Left,
    Right,
}

/// A shape that can be contained by another shape.
///
/// Since the shape must be a convex polygon, it is enough to check that all the
/// vertices of the shape are contained.
pub trait Containable: ConvexPolygon {
    type I: Iterator<Item = Coordinates>;

    /// The vertices of the shape.
    fn vertices(&self) -> Self::I;
}

/// A convex polygon.
pub unsafe trait ConvexPolygon {}
