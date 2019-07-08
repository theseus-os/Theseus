//! Window Components that helps build easy-to-use applications
//! depend on window_manager_alpha which provides multiple-window management
//! 
//! The components object should be owned by application

#![no_std]

extern crate spin;
#[macro_use]
extern crate alloc;
extern crate dfqueue;
extern crate event_types;
#[macro_use]
extern crate log;
extern crate frame_buffer_alpha;
extern crate font;
extern crate spawn;
extern crate mouse;
extern crate path;
extern crate window_manager_alpha;

use path::Path;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::{String, ToString};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::ops::{Deref, DerefMut};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use event_types::{Event, MousePositionEvent};
use frame_buffer_alpha::{ FrameBufferAlpha, Pixel, FINAL_FRAME_BUFFER, alpha_mix, color_mix };
use spin::{Mutex, Once};
use spawn::{KernelTaskBuilder, ApplicationTaskBuilder};
use window_manager_alpha::WindowObjAlpha;

/// The top bar size
const WINDOW_TOPBAR: usize = 15;
/// left, right, bottom border size
const WINDOW_BORDER: usize = 2;
/// border radius
const WINDOW_RADIUS: usize = 5;
/// default background color
const WINDOW_BACKGROUND: Pixel = 0x40FFFFFF;
/// border and bar color
const WINDOW_BORDER_COLOR_INACTIVE: Pixel = 0x00333333;
const WINDOW_BORDER_COLOR_ACTIVE_TOP: Pixel = 0x00BBBBBB;
const WINDOW_BORDER_COLOR_ACTIVE_BOTTOM: Pixel = 0x00666666;
/// window button color
const WINDOW_BUTTON_COLOR_CLOSE: Pixel = 0x00E74C3C;
const WINDOW_BUTTON_COLOR_MINIMIZE: Pixel = 0x00239B56;
const WINDOW_BUTTON_COLOR_MAXIMIZE: Pixel = 0x007D3C98;
const WINDOW_BUTTON_BIAS_X: usize = 12;
const WINDOW_BUTTON_BETWEEN: usize = 15;
const WINDOW_BUTTON_SIZE: usize = 6;
const WINDOW_BUTTON_COLOR_ERROR: Pixel = 0x00000000;  // black

// do not allow overlapping of components, to reduce complexity
// people who needs that attribute should realize another WindowComponents which allowes that
pub struct WindowComponents {
    components: BTreeMap<String, Arc<Mutex<Component>>>,
    pub winobj: Arc<Mutex<WindowObjAlpha>>,
    pub bias_x: usize,  // init as WINDOW_BORDER, should not be modified
    pub bias_y: usize,  // init as WINDOW_TOPBAR, should not be modified
    pub background: Pixel,  // init as WINDOW_BACKGROUND
    pub consumer: DFQueueConsumer<Event>,  // event input
    producer: DFQueueProducer<Event>,  // event output used by window manager

    last_mouse_position_event: MousePositionEvent,
}

impl WindowComponents {
    pub fn new(x: usize, y: usize, width: usize, height: usize) -> Result<Arc<Mutex<WindowComponents>>, &'static str> {

        if width <= 2 * WINDOW_TOPBAR || height <= WINDOW_TOPBAR + WINDOW_BORDER {
            return Err("window too small to even draw border");
        }

        let _winobj = window_manager_alpha::new_window(x, y, width, height)?;

        let consumer = DFQueue::new().into_consumer();
        let producer = consumer.obtain_producer();

        let mut wincomps: WindowComponents = WindowComponents {
            components: BTreeMap::new(),
            winobj: _winobj,
            bias_x: WINDOW_BORDER,
            bias_y: WINDOW_TOPBAR,
            background: WINDOW_BACKGROUND,
        consumer: consumer,
        producer: producer,
            last_mouse_position_event: MousePositionEvent {
                x: 0, y: 0, gx: 0, gy: 0,
                scrolling_up: false, scrolling_down: false,
                left_button_hold: false, right_button_hold: false,
                fourth_button_hold: false, fifth_button_hold: false,
            }
        };

