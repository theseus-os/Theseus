//! This Window Components is designated to help user to build easy-to-use GUI applications
//! 
//! The `window_components` object should be owned by application, see `applications/new_window` as a demo to use this library
//! 
//! This library will create a window with default title bar and border. It handles the commonly used interactions like moving 
//! the window or close the window. Also, it is responsible to show title bar differently when window is active. The library 
//! frees applications from handling the complicated interaction with window manager, however, advanced users could learn from 
//! this library about how to use window manager APIs directly.
//! 
//! Currently the component only supports `textarea` which is a fixed size area to display matrix of characters. User could implement 
//! other components in other crate, means this library has not a centralized control of all components, leaving flexibility to user. 
//! Basically, `textarea` is initialized with the Arc of window object which is initialized by `window_components`, means, 
//! `window_components` and `textarea` are in the same level, except for the priority to handle events.

#![no_std]

extern crate spin;
extern crate alloc;
extern crate dfqueue;
extern crate event_types;
#[macro_use]
extern crate log;
extern crate frame_buffer_alpha;
extern crate font;
extern crate mouse;
extern crate window_manager_alpha;
extern crate frame_buffer;
extern crate window;

use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::ops::{Deref};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use event_types::{Event, MousePositionEvent};
use frame_buffer_alpha::{ AlphaPixel, BLACK, PixelMixer };
use spin::{Mutex};
use window_manager_alpha::{WindowAlpha, WINDOW_MANAGER};
use frame_buffer::{Coord, FrameBuffer};
use window::Window;

/// The title bar size, in number of pixels
const WINDOW_TITLE_BAR: usize = 15;
/// left, right, bottom border size, in number of pixels
const WINDOW_BORDER: usize = 2;
/// border radius, in number of pixels
const WINDOW_RADIUS: usize = 5;
/// default background color
const WINDOW_BACKGROUND: AlphaPixel = 0x40FFFFFF;
/// border and title bar color when window is inactive
const WINDOW_BORDER_COLOR_INACTIVE: AlphaPixel = 0x00333333;
/// border and title bar color when window is active, the top part color
const WINDOW_BORDER_COLOR_ACTIVE_TOP: AlphaPixel = 0x00BBBBBB;
/// border and title bar color when window is active, the bottom part color
const WINDOW_BORDER_COLOR_ACTIVE_BOTTOM: AlphaPixel = 0x00666666;
/// window button color: red
const WINDOW_BUTTON_COLOR_CLOSE: AlphaPixel = 0x00E74C3C;
/// window button color: green
const WINDOW_BUTTON_COLOR_MINIMIZE_MAMIMIZE: AlphaPixel = 0x00239B56;
/// window button color: purple
const WINDOW_BUTTON_COLOR_HIDE: AlphaPixel = 0x007D3C98;
/// window button margin from left, in number of pixels
const WINDOW_BUTTON_BIAS_X: usize = 12;
/// the interval between buttons, in number of pixels
const WINDOW_BUTTON_BETWEEN: usize = 15;
/// the button size, in number of pixels
const WINDOW_BUTTON_SIZE: usize = 6;

/// The buttons shown in title bar
enum TopButton {
    /// Button to close the window
    Close,
    /// Button to minimize/maximize the window (depends on the current state)
    MinimizeMaximize,
    /// Button to hide the window
    Hide,
}

impl From<usize> for TopButton {
    fn from(item: usize) -> Self {
        match item {
            0 => TopButton::Close,
            1 => TopButton::MinimizeMaximize,
            2 => TopButton::Hide,
            _ => TopButton::Close
        }
    }
}

/// abstraction of a window, providing title bar which helps user moving, close, maximize or minimize window
pub struct WindowComponents {
    /// the window object that could be used to initialize components
    pub winobj: Arc<Mutex<WindowAlpha>>,
    /// the width of border, init as WINDOW_BORDER. the border is still part of the window and remains flexibility for user to change border style or remove border. However, for most application a border is useful for user to identify the region.
    border_size: usize,
    /// the height of title bar in pixel, init as WINDOW_TITLE_BAR. it is render inside the window so user shouldn't use this area anymore
    title_size: usize,
    /// the background of this window, init as WINDOW_BACKGROUND
    background: AlphaPixel,
    /// application could get events from this consumer
    pub consumer: DFQueueConsumer<Event>,
    /// event output used by window manager, private variable
    producer: DFQueueProducer<Event>,

    /// last mouse position event, used to judge click and press-moving event
    last_mouse_position_event: MousePositionEvent,
    /// record last result of whether this window is active, to reduce redraw overhead
    last_is_active: bool,
}

