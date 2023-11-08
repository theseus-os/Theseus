#[derive(Clone, Copy, PartialEq, Debug, Hash)]
pub struct Rectangle {
    pub coordinates: Coordinates,
    pub width: usize,
    pub height: usize,
}

#[derive(Clone, Copy, PartialEq, Debug, Hash)]
pub struct Coordinates {
    pub x: usize,
    pub y: usize,
}
