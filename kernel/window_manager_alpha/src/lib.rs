//! Window manager that simulates a desktop environment with alpha channel.
//! 
//! windows overlapped each other would obey the rules of alpha channel composition
//!
//! Applications request window objects from the window manager through:
//! - new_window(`x`, `y`, `width`, `height`) provides a new window whose dimensions the caller must specify
//!
//! There are three types of window: `active`, `show_list` and `hide_list`
//! - `active` window is the only one active, who gets all keyboard event
//! - `show_list` windows are shown by their up-down relationships, with overlapped part using alpha channel composition
//! - `hide_list` windows are those not current shown but will be invoked to show later
//! Window object holder could set itself to active, pushing the last active window (if exists) to the top of show_list
//!

#![no_std]

extern crate spin;
#[macro_use]
extern crate alloc;
extern crate dfqueue;
extern crate event_types;
#[macro_use]
extern crate log;
extern crate frame_buffer_alpha;
extern crate spawn;
extern crate mouse_data;
extern crate keycodes_ascii;
extern crate path;

mod background;
use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::ops::Deref;
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use event_types::{Event, MousePositionEvent};
use frame_buffer_alpha::{ FrameBufferAlpha, Pixel, alpha_mix, color_mix };
use spin::{Mutex, Once};
use spawn::{KernelTaskBuilder, ApplicationTaskBuilder};
use mouse_data::MouseEvent;
use keycodes_ascii::{KeyEvent, Keycode, KeyAction};
use path::Path;

static WINDOW_MANAGER: Once<Mutex<WindowManagerAlpha>> = Once::new();

/// The half size of mouse in number of pixels, the actual size of pointer is 1+2*`MOUSE_POINTER_HALF_SIZE`
const MOUSE_POINTER_HALF_SIZE: usize = 7;
/// Transparent pixel
const T: Pixel = Pixel { alpha: 0xFF, red: 0x00, green: 0x00, blue: 0x00 };
/// Opaque white
const O: Pixel = Pixel { alpha: 0x00, red: 0xFF, green: 0xFF, blue: 0xFF };
/// Opaque blue
const B: Pixel = Pixel { alpha: 0x00, red: 0x00, green: 0x00, blue: 0xFF };
/// the mouse picture
const MOUSE_BASIC: [[Pixel; 2*MOUSE_POINTER_HALF_SIZE+1]; 2*MOUSE_POINTER_HALF_SIZE+1] = [
    [ T, T, T, T, T, T, T, T, T, T, T, T, T, T, T ],
    [ T, T, T, T, T, T, T, T, T, T, T, T, T, T, T ],
    [ T, T, T, T, T, T, T, T, T, T, T, T, T, T, T ],
    [ T, T, T, T, T, T, T, T, T, T, T, T, T, T, T ],
    [ T, T, T, T, T, T, T, T, T, T, T, T, T, T, T ],
    [ T, T, T, T, T, T, T, T, T, T, T, T, T, T, T ],
    [ T, T, T, T, T, T, B, B, B, B, B, B, B, B, B ],
    [ T, T, T, T, T, T, B, O, O, O, O, O, O, B, T ],
    [ T, T, T, T, T, T, B, O, O, O, O, O, B, T, T ],
    [ T, T, T, T, T, T, B, O, O, O, O, B, T, T, T ],
    [ T, T, T, T, T, T, B, O, O, O, O, B, T, T, T ],
    [ T, T, T, T, T, T, B, O, O, B, B, O, B, T, T ],
    [ T, T, T, T, T, T, B, O, B, T, T, B, O, B, T ],
    [ T, T, T, T, T, T, B, B, T, T, T, T, B, O, B ],
    [ T, T, T, T, T, T, B, T, T, T, T, T, T, B, B ],
];

/// the border indicating new window position and size
const WINDOW_BORDER_SIZE: usize = 3;
/// border's inner color
const WINDOW_BORDER_COLOR_INNER: Pixel = Pixel { alpha: 0x00, red: 0xCA, green: 0x6F, blue: 0x1E };
/// border's outter color
const WINDOW_BORDER_COLOR_OUTTER: Pixel = Pixel { alpha: 0xFF, red: 0xFF, green: 0xFF, blue: 0xFF };

/// a 2D point
struct Point {
    x: usize,
    y: usize,
}

/// a rectangle region
struct RectRegion {
    /// x start 
    x_start: isize,
    /// x end (exclusive)
    x_end: isize,
    /// y start
    y_start: isize,
    /// y end (exclusive)
    y_end: isize,
}