impl WindowComponents {
    /// create new WindowComponents by given position and size, return the Mutex of it for ease of sharing
    /// x, y is the distance in pixel relative to left-top of window
    pub fn new(x: isize, y: isize, width: usize, height: usize) -> Result<Arc<Mutex<WindowComponents>>, &'static str> {

        if width <= 2 * WINDOW_TITLE_BAR || height <= WINDOW_TITLE_BAR + WINDOW_BORDER {
            return Err("window too small to even draw border");
        }

        let winobj_mutex = window_manager_alpha::new_window(x, y, width, height)?;

        // create event queue for components
        let consumer = DFQueue::new().into_consumer();
        let producer = consumer.obtain_producer();

        let mut wincomps: WindowComponents = WindowComponents {
            winobj: winobj_mutex,
            border_size: WINDOW_BORDER,
            title_size: WINDOW_TITLE_BAR,
            background: WINDOW_BACKGROUND,
            consumer: consumer,
            producer: producer,
            last_mouse_position_event: MousePositionEvent {
                x: 0, y: 0, gx: 0, gy: 0,
                scrolling_up: false, scrolling_down: false,
                left_button_hold: false, right_button_hold: false,
                fourth_button_hold: false, fifth_button_hold: false,
            },
            last_is_active: true,  // new window is by default active
        };

        let (x_start, x_end, y_start, y_end) = {
            let mut winobj = wincomps.winobj.lock();
            winobj.framebuffer.fill_color(wincomps.background);
            let coordinate = winobj.get_content_position();
            let x_start = coordinate.x;
            let x_end = x_start + winobj.width as isize;
            let y_start = coordinate.y;
            let y_end = y_start + winobj.height as isize;
            (x_start, x_end, y_start, y_end)
        };
        wincomps.draw_border(true);  // draw window with active border
        // draw three buttons
        {
            let mut winobj = wincomps.winobj.lock();
            wincomps.show_button(TopButton::Close, 1, &mut winobj);
            wincomps.show_button(TopButton::MinimizeMaximize, 1, &mut winobj);
            wincomps.show_button(TopButton::Hide, 1, &mut winobj);
        }
        debug!("before refresh");
        window_manager_alpha::refresh_area_absolute(x_start, x_end, y_start, y_end)?;
        debug!("after refresh");

