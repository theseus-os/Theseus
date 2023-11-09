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

pub trait Containable {
    type I: Iterator<Item = Coordinates>;

    fn coordinates(&self) -> Self::I;
}