/// window manager with overlapping and alpha enabled
struct WindowManagerAlpha {
    /// those window currently not shown on screen
    hide_list: VecDeque<Weak<Mutex<WindowObjAlpha>>>,
    /// those window shown on screen that may overlapping each other
    show_list: VecDeque<Weak<Mutex<WindowObjAlpha>>>,
    /// the only active window, receiving all keyboard events (except for those remained for WM)
    active: Weak<Mutex<WindowObjAlpha>>,  // this one is not in show_list
    /// current mouse position
    mouse: Point,
    /// whether show the border to indicating new window's position and size
    is_show_border: bool,
    /// if show border, then where to show it
    border_position: RectRegion,
    /// to record how many terminals has been created, avoid same name
    terminal_id_counter: usize,
    /// the frame buffer that it should print on
    final_fb: FrameBufferAlpha,
    /// if it this is true, do not refresh whole screen until someone calls "refresh_area_absolute"
    delay_refresh_first_time: bool,
}

/// Window manager object that stores non-critical information
impl WindowManagerAlpha {

    /// set one window to active, push last active (if exists) to top of show_list. if `refresh` is `true`, will then refresh the window's area
    pub fn set_active(&mut self, objref: &Arc<Mutex<WindowObjAlpha>>, refresh: bool) -> Result<(), &'static str> {
        let (x_start, x_end, y_start, y_end) = {
            let winobj = objref.lock();
            let x_start = winobj.x; let y_start = winobj.y;
            let x_end = x_start + winobj.width as isize;
            let y_end = y_start + winobj.height as isize;
            (x_start, x_end, y_start, y_end)
        };
        // if it is currently actived, just return
        if let Some(current_active) = self.active.upgrade() {
            if Arc::ptr_eq(&(current_active), objref) {
                return Ok(());  // do nothing
            } else {  // save this to show_list
                self.show_list.push_front(self.active.clone());
                self.active = Weak::new();
            }
        }
        match self.is_window_in_show_list(&objref) {  // remove item in current list
            Some(i) => {
                self.show_list.remove(i);
            }, None => {}
        }
        match self.is_window_in_hide_list(&objref) {  // remove item in current list
            Some(i) => {
                self.hide_list.remove(i);
            }, None => {}
        }
        self.active = Arc::downgrade(objref);
        if refresh {
            self.refresh_area(x_start, x_end, y_start, y_end)?;
        }
        Ok(())
    }

    /// judge whether this window is in hide list and return the index of it
    fn is_window_in_show_list(&mut self, objref: &Arc<Mutex<WindowObjAlpha>>) -> Option<usize> {
        let mut i = 0_usize;
        for item in self.show_list.iter() {
            if let Some(item_ptr) = item.upgrade() {
                if Arc::ptr_eq(&(item_ptr), objref) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// judge whether this window is in hide list and return the index of it
    fn is_window_in_hide_list(&mut self, objref: &Arc<Mutex<WindowObjAlpha>>) -> Option<usize> {
        let mut i = 0_usize;
        for item in self.hide_list.iter() {
            if let Some(item_ptr) = item.upgrade() {
                if Arc::ptr_eq(&(item_ptr), objref) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// delete one window if exists, refresh its region then
    fn delete_window(&mut self, objref: &Arc<Mutex<WindowObjAlpha>>) -> Result<(), &'static str> {
        let (x_start, x_end, y_start, y_end) = {
            let winobj = objref.lock();
            let x_start = winobj.x; let y_start = winobj.y;
            let x_end = x_start + winobj.width as isize; let y_end = y_start + winobj.height as isize;
            (x_start, x_end, y_start, y_end)
        };
        if let Some(current_active) = self.active.upgrade() {
            if Arc::ptr_eq(&(current_active), objref) {
                self.active = Weak::new();  // delete reference
                self.refresh_area(x_start, x_end, y_start, y_end)?;
                return Ok(())
            }
        }
        match self.is_window_in_show_list(&objref) {
            Some(i) => {
                self.show_list.remove(i);
                self.refresh_area(x_start, x_end, y_start, y_end)?;
                return Ok(())
            }, None => {}
        }
        match self.is_window_in_hide_list(&objref) {
            Some(i) => {
                self.hide_list.remove(i);
                self.refresh_area(x_start, x_end, y_start, y_end)?;
                return Ok(())
            }, None => {}
        }
        Err("cannot find this window")
    }

    /// iterately compute single pixel within show_list
    fn recompute_single_pixel_show_list(& self, x: usize, y: usize, idx: usize) -> Pixel {
        if idx >= self.show_list.len() {
            if x < 1280 && y < 1080 {
                return background::BACKGROUND[y/2][x/2].into();
            }
            return 0x00000000.into();  // return black
        }
        if let Some(now_winobj) = self.show_list[idx].upgrade() {
            // first get current color, to determine whether further get colors below   
            let top = {
                let winobj = now_winobj.lock();
                let relative_x = x - winobj.x as usize;
                let relative_y = y - winobj.y as usize;
                let mut ret = T;  // defult is transparent
                if winobj.framebuffer.check_in_buffer(relative_x, relative_y) {
                    let top = match winobj.framebuffer.get_pixel(relative_x, relative_y) {
                        Ok(m) => m,
                        Err(_) => T,  // transparent
                    };
                    if top.alpha == 0 {  // totally opaque, so not waste computation
                        return top;
                    }
                    ret = top;
                }
                ret
            };
            let bottom = self.recompute_single_pixel_show_list(x, y, idx+1);
            return alpha_mix(bottom, top);
        } else {  // need to delete this one, since the owner has been dropped, but here is immutable >.<
            // self.show_list.remove(idx);
            return self.recompute_single_pixel_show_list(x, y, idx+1);
        }
    }

    /// refresh one pixel on frame buffer
    fn refresh_single_pixel_with_buffer(&mut self, x: usize, y: usize) -> Result<(), &'static str> {
        if ! self.final_fb.check_in_buffer(x, y) {
            return Ok(());
        }
        if let Some(current_active) = self.active.upgrade() {
            let current_active_win = current_active.lock();
            let relative_x = x - current_active_win.x as usize;
            let relative_y = y - current_active_win.y as usize;
            if current_active_win.framebuffer.check_in_buffer(relative_x,  relative_y) {
                let top = current_active_win.framebuffer.get_pixel(relative_x,  relative_y)?;
                if top.alpha == 0 {  // totally opaque, so not waste computation
                    self.final_fb.draw_point(x, y, top);
                } else {
                    let bottom = self.recompute_single_pixel_show_list(x, y, 0);
                    self.final_fb.draw_point(x, y, alpha_mix(bottom, top));
                }
            } else {
                let pixel = self.recompute_single_pixel_show_list(x, y, 0);
                self.final_fb.draw_point(x, y, pixel);
            }
        } else {  // nothing is active now
            let pixel = self.recompute_single_pixel_show_list(x, y, 0);
            self.final_fb.draw_point(x, y, pixel);
        }
        // then draw border
        if self.is_show_border {
            let (x_start, x_end, y_start, y_end) = {
                let r = &self.border_position;
                (r.x_start, r.x_end, r.y_start, r.y_end)
            };
            let sx = x as isize;
            let sy = y as isize;
            let ux_start = x_start as usize;
            let uy_start = y_start as usize;
            let ux_end_1 = (x_end - 1) as usize;
            let uy_end_1 = (y_end - 1) as usize;
            let window_border_size = WINDOW_BORDER_SIZE as isize;
            let x_in = sx >= x_start - window_border_size && sx <= x_end-1 + window_border_size;
            let y_in = sy >= y_start - window_border_size && sy <= y_end-1 + window_border_size;
            let left = ux_start - x <= WINDOW_BORDER_SIZE && y_in;
            let right = x - ux_end_1 <= WINDOW_BORDER_SIZE && y_in;
            let top = uy_start - y <= WINDOW_BORDER_SIZE && x_in;
            let bottom = y - uy_end_1 <= WINDOW_BORDER_SIZE && x_in;
            let f32_window_border_size = WINDOW_BORDER_SIZE as f32;
            if left {
                if top {  // left-top
                    let dx = ux_start - x; let dy = uy_start - y;
                    if dx+dy <= WINDOW_BORDER_SIZE {
                        self.final_fb.draw_point_alpha(x, y, color_mix(
                            WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (dx+dy) as f32 / f32_window_border_size));
                    }
                } else if bottom {  // left-bottom
                    let dx = ux_start - x; let dy = y - uy_end_1;
                    if dx+dy <= WINDOW_BORDER_SIZE {
                        self.final_fb.draw_point_alpha(x, y, color_mix(
                            WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (dx+dy) as f32 / f32_window_border_size));
                    }
                } else {  // only left
                    self.final_fb.draw_point_alpha(x, y, color_mix(
                        WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (ux_start - x) as f32 / f32_window_border_size));
                }
            } else if right {
                if top {  // right-top
                    let dx = x - ux_end_1; let dy = uy_start - y;
                    if dx+dy <= WINDOW_BORDER_SIZE {
                        self.final_fb.draw_point_alpha(x, y, color_mix(
                            WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (dx+dy) as f32 / f32_window_border_size));
                    }
                } else if bottom {  // right-bottom
                    let dx = x - ux_end_1; let dy = y - uy_end_1;
                    if dx+dy <= WINDOW_BORDER_SIZE {
                        self.final_fb.draw_point_alpha(x, y, color_mix(
                            WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (dx+dy) as f32 / f32_window_border_size));
                    }
                } else {  // only right
                    self.final_fb.draw_point_alpha(x, y, color_mix(
                        WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (x - ux_end_1) as f32 / f32_window_border_size));
                }
            } else if top {  // only top
                self.final_fb.draw_point_alpha(x, y, color_mix(
                    WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (uy_start - y) as f32 / f32_window_border_size));
            } else if bottom {  // only bottom
                self.final_fb.draw_point_alpha(x, y, color_mix(
                    WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (y - uy_end_1) as f32 / f32_window_border_size));
            }
        }
        // finally draw mouse
        let (cx, cy) = {
            let m = &self.mouse;
            (m.x, m.y)
        };
        if ((x-cx) <= MOUSE_POINTER_HALF_SIZE || (cx-x) <= MOUSE_POINTER_HALF_SIZE) && ((y-cy) <= MOUSE_POINTER_HALF_SIZE || (cy-y) <= MOUSE_POINTER_HALF_SIZE) {
            self.final_fb.draw_point_alpha(x, y, MOUSE_BASIC[MOUSE_POINTER_HALF_SIZE + x - cx][MOUSE_POINTER_HALF_SIZE + y - cy]);
        }
        Ok(())
    }

    /// recompute single pixel value and refresh it on screen
    pub fn refresh_single_pixel(&mut self, x: isize, y: isize) -> Result<(), &'static str> {
        let width = self.final_fb.width;
        let height = self.final_fb.height;
        if (x as usize) < width && (y as usize) < height {
            return self.refresh_single_pixel_with_buffer(x as usize, y as usize);
        }
        return Ok(())  // don't need to update this pixel because it is not displayed on the screen
    }

    /// refresh an area
    fn refresh_area(&mut self, x_start: isize, x_end: isize, y_start: isize, y_end: isize) -> Result<(), &'static str> {
        let x_start = core::cmp::max(x_start, 0);
        let x_end = core::cmp::min(x_end, self.final_fb.width as isize);
        let y_start = core::cmp::max(y_start, 0);
        let y_end = core::cmp::min(y_end, self.final_fb.height as isize);
        if x_start <= x_end && y_start <= y_end {
            for x in x_start .. x_end {
                for y in y_start .. y_end {
                    self.refresh_single_pixel_with_buffer(x as usize, y as usize)?;
                }
            }
        }
        Ok(())
    }

    /// refresh an rectangle border
    fn refresh_rect_border(&mut self, x_start: isize, x_end: isize, y_start: isize, y_end: isize) -> Result<(), &'static str> {
        let x_start = core::cmp::max(x_start, 0);
        let x_end = core::cmp::min(x_end, self.final_fb.width as isize);
        let y_start = core::cmp::max(y_start, 0);
        let y_end = core::cmp::min(y_end, self.final_fb.height as isize);
        if x_start <= x_end {
            if y_start < self.final_fb.height as isize {
                for x in x_start .. x_end {
                    self.refresh_single_pixel_with_buffer(x as usize, y_start as usize)?;
                }
            }
            if y_end > 0 {
                for x in x_start .. x_end {
                    self.refresh_single_pixel_with_buffer(x as usize, y_end as usize - 1)?;
                }
            }
        }
        if y_start <= y_end {
            if x_start < self.final_fb.width as isize {
                for y in y_start .. y_end {
                    self.refresh_single_pixel_with_buffer(x_start as usize, y as usize)?;
                }
            }
            if x_end > 0 {
                for y in y_start .. y_end {
                    self.refresh_single_pixel_with_buffer(x_end as usize - 1, y as usize)?;
                }
            }
        }
        Ok(())
    }

    /// pass keyboard event to currently active window
    fn pass_keyboard_event_to_window(& self, key_event: KeyEvent) -> Result<(), &'static str> {
        if let Some(current_active) = self.active.upgrade() {
            let current_active_win = current_active.lock();
            current_active_win.producer.enqueue(Event::new_input_event(key_event));
        }
        Err("cannot find window to pass key event")
    }

    /// pass mouse event to the top window that mouse on, may also transfer to those want all events
    fn pass_mouse_event_to_window(& self, mouse_event: MouseEvent) -> Result<(), &'static str> {
        let (x, y) = {
            let m = &self.mouse;
            (m.x as isize, m.y as isize)
        };
        let mut event: MousePositionEvent = MousePositionEvent {
            x: 0,
            y: 0,
            gx: x,
            gy: y,
            scrolling_up: mouse_event.mousemove.scrolling_up,
            scrolling_down: mouse_event.mousemove.scrolling_down,
            left_button_hold: mouse_event.buttonact.left_button_hold,
            right_button_hold: mouse_event.buttonact.right_button_hold,
            fourth_button_hold: mouse_event.buttonact.fourth_button_hold,
            fifth_button_hold: mouse_event.buttonact.fifth_button_hold,
        };
        // check if some application want all mouse_event
        // active
        if let Some(current_active) = self.active.upgrade() {
            let current_active_win = current_active.lock();
            let cx = current_active_win.x;
            let cy = current_active_win.y;
            if current_active_win.give_all_mouse_event {
                event.x = x - cx;
                event.y = y - cy;
                current_active_win.producer.enqueue(Event::MousePositionEvent(event.clone()));
            }
        }
        // show list
        for i in 0..self.show_list.len() {
            if let Some(now_winobj_mutex) = self.show_list[i].upgrade() {
                let now_winobj = now_winobj_mutex.lock();
                let cx = now_winobj.x;
                let cy = now_winobj.y;
                if now_winobj.give_all_mouse_event {
                    event.x = x - cx;
                    event.y = y - cy;
                    now_winobj.producer.enqueue(Event::MousePositionEvent(event.clone()));
                }
            }
        }

        // first check the active one
        if let Some(current_active) = self.active.upgrade() {
            let current_active_win = current_active.lock();
            let cx = current_active_win.x;
            let cy = current_active_win.y;
            if current_active_win.framebuffer.check_in_buffer((x - cx) as usize, (y - cy) as usize) {
                event.x = x - cx;
                event.y = y - cy;
                // debug!("pass to active: {}, {}", event.x, event.y);
                current_active_win.producer.enqueue(Event::MousePositionEvent(event));
                return Ok(());
            }
            if current_active_win.is_moving {  // do not pass the movement event to other windows
                return Ok(());
            }
        }
        // then check show_list
        for i in 0..self.show_list.len() {
            if let Some(now_winobj_mutex) = self.show_list[i].upgrade() {
                let now_winobj = now_winobj_mutex.lock();
                let cx = now_winobj.x;
                let cy = now_winobj.y;
                if now_winobj.framebuffer.check_in_buffer((x - cx) as usize, (y - cy) as usize) {
                    event.x = x - cx;
                    event.y = y - cy;
                    now_winobj.producer.enqueue(Event::MousePositionEvent(event));
                    return Ok(());
                }
            }
        }
        Err("cannot find window to pass")
    }

    /// refresh the floating border indicating user of new window position and size. `show` indicates whether to show the border or not. 
    /// (x_start, x_end, y_start, y_end) indicates the position and size of this border.
    fn refresh_floating_border(& mut self, show: bool, x_start: isize, x_end: isize, y_start: isize, y_end: isize) -> Result<(), &'static str> {
        if !self.is_show_border && !show { return Ok(()) }
        // first clear old border
        let last_show_border = self.is_show_border;
        self.is_show_border = show;
        if last_show_border {
            let (old_x_start, old_x_end, old_y_start, old_y_end) = {
                let r = &self.border_position;
                (r.x_start, r.x_end, r.y_start, r.y_end)
            };
            if show {  // otherwise don't change position
                self.border_position = RectRegion { x_start, x_end, y_start, y_end };
            }
            for i in 0..(WINDOW_BORDER_SIZE+1) as isize {
                self.refresh_rect_border(old_x_start-i, old_x_end+i, old_y_start-i, old_y_end+i)?;
            }
        }
        // then draw current border
        if show {
            self.border_position = RectRegion { x_start, x_end, y_start, y_end };
            for i in 0..(WINDOW_BORDER_SIZE+1) as isize {
                self.refresh_rect_border(x_start-i, x_end+i, y_start-i, y_end+i)?;
            }
        }
        Ok(())
    }

    /// optimized refresh area for less computation up to 2x
    fn refresh_area_with_old_new(&mut self, 
            old_x_start: isize, old_x_end: isize, old_y_start: isize, old_y_end: isize, 
            new_x_start: isize, new_x_end: isize, new_y_start: isize, new_y_end: isize) -> Result<(), &'static str> {
        // first refresh new area for better user experience
        self.refresh_area(new_x_start, new_x_end, new_y_start, new_y_end)?;
        // then refresh old area that not overlapped by new one
        let x_not_overlapped = new_x_start >= old_x_end || new_x_end <= old_x_start;
        let y_not_overlapped = new_y_start >= old_y_end || new_y_end <= old_y_start;
        if x_not_overlapped || y_not_overlapped {  // have to refresh all
            self.refresh_area(old_x_start, old_x_end, old_y_start, old_y_end)?;
        } else {  // there existes overlapped region, optimize them!
            let mut greater_x_start = old_x_start;  // this should be the larger one between `old_x_start` and `new_x_start`
            let mut smaller_x_end = old_x_end;  // this should be the smaller one between `old_x_end` and `new_x_end`
            if old_x_start < new_x_start {
                self.refresh_area(old_x_start, new_x_start, old_y_start, old_y_end)?;
                greater_x_start = new_x_start;
            }
            if old_x_end > new_x_end {
                self.refresh_area(new_x_end, old_x_end, old_y_start, old_y_end)?;
                smaller_x_end = new_x_end;
            }
            if old_y_start < new_y_start {
                self.refresh_area(greater_x_start, smaller_x_end, old_y_start, new_y_start)?;
            }
            if old_y_end > new_y_end {
                self.refresh_area(greater_x_start, smaller_x_end, new_y_end, old_y_end)?;
            }
        }
        Ok(())
    }

    /// private function: take active window's base position and current mouse, move the window with delta
    fn move_active_window(&mut self) -> Result<(), &'static str> {
        if let Some(current_active) = self.active.upgrade() {
            let (old_x_start, old_x_end, old_y_start, old_y_end, new_x_start, new_x_end, new_y_start, new_y_end) = {
                let mut current_active_win = current_active.lock();
                let (current_x, current_y) = {
                    let m = &self.mouse;
                    (m.x as isize, m.y as isize)
                };
                let (base_x, base_y) = current_active_win.moving_base;
                let old_x = current_active_win.x;
                let old_y = current_active_win.y;
                let new_x = old_x + (current_x - base_x);
                let new_y = old_y + (current_y - base_y);
                let width = current_active_win.width;
                let height = current_active_win.height;
                current_active_win.x = new_x;
                current_active_win.y = new_y;
                (old_x, old_x + width as isize, old_y, old_y + height as isize, new_x, new_x + width as isize, new_y, new_y + height as isize)
            };
            // then try to reduce time on refresh old ones
            self.refresh_area_with_old_new(old_x_start, old_x_end, old_y_start, old_y_end, new_x_start, new_x_end, new_y_start, new_y_end)?;
        } else {
            return Err("cannot fid active window to move");
        }
        Ok(())
    }
}