        Ok(Arc::new(Mutex::new(wincomps)))
    }

    /// draw the border of this window, with argument of whether this window is active now
    fn draw_border(&mut self, active: bool) {
        let mut winobj = self.winobj.lock();
        // first draw left, bottom, right border
        let mut border_color = WINDOW_BORDER_COLOR_INACTIVE;
        if active {
            border_color = WINDOW_BORDER_COLOR_ACTIVE_BOTTOM;
        }
        let width = winobj.width;
        let height = winobj.height;
        winobj.framebuffer.draw_rect(0, self.border_size, self.title_size, height, border_color);
        winobj.framebuffer.draw_rect(0, width, height - self.border_size, height, border_color);
        winobj.framebuffer.draw_rect(width - self.border_size, width, self.title_size, height, border_color);
        // then draw the title bar
        if active {
            for i in 0..self.title_size {
                winobj.framebuffer.draw_rect(0, width, i, i+1, WINDOW_BORDER_COLOR_ACTIVE_BOTTOM.color_mix(
                    WINDOW_BORDER_COLOR_ACTIVE_TOP, (i as f32) / (self.title_size as f32)
                ));
            }
        } else {
            winobj.framebuffer.draw_rect(0, width, 0, self.title_size, border_color);
        }
        // draw radius finally
        let r2 = WINDOW_RADIUS * WINDOW_RADIUS;
        for i in 0..WINDOW_RADIUS {
            for j in 0..WINDOW_RADIUS {
                let dx1 = WINDOW_RADIUS - i;
                let dy1 = WINDOW_RADIUS - j;
                if dx1*dx1 + dy1*dy1 > r2 {  // draw this to transparent
                    winobj.framebuffer.draw_pixel(Coord::new(i as isize, j as isize), 0xFFFFFFFF);
                    winobj.framebuffer.draw_pixel(Coord::new((width-i-1) as isize, j as isize), 0xFFFFFFFF);
                }
            }
        }
    }

    /// refresh border, telling window manager to refresh 
    fn refresh_border(& self, bx: isize, by: isize) -> Result<(), &'static str> {
        let (width, height) = {
            let winobj = self.winobj.lock();
            let width = winobj.width;
            let height = winobj.height;
            (width as isize, height as isize)
        };
        let border_size = self.border_size as isize;
        let title_size = self.title_size as isize;
        window_manager_alpha::refresh_area_absolute(bx, bx+border_size, by+title_size, by+height)?;
        window_manager_alpha::refresh_area_absolute(bx, bx+width, by+height - border_size, by+height)?;
        window_manager_alpha::refresh_area_absolute(bx+width - border_size, bx+width, by+title_size, by+height)?;
        window_manager_alpha::refresh_area_absolute(bx, bx+width, by, by+title_size)?;
        Ok(())
    }

    /// show three button with status. state = 0,1,2 for three different color
    fn show_button(& self, button: TopButton, state: usize, winobj: &mut WindowAlpha) {
        let y = self.title_size / 2;
        let x = WINDOW_BUTTON_BIAS_X + WINDOW_BUTTON_BETWEEN * match button {
            TopButton::Close => 0,
            TopButton::MinimizeMaximize => 1,
            TopButton::Hide => 2,
        };
        winobj.framebuffer.draw_circle_alpha(x, y, WINDOW_BUTTON_SIZE, BLACK.color_mix(
            match button {
                TopButton::Close => WINDOW_BUTTON_COLOR_CLOSE,
                TopButton::MinimizeMaximize => WINDOW_BUTTON_COLOR_MINIMIZE_MAMIMIZE,
                TopButton::Hide => WINDOW_BUTTON_COLOR_HIDE,
            }, 0.2f32 * (state as f32)
        ));
    }

    /// refresh the top left three button's appearance
    fn refresh_three_button(& self, bx: usize, by: usize) -> Result<(), &'static str> {
        for i in 0..3 {
            let y = by + self.title_size / 2;
            let x = bx + WINDOW_BUTTON_BIAS_X + i * WINDOW_BUTTON_BETWEEN;
            let r = WINDOW_RADIUS;
            window_manager_alpha::refresh_area_absolute((x-r) as isize, (x+r+1) as isize, (y-r) as isize, (y+r+1) as isize)?;
        }
        Ok(())
    }

    /// return the available inner size, excluding title bar and border
    pub fn inner_size(& self) -> (usize, usize) {
        let winobj = self.winobj.lock();
        (winobj.width - 2 * self.border_size, winobj.height - self.border_size - self.title_size)
    }

    /// event handler that should be called periodically by applications. This will handle user events as well as produce 
    /// the unhandled ones for other components to handle.
    pub fn handle_event(&mut self) -> Result<(), &'static str> {
        let mut call_later_do_refresh_floating_border = false;
        let mut call_later_do_move_active_window = false;
        let mut need_to_set_active = false;
        let mut need_refresh_three_button = false;
        let is_active = window_manager_alpha::is_active(&self.winobj);
        if is_active != self.last_is_active {
            self.draw_border(is_active);
            self.last_is_active = is_active;
            let (bx, by) = {
                let mut winobj = self.winobj.lock();
                let coordinate = winobj.get_content_position();
                let bx = coordinate.x;
                let by = coordinate.y;
                self.show_button(TopButton::Close, 1, &mut winobj);
                self.show_button(TopButton::MinimizeMaximize, 1, &mut winobj);
                self.show_button(TopButton::Hide, 1, &mut winobj);
                (bx, by)
            };
            if let Err(err) = self.refresh_border(bx, by) {
                error!("refresh_border failed {}", err);
            }
        }
        let (bx, by) = {
            let mut winobj = self.winobj.lock();
            let consumer = &winobj.consumer;
            let event = match consumer.peek() {
                Some(ev) => ev,
                _ => { return Ok(()); }
            };
            let coordinate = winobj.get_content_position();
            let bx = coordinate.x;
            let by = coordinate.y;
            match event.deref() {
                &Event::KeyboardEvent(ref input_event) => {
                    let key_input = input_event.key_event;
                    self.producer.enqueue(Event::new_keyboard_event(key_input));
                }
                &Event::MousePositionEvent(ref mouse_event) => {
                    // debug!("mouse_event: {:?}", mouse_event);
                    if winobj.is_moving() {  // only wait for left button up to exit this mode
                        if ! mouse_event.left_button_hold {
                            winobj.set_is_moving(false);
                            winobj.set_give_all_mouse_event(false);
                            self.last_mouse_position_event = mouse_event.clone();
                            call_later_do_refresh_floating_border = true;
                            call_later_do_move_active_window = true;
                        }
                    } else {
                        if (mouse_event.y as usize) < self.title_size && (mouse_event.x as usize) < winobj.width {  // the region of title bar
                            let r2 = WINDOW_RADIUS * WINDOW_RADIUS;
                            let mut is_three_button = false;
                            for i in 0..3 {
                                let dx = mouse_event.x - WINDOW_BUTTON_BIAS_X as isize - (i as isize) * WINDOW_BUTTON_BETWEEN as isize;
                                let dy = mouse_event.y - self.title_size as isize / 2;
                                if dx*dx + dy*dy <= r2 as isize {
                                    is_three_button = true;
                                    if mouse_event.left_button_hold {
                                        self.show_button(TopButton::from(i), 2, &mut winobj);
                                        need_refresh_three_button = true;
                                    } else {
                                        self.show_button(TopButton::from(i), 0, &mut winobj);
                                        need_refresh_three_button = true;
                                        if self.last_mouse_position_event.left_button_hold {  // click event
                                            if i == 0 {
                                                debug!("close window");
                                                return Err("user close window");  // window will not close until app drop self
                                            }
                                        }
                                    }
                                } else {
                                    self.show_button(TopButton::from(i), 1, &mut winobj);
                                    need_refresh_three_button = true;
                                }
                            }
                            // check if user push the title bar, which means user willing to move the window
                            if !is_three_button && !self.last_mouse_position_event.left_button_hold && mouse_event.left_button_hold {
                                winobj.set_is_moving(true);
                                winobj.set_give_all_mouse_event(true);
                                winobj.moving_base = Coord::new(mouse_event.gx, mouse_event.gy);
                                call_later_do_refresh_floating_border = true;
                            }
                        } else {  // the region of components
                            // TODO: if any components want this event? ask them!
                            self.producer.enqueue(Event::MousePositionEvent(mouse_event.clone()));
                        }
                        if (mouse_event.y as usize) < winobj.height && (mouse_event.x as usize) < winobj.width &&
                                !self.last_mouse_position_event.left_button_hold && mouse_event.left_button_hold {
                            need_to_set_active = true;
                        }
                        self.last_mouse_position_event = mouse_event.clone();
                    }
                }
                _ => { return Ok(()); }
            };
            event.mark_completed();
            (bx, by)
        };
        if need_to_set_active {
            if let Err(err) = window_manager_alpha::set_active(&self.winobj) {
                error!("cannot set to active {}", err);
            }
        }
        if need_refresh_three_button {  // if border has been refreshed, no need to refresh buttons
            if let Err(err) = self.refresh_three_button(bx as usize, by as usize) {
                error!("refresh_three_button failed {}", err);
            }
        }
        if call_later_do_refresh_floating_border {
            if let Err(err) = window_manager_alpha::do_refresh_floating_border() {
                error!("do_refresh_floating_border failed {}", err);
            }
        }
        if call_later_do_move_active_window {
            if let Err(err) = window_manager_alpha::do_move_active_window() {
                error!("do_move_active_window failed {}", err);
            }
        }

        window_manager_alpha::render(None)
    }

    /// get space remained for border, in number of pixel. There is border on the left, right and bottom. 
    /// When user add their components, should margin its area to avoid overlapping these borders.
    pub fn get_border_size(&self) -> usize { self.border_size }

    /// get space remained for title bar, in number of pixel. The title bar is on the top of the window, so when user 
    /// add their components, should margin its area to avoid overlapping the title bar.
    pub fn get_title_size(&self) -> usize { self.title_size }

    /// get background color
    pub fn get_background(&self) -> AlphaPixel { self.background }
}