        let mut winobj = wincomps.winobj.lock();
        winobj.framebuffer.fullfill_color(wincomps.background);
        let xs = winobj.x;
        let xe = xs + winobj.width;
        let ys = winobj.y;
        let ye = ys + winobj.height;
        drop(winobj);
        wincomps.draw_border(true);  // active border
        window_manager_alpha::refresh_area_absolute(xs, xe, ys, ye)?;

        Ok(Arc::new(Mutex::new(wincomps)))
    }

    pub fn draw_border(&mut self, active: bool) {
        let mut winobj = self.winobj.lock();
        // first draw left, bottom, right border
        let mut border_color = WINDOW_BORDER_COLOR_INACTIVE;
        if active {
            border_color = WINDOW_BORDER_COLOR_ACTIVE_BOTTOM;
        }
        let width = winobj.width;
        let height = winobj.height;
        winobj.framebuffer.draw_rect(0, self.bias_x, self.bias_y, height, border_color);
        winobj.framebuffer.draw_rect(0, width, height - self.bias_x, height, border_color);
        winobj.framebuffer.draw_rect(width - self.bias_x, width, self.bias_y, height, border_color);
        // then draw the top bar
        if active {
            for i in 0..self.bias_y {
                winobj.framebuffer.draw_rect(0, width, i, i+1, frame_buffer_alpha::color_mix(
                    WINDOW_BORDER_COLOR_ACTIVE_BOTTOM, WINDOW_BORDER_COLOR_ACTIVE_TOP, (i as f32) / (self.bias_y as f32)
                ));
            }
        } else {
            winobj.framebuffer.draw_rect(0, width, 0, self.bias_y, border_color);
        }
        // draw radius finally
        let r2 = WINDOW_RADIUS * WINDOW_RADIUS;
        for i in 0..WINDOW_RADIUS {
            for j in 0..WINDOW_RADIUS {
                let dx1 = WINDOW_RADIUS - i;
                let dy1 = WINDOW_RADIUS - j;
                if dx1*dx1 + dy1*dy1 > r2 {  // draw this to transparent
                    winobj.framebuffer.draw_point(i, j, 0xFFFFFFFF);
                    winobj.framebuffer.draw_point(width-i-1, j, 0xFFFFFFFF);
                }
            }
        }
        // draw three buttons
        self.show_button(0, 1, &mut winobj);
        self.show_button(1, 1, &mut winobj);
        self.show_button(2, 1, &mut winobj);
    }

    /// show three button with status. idx = 0,1,2, state = 0,1,2 
    fn show_button(& self, idx: usize, state: usize, mut winobj: &mut WindowObjAlpha) {
        if idx > 2 { return; }
        if state > 2 { return; }
        let y = self.bias_y / 2;
        let x = WINDOW_BUTTON_BIAS_X + idx * WINDOW_BUTTON_BETWEEN;
        winobj.framebuffer.draw_circle_alpha(x, y, WINDOW_BUTTON_SIZE, color_mix(
            0x00000000, match idx {
                0 => WINDOW_BUTTON_COLOR_CLOSE,
                1 => WINDOW_BUTTON_COLOR_MINIMIZE,
                2 => WINDOW_BUTTON_COLOR_MAXIMIZE,
                _ => WINDOW_BUTTON_COLOR_ERROR,
            }, 0.2f32 * (state as f32)
        ));
    }

    fn refresh_three_button(& self, bx: usize, by: usize) -> Result<(), &'static str> {
        for i in 0..3 {
            let y = by + self.bias_y / 2;
            let x = bx + WINDOW_BUTTON_BIAS_X + i * WINDOW_BUTTON_BETWEEN;
            let r = WINDOW_RADIUS;
            window_manager_alpha::refresh_area_absolute(x-r, x+r+1, y-r, y+r+1)?;
        }
        Ok(())
    }

    pub fn inner_size(& self) -> (usize, usize) {
        let winobj = self.winobj.lock();
        (winobj.width - 2 * self.bias_x, winobj.height - self.bias_x - self.bias_y)
    }

    pub fn handle_event(&mut self) {
        let mut winobj = self.winobj.lock();
        let mut consumer = &winobj.consumer;
        let event = match consumer.peek() {
            Some(ev) => ev,
            _ => { return; }
        };
        let mut call_later__do_refresh_floating_border = false;
        let mut call_later__do_move_active_window = false;
        match event.deref() {
            &Event::InputEvent(ref input_event) => {
                let key_input = input_event.key_event;
                self.producer.enqueue(Event::new_input_event(key_input));
            }
            &Event::MousePositionEvent(ref mouse_event) => {
                // debug!("mouse_event: {:?}", mouse_event);
                if winobj.is_moving {  // only wait for left button up to exit this mode
                    if ! mouse_event.left_button_hold {
                        winobj.is_moving = false;
                        winobj.give_all_mouse_event = false;
                        self.last_mouse_position_event = mouse_event.clone();
                        call_later__do_refresh_floating_border = true;
                        call_later__do_move_active_window = true;
                    }
                } else {
                    if mouse_event.y < self.bias_y {  // the region of top bar
                        let r2 = WINDOW_RADIUS * WINDOW_RADIUS;
                        let mut is_three_button = false;
                        for i in 0..3 {
                            let dx = mouse_event.x - WINDOW_BUTTON_BIAS_X - i * WINDOW_BUTTON_BETWEEN;
                            let dy = mouse_event.y - self.bias_y / 2;
                            if dx*dx + dy*dy <= r2 {
                                is_three_button = true;
                                if mouse_event.left_button_hold {
                                    self.show_button(i, 2, &mut winobj);
                                } else {
                                    self.show_button(i, 0, &mut winobj);
                                }
                            } else {
                                self.show_button(i, 1, &mut winobj);
                            }
                        }
                        // check if user push the top bar, which means user willing to move the window
                        if !is_three_button && !self.last_mouse_position_event.left_button_hold && mouse_event.left_button_hold {
                            winobj.is_moving = true;
                            winobj.give_all_mouse_event = true;
                            winobj.moving_base = (mouse_event.gx, mouse_event.gy);
                            call_later__do_refresh_floating_border = true;
                        }
                        self.last_mouse_position_event = mouse_event.clone();
                    } else {  // the region of components
                        // TODO: if any components want this event? ask them!
                        self.producer.enqueue(Event::MousePositionEvent(mouse_event.clone()));
                    }
                }
            }
            _ => { return; }
        };
        let bx = winobj.x;
        let by = winobj.y;
        drop(winobj);
        match self.refresh_three_button(bx, by) {
            Ok(_) => { }
            Err(err) => { debug!("refresh_three_button failed {}", err); }
        }
        if call_later__do_refresh_floating_border {
            match window_manager_alpha::do_refresh_floating_border() {
                Ok(_) => { }
                Err(err) => { debug!("do_refresh_floating_border failed {}", err); }
            }
        }
        if call_later__do_move_active_window {
            match window_manager_alpha::do_move_active_window() {
                Ok(_) => { }
                Err(err) => { debug!("do_move_active_window failed {}", err); }
            }
        }

        event.mark_completed();
    }
}

