#![no_std]

//NOTE: could be reduced to 9+9+4=22-bit, +2-bit padding = 3*8-bit
#[derive(Debug, Clone)]
pub struct MouseMovementRelative {
    pub x_movement: i16,
    pub y_movement: i16,
    pub scroll_movement: i8,
}

impl MouseMovementRelative {
    pub fn new(x_movement: i16, y_movement: i16, scroll_movement: i8) -> Self {
        Self {
            x_movement,
            y_movement,
            scroll_movement,
        }
    }
}

//NOTE: could be reduced to 8-bit
#[derive(Debug, Clone)]
pub struct ButtonAction {
    pub left_button_hold: bool,
    pub right_button_hold: bool,
    pub middle_button_hold: bool,
    pub fourth_button_hold: bool,
    pub fifth_button_hold: bool,
}

impl ButtonAction {
    pub fn new(
        left_button_hold: bool,
        right_button_hold: bool,
        middle_button_hold: bool,
        fourth_button_hold: bool,
        fifth_button_hold: bool,
    ) -> Self {
        Self {
            left_button_hold,
            right_button_hold,
            middle_button_hold,
            fourth_button_hold,
            fifth_button_hold,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MouseEvent {
    pub buttonact: ButtonAction,
    pub mousemove: MouseMovementRelative,
}

impl MouseEvent {
    pub fn new(buttonact: ButtonAction, mousemove: MouseMovementRelative) -> MouseEvent {
        MouseEvent {
            buttonact,
            mousemove,
        }
    }
}