impl Drop for WindowComponents {
    fn drop(&mut self) {
        match WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized") {
            Ok(wm) => {
                if let Err(err) = wm.lock().delete_window(&self.winobj) {
                    error!("delete_window failed {}", err);
                }
            },
            Err(err) => {
                error!("delete_window failed {}", err);
            }
        }
       
    }
}

/// a textarea with fixed size, showing matrix of chars.
///
/// The chars are allowed to be modified and update, however, one cannot change the matrix size during run-time.
pub struct TextArea {
    x: usize,
    y: usize,
    // width: usize,
    // height: usize,
    line_spacing: usize,
    column_spacing: usize,
    background_color: AlphaPixel,
    text_color: AlphaPixel,
    /// the x dimension char count
    x_cnt: usize,
    /// the y dimension char count
    y_cnt: usize,
    char_matrix: Vec<u8>,
    winobj: Weak<Mutex<WindowAlpha>>,
    next_index: usize,
}

impl TextArea {
    /// create new textarea with all characters initialized as ' ' (space character which shows nothing).
    /// after initialization, this textarea has a weak reference to the window object, 
    /// and calling the API to change textarea will immediately update display on screen
    ///
    /// * `x`, `y`, `width`, `height`: the position and size of this textarea. Note that position is relative to window
    /// * `line_spacing`: the spacing between lines, default to 2
    /// * `column_spacing`: the spacing between chars, default to 1
    /// * `background_color`: the background color, default to transparent
    /// * `text_color`: the color of text, default to opaque black
    pub fn new(x: usize, y: usize, width: usize, height: usize, winobj: &Arc<Mutex<WindowAlpha>>
            , line_spacing: Option<usize>, column_spacing: Option<usize>
            , background_color: Option<AlphaPixel>, text_color: Option<AlphaPixel>)
        -> Result<Arc<Mutex<TextArea>>, &'static str> {