pub enum Component {
    textarea(TextArea),
}

/// a textarea with fixed size, showing matrix of chars
pub struct TextArea {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    line_spacing: usize,
    column_spacing: usize,
    background_color: Pixel,
    text_color: Pixel,
    pub x_cnt: usize,  // do not change this
    pub y_cnt: usize,  // do not change this
    char_matrix: Vec<u8>,
    winobj: Weak<Mutex<WindowObjAlpha>>,
}

impl TextArea {
    pub fn new(x: usize, y: usize, width: usize, height: usize, winobj: &Arc<Mutex<WindowObjAlpha>>
            , line_spacing: Option<usize>, column_spacing: Option<usize>
            , background_color: Option<Pixel>, text_color: Option<Pixel>)
        -> Result<Arc<Mutex<TextArea>>, &'static str> {

        let mut textarea: TextArea = TextArea {
            x: x,
            y: y,
            width: width,
            height: height,
            line_spacing: match line_spacing {
                Some(m) => m,
                _ => 2,
            },
            column_spacing: match column_spacing {
                Some(m) => m,
                _ => 1,
            },
            background_color: match background_color {
                Some(m) => m,
                _ => 0xFFFFFFFF,  // default is a transparent one
            },
            text_color: match text_color {
                Some(m) => m,
                _ => 0x00000000,  // default is an opaque black
            },
            x_cnt: 0,
            y_cnt: 0,
            char_matrix: Vec::new(),
            winobj: Arc::downgrade(winobj),
        };

