#![no_std]
use core::ops::{Add, Sub};

pub static SCREEN_WIDTH: usize = 1024;
pub static SCREEN_HEIGHT: usize = 768;
/// Position that is always within the screen coordinates
#[derive(Debug, Clone, Copy)]
pub struct RelativePos {
    pub x: u32,
    pub y: u32,
}

impl RelativePos {
    pub fn new(x: u32, y: u32) -> Self {
        let x = core::cmp::min(x,SCREEN_WIDTH as u32);
        let y = core::cmp::min(y, SCREEN_HEIGHT as u32);
        Self { x, y }
    }

    pub fn to_1d_pos(&self, target_stride: u32) -> usize {
        ((target_stride * self.y) + self.x) as usize
    }
}

/// Screen position, absolute coordinates.
#[derive(Debug, Clone, Copy)]
pub struct ScreenPos {
    pub x: i32,
    pub y: i32,
}

impl ScreenPos {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub fn to_1d_pos(&self) -> usize {
        ((SCREEN_WIDTH as i32 * self.y) + self.x) as usize
    }
}

impl Add for ScreenPos {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

impl Sub for ScreenPos {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl Add<Rect> for ScreenPos {
    type Output = Self;

    fn add(self, other: Rect) -> Self {
        Self {
            x: self.x + other.x as i32,
            y: self.y + other.y as i32,
        }
    }
}

/// Ubiquitous structure representing a rectangle.
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub width: usize,
    pub height: usize,
    pub x: isize,
    pub y: isize,
}

impl Rect {
    pub fn new(width: usize, height: usize, x: isize, y: isize) -> Rect {
        Rect {
            width,
            height,
            x,
            y,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn to_screen_pos(&self) -> ScreenPos {
        ScreenPos {
            x: self.x as i32,
            y: self.y as i32,
        }
    }

    /// Return's `RelativePos` from `x` and `y` of itself,
    pub fn to_relative_pos(&self) -> RelativePos {
        let x = core::cmp::max(0, self.x) as u32;
        RelativePos { x, y: self.y as u32 }
    }

    pub fn set_position(&mut self, x: u32, y: u32) {
        self.x = x as isize;
        self.y = y as isize;
    }

    pub fn x_plus_width(&self) -> isize {
        self.x + self.width as isize
    }

    pub fn y_plus_height(&self) -> isize {
        self.y + self.height as isize
    }

    pub fn detect_collision(&self, other: &Rect) -> bool {
        self.x < other.x_plus_width()
            && self.x_plus_width() > other.x
            && self.y < other.y_plus_height()
            && self.y_plus_height() > other.y
    }

    /// Checks if left side of `Rectangle` is outside the screen or not.
    pub fn left_side_out(&self) -> bool {
        self.x < 0
    }

    /// Checks if right side of `Rectangle` is outside the screen or not.
    pub fn right_side_out(&self) -> bool {
        self.x + self.width as isize > SCREEN_WIDTH as isize
    }

    /// Checks if bottom side of `Rectangle` is outside the screen or not.
    pub fn bottom_side_out(&self) -> bool {
        self.y + self.height as isize > SCREEN_HEIGHT as isize
    }

    /// Creates a new `Rect` from visible parts of itself, by obtaining `SCREEN_WIDTH` and `SCREEN_HEIGHT` then
    /// compares with it's own dimensions.
    pub fn visible_rect(&self) -> Rect {
        let mut x = self.x;
        let y = self.y;
        let mut width = self.width as isize;
        let mut height = self.height as isize;
        if self.left_side_out() {
            x = 0;
            width = self.x_plus_width();
        } else if self.right_side_out() {
            x = self.x;
            let gap = (self.x + self.width as isize) - SCREEN_WIDTH as isize;
            width = self.width as isize - gap;
        }
        if self.bottom_side_out() {
            let gap = (self.y + self.height as isize) - SCREEN_HEIGHT as isize;
            height = self.height as isize - gap;
        }
        let visible_rect = Rect::new(width as usize, height as usize, x, y);
        visible_rect
    }
}