/// delete window the given window
pub fn delete_window(objref: &Arc<Mutex<WindowObjAlpha>>) -> Result<(), &'static str> {
    let mut win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    win.delete_window(objref)
}

/// set window as active
pub fn set_active(objref: &Arc<Mutex<WindowObjAlpha>>) -> Result<(), &'static str> {
    let mut win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    win.set_active(objref, true)
}

/// whether a window is active
pub fn is_active(objref: &Arc<Mutex<WindowObjAlpha>>) -> bool {
    match WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized") {
        Ok(mtx) => {
            let win = mtx.lock();
            if let Some(current_active) = win.active.upgrade() {
                if Arc::ptr_eq(&(current_active), objref) {
                    return true;
                }
            }
        }
        _ => {}
    }
    false
}

/// refresh the floating border display, will lock WINDOW_MANAGER. This is useful to show the window size and position without much computation, 
/// means only a thin border is updated and shown. The size and position of floating border is set inside active window by `moving_base`. Only moving 
/// is supported now, which means the relative position of current mouse and `moving_base` is actually the new position of border
pub fn do_refresh_floating_border() -> Result<(), &'static str> {
    let mut win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    let (new_x, new_y) = {
        let m = &win.mouse;
        (m.x as isize, m.y as isize)
    };
    if let Some(current_active) = win.active.upgrade() {
        let (is_draw, border_x_start, border_x_end, border_y_start, border_y_end) = {
            let current_active_win = current_active.lock();
            if current_active_win.is_moving {  // move this window
                // for better performance, while moving window, only border is shown for indication
                let current_x = current_active_win.x;
                let current_y = current_active_win.y;
                let (base_x, base_y) = current_active_win.moving_base;
                let width = current_active_win.width;
                let height = current_active_win.height;
                let border_x_start = current_x + (new_x - base_x);
                let border_x_end = border_x_start + width as isize;
                let border_y_start = current_y + (new_y - base_y);
                let border_y_end = border_y_start + height as isize;
                // debug!("drawing border: {:?}", (border_x_start, border_x_end, border_y_start, border_y_end));
                (true, border_x_start, border_x_end, border_y_start, border_y_end)
            } else {
                (false, 0, 0, 0, 0)
            }
        };
        win.refresh_floating_border(is_draw, border_x_start, border_x_end, border_y_start, border_y_end)?;  // refresh current border position
    } else {
        win.refresh_floating_border(false, 0, 0, 0, 0)?;  // hide border
    }
    Ok(())
}