        let mut textarea: TextArea = TextArea {
            x: x,
            y: y,
            // width: width,
            // height: height,
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
            x_cnt: 0,  // will modify later
            y_cnt: 0,  // will modify later
            char_matrix: Vec::new(),
            winobj: Arc::downgrade(winobj),
            next_index: 0,
        };

        // compute x_cnt and y_cnt and remain constant
        if height < (16 + textarea.line_spacing) || width < (8 + textarea.column_spacing) {
            return Err("textarea too small to put even one char");
        }
        textarea.x_cnt = width / (8 + textarea.column_spacing);
        textarea.y_cnt = height / (16 + textarea.line_spacing);
        textarea.char_matrix.resize(textarea.x_cnt * textarea.y_cnt, ' ' as u8);  // first fill with blank char

        Ok(Arc::new(Mutex::new(textarea)))
    }

    /// get the x dimension char count
    pub fn get_x_cnt(& self) -> usize {
        self.x_cnt
    }

    /// get the y dimension char count
    pub fn get_y_cnt(& self) -> usize {
        self.y_cnt
    }

    pub fn get_next_index(&self) -> usize {
        self.next_index
    }

    /// compute the index of char, does not check bound. one can use this to compute index as argument for `set_char_absolute`.
    pub fn index(& self, x: usize, y: usize) -> usize {  // does not check bound
        return x + y * self.x_cnt;
    }

    /// set char at given index, for example, if you want to modify the char at (i, j), the `idx` should be `self.index(i, j)`
    pub fn set_char_absolute(&mut self, idx: usize, c: u8) -> Result<(), &'static str> {
        if idx >= self.x_cnt * self.y_cnt { return Err("x out of range"); }
        self.set_char(idx % self.x_cnt, idx / self.x_cnt, c)
    }

    /// set char at given position, where i < self.x_cnt, j < self.y_cnt
    pub fn set_char(&mut self, x: usize, y: usize, c: u8) -> Result<(), &'static str> {
        if x >= self.x_cnt { return Err("x out of range"); }
        if y >= self.y_cnt { return Err("y out of range"); }
        if let Some(winobj_mutex) = self.winobj.upgrade() {
            if self.char_matrix[self.index(x, y)] != c {  // need to redraw
                let idx = self.index(x, y);
                self.char_matrix[idx] = c;
                let wx = self.x + x * (8 + self.column_spacing);
                let wy = self.y + y * (16 + self.line_spacing);
                let (winx, winy) = {
                    let mut winobj = winobj_mutex.lock();
                    let coordinate = winobj.get_content_position();
                    let winx = coordinate.x;
                    let winy = coordinate.y;
                    for j in 0..16 {
                        let char_font: u8 = font::FONT_BASIC[c as usize][j];
                        for i in 0..8 {
                            let nx = wx + i;
                            let ny = wy + j;
                            if char_font & (0x80u8 >> i) != 0 {
                                winobj.framebuffer.draw_pixel(Coord::new(nx as isize, ny as isize), self.text_color);
                            } else {
                                winobj.framebuffer.draw_pixel(Coord::new(nx as isize, ny as isize), self.background_color);
                            }
                        }
                    }
                    (winx, winy)
                };
                for j in 0..16 {
                    for i in 0..8 {
                        window_manager_alpha::refresh_pixel_absolute(winx + wx as isize + i, winy + wy as isize + j)?;
                    }
                }
            }
        } else {
            return Err("winobj not existed, perhaps calling this function after window is destoryed");
        }
        Ok(())
    }

    /// update char matrix with a new one, must be equal size of current one
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

    /// display a basic string, only support normal chars and `\n`
    pub fn display_string_basic(&mut self, string: &str) -> Result<(), &'static str> {
        let a = string.as_bytes();
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

        self.next_index = self.index(i, j);
        
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
        window_manager_alpha::render(None)
    }
}