        // compute x_cnt and y_cnt and remain constant
        if height < (16 + textarea.line_spacing) || width < (8 + textarea.column_spacing) {
            return Err("textarea too small to put even one char");
        }
        textarea.x_cnt = width / (8 + textarea.column_spacing);
        textarea.y_cnt = height / (16 + textarea.line_spacing);
        textarea.char_matrix.resize(textarea.x_cnt * textarea.y_cnt, ' ' as u8);  // first fill with blank char
        // for i in 0..textarea.x_cnt {
        //     for j in 0..textarea.y_cnt {
        //         textarea.set_char(i, j, ' ' as u8)?;  // set them all to ' ', means nothing to show
        //     }
        // }

        Ok(Arc::new(Mutex::new(textarea)))
    }

    pub fn index(& self, x: usize, y: usize) -> usize {  // does not check bound
        return x + y * self.x_cnt;
    }

    pub fn set_char_absolute(&mut self, idx: usize, c: u8) -> Result<(), &'static str> {
        if idx >= self.x_cnt * self.y_cnt { return Err("x out of range"); }
        self.set_char(idx % self.x_cnt, idx / self.x_cnt, c)
    }

    pub fn set_char(&mut self, x: usize, y: usize, c: u8) -> Result<(), &'static str> {
        if x >= self.x_cnt { return Err("x out of range"); }
        if y >= self.y_cnt { return Err("y out of range"); }
        if let Some(_winobj) = self.winobj.upgrade() {
            if self.char_matrix[self.index(x, y)] != c {  // need to redraw
                let idx = self.index(x, y);
                self.char_matrix[idx] = c;
                let wx = self.x + x * (8 + self.column_spacing);
                let wy = self.y + y * (16 + self.line_spacing);
                let mut winobj = _winobj.lock();
                let winx = winobj.x;
                let winy = winobj.y;
                for j in 0..16 {
                    let char_font: u8 = font::FONT_BASIC[c as usize][j];
                    for i in 0..8 {
                        let nx = wx + i;
                        let ny = wy + j;
                        if char_font & (0x80u8 >> i) != 0 {
                            winobj.framebuffer.draw_point(nx, ny, self.text_color);
                        } else {
                            winobj.framebuffer.draw_point(nx, ny, self.background_color);
                        }
                    }
                }
                drop(winobj);  // release the lock
                for j in 0..16 {
                    for i in 0..8 {
                        window_manager_alpha::refresh_pixel_absolute(winx + wx + i, winy + wy + j)?;
                    }
                }
            }
        } else {
            return Err("winobj not existed");
        }
        Ok(())
    }

    pub fn set_char_matrix(&mut self, char_matrix: &Vec<u8>) -> Result<(), &'static str> {
        if char_matrix.len() != self.char_matrix.len() {
            return Err("char matrix size not match");
        }
        for i in 0 .. self.x_cnt {
            for j in 0 .. self.y_cnt {
                self.set_char(i, j, char_matrix[self.index(i, j)])?;
            }
        }
        Ok(())
    }

    pub fn display_string_basic(&mut self, _a: &str) -> Result<(), &'static str> {
        let a = _a.as_bytes();
        let mut i = 0;
        let mut j = 0;
        for k in 0 .. a.len() {
            let c = a[k] as u8;
            // debug!("{}", a[k] as u8);
            if c == '\n' as u8 {
                for x in i .. self.x_cnt {
                    self.set_char(x, j, ' ' as u8)?;
                }
                j += 1;
                i = 0;
            } else {
                self.set_char(i, j, c)?;
                i += 1;
                if i >= self.x_cnt {
                    j += 1;
                    i = 0;
                }
            }
            if j >= self.y_cnt { break; }
        }
        if j < self.y_cnt {
            for x in i .. self.x_cnt {
                self.set_char(x, j, ' ' as u8)?;
            }
            for y in j+1 .. self.y_cnt {
                for x in 0 .. self.x_cnt {
                    self.set_char(x, y, ' ' as u8)?;
                }
            }
        }
        Ok(())
    }
}