/// execute moving active window action, this will lock WINDOW_MANAGER
pub fn do_move_active_window() -> Result<(), &'static str> {
    let mut win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    win.move_active_window()
}

/// refresh one pixel using absolute position, will lock WINDOW_MANAGER
pub fn refresh_pixel_absolute(x: isize, y: isize) -> Result<(), &'static str> {
    let mut win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    win.refresh_single_pixel(x, y)
}

/// refresh an area using abosolute position, will lock WINDOW_MANAGER
pub fn refresh_area_absolute(x_start: isize, x_end: isize, y_start: isize, y_end: isize) -> Result<(), &'static str> {
    let mut win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    if win.delay_refresh_first_time {
        win.delay_refresh_first_time = false;
        let width = win.final_fb.width as isize;
        let height = win.final_fb.height as isize;
        win.refresh_area_with_old_new(0, width, 0, height, x_start, x_end, y_start, y_end)   
    } else {
        win.refresh_area(x_start, x_end, y_start, y_end)   
    }
}

/// Initialize the window manager, should provide the consumer of keyboard and mouse event, as well as a frame buffer to draw
pub fn init(key_consumer: DFQueueConsumer<Event>, mouse_consumer: DFQueueConsumer<Event>, 
        final_fb: FrameBufferAlpha) -> Result<(), &'static str> {
    debug!("window manager alpha init called");

    // initialize static window manager
    let delay_refresh_first_time = true;
    let window_manager = WindowManagerAlpha {
            hide_list: VecDeque::new(),
            show_list: VecDeque::new(),
            active: Weak::new(),
            mouse: Point { x: 0, y: 0 },
            is_show_border: false,
            border_position: RectRegion { x_start: 0, x_end: 0, y_start: 0, y_end: 0 },
            terminal_id_counter: 1,
            final_fb: final_fb,
            delay_refresh_first_time: delay_refresh_first_time,
    };
    WINDOW_MANAGER.call_once(|| Mutex::new(window_manager));

    let mut win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    let screen_width = win.final_fb.width;
    let screen_height = win.final_fb.height;
    win.mouse = Point { x: screen_width/2, y: screen_height/2 };  // set mouse to middle
    if ! delay_refresh_first_time {
        win.refresh_area(0, screen_width as isize, 0, screen_height as isize)?;
    }

    KernelTaskBuilder::new(window_manager_loop, (key_consumer, mouse_consumer) )
        .name("window_manager_loop".to_string())
        .spawn()?;

    Ok(())
}

