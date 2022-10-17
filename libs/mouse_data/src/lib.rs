#![allow(dead_code)]
#![no_std]




#[derive(Debug, Copy, Clone)]
pub struct Displacement {
    pub x: u8,
    pub y: u8,
}

impl Displacement {
    pub fn read_from_data(readdata: u32) -> Self {
        Self {
            x: ((readdata & 0x0000ff00) >> 8) as u8,
            y: ((readdata & 0x00ff0000) >> 16) as u8
        }
    }
}
#[derive(Debug, Copy, Clone)]
pub struct ButtonAction {
    pub left_button_hold: bool,
    pub right_button_hold: bool,
    pub fourth_button_hold: bool,
    pub fifth_button_hold: bool,
}

impl ButtonAction {
    pub fn read_from_data(readdata: u32) -> Self {
        Self {
            left_button_hold: readdata & 0x01 == 0x01,
            right_button_hold: readdata & 0x02 == 0x02,
            fourth_button_hold: readdata & 0x10000000 == 0x10000000,
            fifth_button_hold: readdata & 0x20000000 == 0x20000000,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct MouseMovement {
    pub right: bool,
    pub left: bool,
    pub down: bool,
    pub up: bool,
    pub scrolling_up: bool,
    pub scrolling_down: bool,
}

impl MouseMovement {
    pub fn read_from_data(readdata: u32) -> Self {
        let first_byte = (readdata & 0xff) as u8;
        let second_byte = ((readdata & 0xff00) >> 8) as u8;
        let third_byte = ((readdata & 0xff0000) >> 16) as u8;
        let fourth_byte = ((readdata & 0xff000000) >> 24) as u8;
        let mut right = false;
        let mut left = false;
        let mut down = false;
        let mut up = false;
        let mut scrolling_up = false;
        let mut scrolling_down = false;

        if third_byte == 0 {
            down = false;
            up = false;
        } else {
            if first_byte & 0x20 == 0x20 {
                down = true;
                up = false;
            } else {
                up = true;
                down = false;
            }
        }

        if second_byte == 0 {
            left = false;
            right = false;
        } else {
            if first_byte & 0x10 == 0x10 {
                left = true;
                right = false;
            } else {
                right = true;
                left = false;
            }
        }
        if fourth_byte == 0 {
            scrolling_up = false;
            scrolling_down = false;
        } else {
            if fourth_byte & 0x0F == 0x0F {
                scrolling_down = true;
            } else if fourth_byte & 0x0F != 0x0F {
                scrolling_down = false;
                if fourth_byte & 0x01 == 0x01 {
                    scrolling_up = true;
                } else if fourth_byte & 0x01 == 0x0 {
                    scrolling_up = false;
                }
            }
        }
        return Self { right, left, down, up, scrolling_up, scrolling_down }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct MouseEvent {
    pub buttonact: ButtonAction,
    pub mousemove: MouseMovement,
    pub displacement: Displacement,
}

impl MouseEvent {
    pub fn new(
        buttonact: ButtonAction,
        mousemove: MouseMovement,
        displacement: Displacement,
    ) -> MouseEvent {
        MouseEvent {
            buttonact,
            mousemove,
            displacement,
        }
    }
}
