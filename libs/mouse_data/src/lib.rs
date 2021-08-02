#![allow(dead_code)]
#![no_std]




#[derive(Debug, Copy, Clone)]
pub struct Displacement {
    pub x: u8,
    pub y: u8,
}

impl Displacement {
    pub const fn default() -> Displacement {
        Displacement { x: 0, y: 0 }
    }
    pub fn read_from_data(&mut self, readdata: u32) {
        let x_dis: u8 = ((readdata & 0x0000ff00) >> 8) as u8;
        let y_dis: u8 = ((readdata & 0x00ff0000) >> 16) as u8;
        self.x = x_dis;
        self.y = y_dis;
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
    pub const fn default() -> ButtonAction {
        ButtonAction {
            left_button_hold: false,
            right_button_hold: false,
            fourth_button_hold: false,
            fifth_button_hold: false,
        }
    }

    pub fn read_from_data(&mut self, readdata: u32) {
        if readdata & 0x01 == 0x01 {
            self.left_button_hold = true;
        } else {
            self.left_button_hold = false;
        }

        if readdata & 0x02 == 0x02 {
            self.right_button_hold = true;
        } else {
            self.right_button_hold = false;
        }

        if readdata & 0x10000000 == 0x10000000 {
            self.fourth_button_hold = true;
        } else {
            self.fourth_button_hold = false;
        }

        if readdata & 0x20000000 == 0x20000000 {
            self.fifth_button_hold = true;
        } else {
            self.fifth_button_hold = false;
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
    pub const fn default() -> MouseMovement {
        MouseMovement {
            right: false,
            left: false,
            down: false,
            up: false,
            scrolling_up: false,
            scrolling_down: false,
        }
    }

    pub fn read_from_data(&mut self, readdata: u32) {
        let first_byte = (readdata & 0xff) as u8;
        let second_byte = ((readdata & 0xff00) >> 8) as u8;
        let third_byte = ((readdata & 0xff0000) >> 16) as u8;
        let fourth_byte = ((readdata & 0xff000000) >> 24) as u8;

        if first_byte & 0x80 == 0x80 || first_byte & 0x40 == 0x40 {

        } else {
            if third_byte == 0 {
                self.down = false;
                self.up = false;
            } else {
                if first_byte & 0x20 == 0x20 {
                    self.down = true;
                    self.up = false;
                } else {
                    self.up = true;
                    self.down = false;
                }
            }

            if second_byte == 0 {
                self.left = false;
                self.right = false;
            } else {
                if first_byte & 0x10 == 0x10 {
                    self.left = true;
                    self.right = false;
                } else {
                    self.right = true;
                    self.left = false;
                }
            }
            if fourth_byte == 0 {
                self.scrolling_up = false;
                self.scrolling_down = false;
            } else {
                if fourth_byte & 0x0F == 0x0F {
                    self.scrolling_down = true;
                } else if fourth_byte & 0x0F != 0x0F {
                    self.scrolling_down = false;
                    if fourth_byte & 0x01 == 0x01 {
                        self.scrolling_up = true;
                    } else if fourth_byte & 0x01 == 0x0 {
                        self.scrolling_up = false;
                    }
                }

            }
        }
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