/// Window object that should be owned by application
pub struct WindowObjAlpha {
    /// absolute position of this window, the number of pixels to the left of the screen
    pub x: isize,
    /// absolute position of this window, the number of pixels to the top of the screen
    pub y: isize,
    pub width: usize,
    pub height: usize,
    /// event consumer that could be used to get event input given to this window
    pub consumer: DFQueueConsumer<Event>,  // event input
    producer: DFQueueProducer<Event>,  // event output used by window manager
    /// frame buffer of this window
    pub framebuffer: FrameBufferAlpha,

    /// if true, window manager will send all mouse event to this window, otherwise only when mouse is on this window does it send
    pub give_all_mouse_event: bool,  // whether give this application the mouse event
    /// whether in moving state, only available when it is active
    pub is_moving: bool,
    /// the base position of window moving action, should be the mouse position when `is_moving` is set to true
    pub moving_base: (isize, isize),
}

/// handles all keyboard and mouse movement in this window manager
fn window_manager_loop( consumer: (DFQueueConsumer<Event>, DFQueueConsumer<Event>) ) -> Result<(), &'static str> {
    let (key_consumer, mouse_consumer) = consumer;
    
    loop {

        let event = {
            let ev = match key_consumer.peek() {
                Some(ev) => ev,
                _ => { match mouse_consumer.peek() {
                    Some(ev) => ev,
                    _ => { continue; }
                } }
            };
            let event = ev.clone();
            ev.mark_completed();
            event
        };

        // event could be either key input or mouse input
        match event {
            Event::ExitEvent => {
                trace!("exiting the main loop of the window manager loop");
                return Ok(()); 
            }
            Event::InputEvent(ref input_event) => {
                let key_input = input_event.key_event;
                keyboard_handle_application(key_input)?;
            }
            Event::MouseInputEvent(ref mouse_event) => {
                // mouse::mouse_to_print(&mouse_event);
                let mouse_displacement = &mouse_event.displacement;
                let mut x = (mouse_displacement.x as i8) as isize;
                let mut y = (mouse_displacement.y as i8) as isize;
                // need to combine mouse events if there pending a lot
                loop {
                    let next_event = match mouse_consumer.peek() {
                        Some(ev) => ev,
                        _ => { break; }
                    };
                    match next_event.deref() {
                        &Event::MouseInputEvent(ref next_mouse_event) => {
                            if next_mouse_event.mousemove.scrolling_up == mouse_event.mousemove.scrolling_up &&
                                    next_mouse_event.mousemove.scrolling_down == mouse_event.mousemove.scrolling_down &&
                                    next_mouse_event.buttonact.left_button_hold == mouse_event.buttonact.left_button_hold &&
                                    next_mouse_event.buttonact.right_button_hold == mouse_event.buttonact.right_button_hold &&
                                    next_mouse_event.buttonact.fourth_button_hold == mouse_event.buttonact.fourth_button_hold &&
                                    next_mouse_event.buttonact.fifth_button_hold == mouse_event.buttonact.fifth_button_hold {
                                x += (next_mouse_event.displacement.x as i8) as isize;
                                y += (next_mouse_event.displacement.y as i8) as isize;
                            }
                        }
                        _ => {break;}
                    }
                    next_event.mark_completed();
                }
                if x != 0 || y != 0 {
                    move_cursor(x as isize, -(y as isize))?;
                }
                cursor_handle_application(*mouse_event)?;  // tell the event to application, or moving window
            }
            _ => { }
        }

    }
}

/// handle keyboard event, push it to the active window if exists
fn keyboard_handle_application(key_input: KeyEvent) -> Result<(), &'static str> {
    // first judge whether is system remained keys
    if key_input.modifiers.control && key_input.keycode == Keycode::T && key_input.action == KeyAction::Pressed {
        let terminal_id_counter = {
            let mut win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
            win.terminal_id_counter += 1;
            win.terminal_id_counter
        };
        let task_name: String = format!("terminal {}", terminal_id_counter);
        let args: Vec<String> = vec![]; // terminal::main() does not accept any arguments
        ApplicationTaskBuilder::new(Path::new(String::from("terminal")))
            .argument(args)
            .name(task_name)
            .spawn()?;
    }
    // then pass them to window
    let win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    if let Err(_) = win.pass_keyboard_event_to_window(key_input) {
        // note that keyboard event should be passed to currently active window
        // if no window is active now, this function will return Err, but that's OK for now.
        // This part could be used to add logic when no active window is present, how to handle keyboards, but just leave blank now
    }
    Ok(())
}

/// handle mouse event, push it to related window or anyone asked for it
fn cursor_handle_application(mouse_event: MouseEvent) -> Result<(), &'static str> {
    do_refresh_floating_border()?;
    let win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    if let Err(_) = win.pass_mouse_event_to_window(mouse_event) {
        // the mouse event should be passed to the window that satisfies:
        // 1. the mouse position is currently in the window area
        // 2. the window is the top one (active window or show_list windows) under the mouse pointer
        // if no window is found in this position, that is system background area. Add logic to handle those events later
    }
    Ok(())
}

/// return the screen size of current window manager, (width, height)
pub fn get_screen_size() -> Result<(usize, usize), &'static str> {
    let win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    Ok((win.final_fb.width, win.final_fb.height))
}

/// return current absolute position of mouse, (x, y)
pub fn get_cursor() -> Result<(usize, usize), &'static str> {
    let win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    let x = win.mouse.x;
    let y = win.mouse.y;
    Ok((x, y))
}

/// move mouse with delta, this will refresh mouse position
fn move_cursor(x: isize, y: isize) -> Result<(), &'static str> {
    let (ox, oy) = get_cursor()?;
    let mut nx = (ox as isize) + (x as isize);
    let mut ny = (oy as isize) + (y as isize);
    let (screen_width, screen_height) = get_screen_size()?;
    if nx < 0 { nx = 0; }
    if ny < 0 { ny = 0; }
    if nx >= (screen_width as isize) { nx = (screen_width as isize)-1; }
    if ny >= (screen_height as isize) { ny = (screen_height as isize)-1; }
    move_cursor_to(nx as usize, ny as usize)?;
    Ok(())
}

/// move mouse to absolute position
fn move_cursor_to(nx: usize, ny: usize) -> Result<(), &'static str> {
    let (ox, oy) = get_cursor()?;
    let mut win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    win.mouse = Point { x: nx, y: ny };
    // then update region of old mouse
    for x in (ox-MOUSE_POINTER_HALF_SIZE) as isize .. (ox+MOUSE_POINTER_HALF_SIZE+1) as isize {
        for y in (oy-MOUSE_POINTER_HALF_SIZE) as isize .. (oy+MOUSE_POINTER_HALF_SIZE+1) as isize {
            win.refresh_single_pixel(x, y)?;
        }
    }
    // draw new mouse in the new position
    for x in (nx-MOUSE_POINTER_HALF_SIZE) as isize .. (nx+MOUSE_POINTER_HALF_SIZE+1) as isize {
        for y in (ny-MOUSE_POINTER_HALF_SIZE) as isize .. (ny+MOUSE_POINTER_HALF_SIZE+1) as isize {
            win.refresh_single_pixel(x, y)?;
        }
    }
    Ok(())
}

/// new window object with given position and size
pub fn new_window<'a>(
    x: isize, y: isize, width: usize, height: usize,
) -> Result<Arc<Mutex<WindowObjAlpha>>, &'static str> {

    // Init the key input producer and consumer
    let consumer = DFQueue::new().into_consumer();
    let producer = consumer.obtain_producer();

    // Init the frame buffer of the window
    let mut framebuffer = FrameBufferAlpha::new(width, height, None)?;
    framebuffer.fill_color(0x80FFFFFF.into());  // draw with half transparent white

    // new window object
    let window: WindowObjAlpha = WindowObjAlpha {
        x: x,
        y: y,
        width: width,
        height: height,
        consumer: consumer,
        producer: producer,
        framebuffer: framebuffer,
        give_all_mouse_event: false,
        is_moving: false,
        moving_base: (0, 0),  // the point as a base to start moving
    };

    let window_ref = Arc::new(Mutex::new(window));
    let mut win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    win.set_active(&window_ref, false)?;  // do not refresh now for better speed

    Ok(window_ref)
}
