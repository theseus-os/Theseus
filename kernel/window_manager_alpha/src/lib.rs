//! Window manager that simulates a desktop environment with alpha channel.
//! 
//! windows overlapped each other would obey the rules of alpha channel composition
//!
//! Applications request window objects from the window manager through:
//! - new_window(x, y, width, height) provides a new window whose dimensions the caller must specify
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

/// The maximum size of mouse
const MOUSE_MAX_SIZE: usize = 7;
/// Transparent pixel
const T: Pixel = 0xFF000000;
/// Opaque white
const O: Pixel = 0x00FFFFFF;
/// Opaque black
const B: Pixel = 0x000000FF;
/// the mouse picture
const MOUSE_BASIC: [[Pixel; 2*MOUSE_MAX_SIZE+1]; 2*MOUSE_MAX_SIZE+1] = [
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
const WINDOW_BORDER_COLOR_INNER: Pixel = 0x00CA6F1E;
/// border's outter color
const WINDOW_BORDER_COLOR_OUTTER: Pixel = 0xFFFFFFFF;

/// a 2D point
struct Point {
    x: usize,
    y: usize,
}

/// a rectangle region
struct RectRegion {
    /// x start 
    xs: usize,
    /// x end (exclusive)
    xe: usize,
    /// y start
    ys: usize,
    /// y end (exclusive)
    ye: usize,
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
}

/// Window manager object that stores non-critical information
impl WindowManagerAlpha {

    /// set one window to active, push last active (if exists) to top of show_list
    pub fn set_active(&mut self, objref: &Arc<Mutex<WindowObjAlpha>>) -> Result<(), &'static str> {
        let (xs, xe, ys, ye) = {
            let winobj = objref.lock();
            let xs = winobj.x; let ys = winobj.y;
            let xe = xs + winobj.width; let ye = ys + winobj.height;
            (xs, xe, ys, ye)
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
        self.refresh_area(xs, xe, ys, ye)?;
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
        let (xs, xe, ys, ye) = {
            let winobj = objref.lock();
            let xs = winobj.x; let ys = winobj.y;
            let xe = xs + winobj.width; let ye = ys + winobj.height;
            (xs, xe, ys, ye)
        };
        if let Some(current_active) = self.active.upgrade() {
            if Arc::ptr_eq(&(current_active), objref) {
                self.active = Weak::new();  // delete reference
                self.refresh_area(xs, xe, ys, ye)?;
                return Ok(())
            }
        }
        match self.is_window_in_show_list(&objref) {
            Some(i) => {
                self.show_list.remove(i);
                self.refresh_area(xs, xe, ys, ye)?;
                return Ok(())
            }, None => {}
        }
        match self.is_window_in_hide_list(&objref) {
            Some(i) => {
                self.hide_list.remove(i);
                self.refresh_area(xs, xe, ys, ye)?;
                return Ok(())
            }, None => {}
        }
        Err("cannot find this window")
    }

    /// iterately compute single pixel within show_list
    fn recompute_single_pixel_show_list(& self, x: usize, y: usize, idx: usize) -> Pixel {
        if idx >= self.show_list.len() {
            if x < 1280 && y < 1080 {
                return background::BACKGROUND[y/2][x/2];
            }
            return 0x00000000;  // return black
        }
        if let Some(now_winobj) = self.show_list[idx].upgrade() {
            // first get current color, to determine whether further get colors below   
            let top = {
                let winobj = now_winobj.lock();
                let cx = winobj.x;
                let cy = winobj.y;
                let mut ret = T;  // defult is transparent
                if winobj.framebuffer.check_in_buffer(x - cx, y - cy) {
                    let top = match winobj.framebuffer.get_pixel(x - cx, y - cy) {
                        Ok(m) => m,
                        Err(_) => T,  // transparent
                    };
                    if (top >> 24) == 0 {  // totally opaque, so not waste computation
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
            let cx = current_active_win.x;
            let cy = current_active_win.y;
            if current_active_win.framebuffer.check_in_buffer(x - cx, y - cy) {
                let top = current_active_win.framebuffer.get_pixel(x - cx, y - cy)?;
                if (top >> 24) == 0 {  // totally opaque, so not waste computation
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
            let (xs, xe, ys, ye) = {
                let r = &self.border_position;
                (r.xs, r.xe, r.ys, r.ye)
            };
            let x_in = x as isize >= (xs-WINDOW_BORDER_SIZE) as isize && x as isize <= (xe-1+WINDOW_BORDER_SIZE) as isize;
            let y_in = y as isize >= (ys-WINDOW_BORDER_SIZE) as isize && y as isize <= (ye-1+WINDOW_BORDER_SIZE) as isize;
            let left = xs-x <= WINDOW_BORDER_SIZE && y_in;
            let right = x-(xe-1) <= WINDOW_BORDER_SIZE && y_in;
            let top = ys-y <= WINDOW_BORDER_SIZE && x_in;
            let bottom = y-(ye-1) <= WINDOW_BORDER_SIZE && x_in;
            if left {
                if top {  // left-top
                    let dx = xs-x; let dy = ys-y;
                    if dx+dy <= WINDOW_BORDER_SIZE {
                        self.final_fb.draw_point_alpha(x, y, color_mix(
                            WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (dx+dy) as f32 / WINDOW_BORDER_SIZE as f32));
                    }
                } else if bottom {  // left-bottom
                    let dx = xs-x; let dy = y-(ye-1);
                    if dx+dy <= WINDOW_BORDER_SIZE {
                        self.final_fb.draw_point_alpha(x, y, color_mix(
                            WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (dx+dy) as f32 / WINDOW_BORDER_SIZE as f32));
                    }
                } else {  // only left
                    self.final_fb.draw_point_alpha(x, y, color_mix(
                        WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (xs-x) as f32 / WINDOW_BORDER_SIZE as f32));
                }
            } else if right {
                if top {  // right-top
                    let dx = x-(xe-1); let dy = ys-y;
                    if dx+dy <= WINDOW_BORDER_SIZE {
                        self.final_fb.draw_point_alpha(x, y, color_mix(
                            WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (dx+dy) as f32 / WINDOW_BORDER_SIZE as f32));
                    }
                } else if bottom {  // right-bottom
                    let dx = x-(xe-1); let dy = y-(ye-1);
                    if dx+dy <= WINDOW_BORDER_SIZE {
                        self.final_fb.draw_point_alpha(x, y, color_mix(
                            WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (dx+dy) as f32 / WINDOW_BORDER_SIZE as f32));
                    }
                } else {  // only right
                    self.final_fb.draw_point_alpha(x, y, color_mix(
                        WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (x-(xe-1)) as f32 / WINDOW_BORDER_SIZE as f32));
                }
            } else if top {  // only top
                self.final_fb.draw_point_alpha(x, y, color_mix(
                    WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (ys-y) as f32 / WINDOW_BORDER_SIZE as f32));
            } else if bottom {  // only bottom
                self.final_fb.draw_point_alpha(x, y, color_mix(
                    WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (y-(ye-1)) as f32 / WINDOW_BORDER_SIZE as f32));
            }
        }
        // finally draw mouse
        let (cx, cy) = {
            let m = &self.mouse;
            (m.x, m.y)
        };
        if ((x-cx) <= MOUSE_MAX_SIZE || (cx-x) <= MOUSE_MAX_SIZE) && ((y-cy) <= MOUSE_MAX_SIZE || (cy-y) <= MOUSE_MAX_SIZE) {
            self.final_fb.draw_point_alpha(x, y, MOUSE_BASIC[MOUSE_MAX_SIZE + x - cx][MOUSE_MAX_SIZE + y - cy]);
        }
        Ok(())
    }

    /// recompute single pixel value and refresh it on screen
    pub fn refresh_single_pixel(&mut self, x: usize, y: usize) -> Result<(), &'static str> {
        self.refresh_single_pixel_with_buffer(x, y)
    }

    /// refresh an area
    fn refresh_area(&mut self, xs: usize, xe: usize, ys: usize, ye: usize) -> Result<(), &'static str> {
        for x in xs as isize .. xe as isize {
            for y in ys as isize .. ye as isize {
                self.refresh_single_pixel_with_buffer(x as usize, y as usize)?;
            }
        }
        Ok(())
    }

    /// refresh an rectangle border
    fn refresh_rect_border(&mut self, xs: usize, xe: usize, ys: usize, ye: usize) -> Result<(), &'static str> {
        for x in xs as isize .. xe as isize {
            self.refresh_single_pixel_with_buffer(x as usize, ys)?;
            self.refresh_single_pixel_with_buffer(x as usize, ye-1)?;
        }
        for y in ys as isize .. ye as isize {
            self.refresh_single_pixel_with_buffer(xs, y as usize)?;
            self.refresh_single_pixel_with_buffer(xe-1, y as usize)?;
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
            (m.x, m.y)
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
            if current_active_win.framebuffer.check_in_buffer(x - cx, y - cy) {
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
                if now_winobj.framebuffer.check_in_buffer(x - cx, y - cy) {
                    event.x = x - cx;
                    event.y = y - cy;
                    now_winobj.producer.enqueue(Event::MousePositionEvent(event));
                    return Ok(());
                }
            }
        }
        Err("cannot find window to pass")
    }

    /// refresh the floating border indicating user of new window position and size
    fn refresh_floating_border(& mut self, show: bool, xs: usize, xe: usize, ys: usize, ye: usize) -> Result<(), &'static str> {
        if !self.is_show_border && !show { return Ok(()) }
        // first clear old border
        let last_show_border = self.is_show_border;
        self.is_show_border = show;
        if last_show_border {
            let (oxs, oxe, oys, oye) = {
                let r = &self.border_position;
                (r.xs, r.xe, r.ys, r.ye)
            };
            if show {  // otherwise don't change position
                self.border_position = RectRegion { xs, xe, ys, ye };
            }
            for i in 0..WINDOW_BORDER_SIZE+1 {
                self.refresh_rect_border(oxs-i, oxe+i, oys-i, oye+i)?;
            }
        }
        // then draw current border
        if show {
            self.border_position = RectRegion { xs, xe, ys, ye };
            for i in 0..WINDOW_BORDER_SIZE+1 {
                self.refresh_rect_border(xs-i, xe+i, ys-i, ye+i)?;
            }
        }
        Ok(())
    }

    /// optimized refresh area for less computation up to 2x
    fn refresh_area_with_old_new(&mut self, 
            uoxs: usize, uoxe: usize, uoys: usize, uoye: usize, 
            unxs: usize, unxe: usize, unys: usize, unye: usize) -> Result<(), &'static str> {
        let oxs = uoxs as isize; let oxe = uoxe as isize; let oys = uoys as isize; let oye = uoye as isize;
        let nxs = unxs as isize; let nxe = unxe as isize; let nys = unys as isize; let nye = unye as isize;
        // first refresh new area for better user experience
        self.refresh_area(unxs, unxe, unys, unye)?;
        // then refresh old area that not overlapped by new one
        if nxs >= oxe || nxe <= oxs || nys >= oye || nye <= oys {  // have to refresh all
            self.refresh_area(uoxs, uoxe, uoys, uoye)?;
        } else {  // there existes overlapped region, optimize them!
            let mut ucxs = uoxs; let mut ucxe = uoxe;
            if oxs < nxs { self.refresh_area(uoxs, unxs, uoys, uoye)?; ucxs = unxs; }
            if oxe > nxe { self.refresh_area(unxe, uoxe, uoys, uoye)?; ucxe = unxe; }
            if oys < nys { self.refresh_area(ucxs, ucxe, uoys, unys)?; }
            if oye > nye { self.refresh_area(ucxs, ucxe, unye, uoye)?; }
        }
        Ok(())
    }

    /// private function: take active window's base position and current mouse, move the window with delta
    fn move_active_window(&mut self) -> Result<(), &'static str> {
        if let Some(current_active) = self.active.upgrade() {
            let (oxs, oxe, oys, oye, nxs, nxe, nys, nye) = {
                let mut current_active_win = current_active.lock();
                let (cx, cy) = {
                    let m = &self.mouse;
                    (m.x, m.y)
                };
                let (bx, by) = current_active_win.moving_base;
                let ox = current_active_win.x;
                let oy = current_active_win.y;
                let nx = ox + cx - bx;
                let ny = oy + cy - by;
                let width = current_active_win.width;
                let height = current_active_win.height;
                current_active_win.x = nx;
                current_active_win.y = ny;
                (ox, ox + width, oy, oy + height, nx, nx + width, ny, ny + height)
            };
            // then try to reduce time on refresh old ones
            self.refresh_area_with_old_new(oxs, oxe, oys, oye, nxs, nxe, nys, nye)?;
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
    win.set_active(objref)
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

/// refresh the floating border display, will lock WINDOW_MANAGER
pub fn do_refresh_floating_border() -> Result<(), &'static str> {
    let mut win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    let (nx, ny) = {
        let m = &win.mouse;
        (m.x, m.y)
    };
    if let Some(current_active) = win.active.upgrade() {
        let (is_draw, bxs, bxe, bys, bye) = {
            let current_active_win = current_active.lock();
            if current_active_win.is_moving {  // move this window
                // for better performance, while moving window, only border is shown for indication
                let cx = current_active_win.x;
                let cy = current_active_win.y;
                let (bx, by) = current_active_win.moving_base;
                let width = current_active_win.width;
                let height = current_active_win.height;
                let bxs = cx + nx - bx;
                let bxe = bxs + width;
                let bys = cy + ny - by;
                let bye = bys + height;
                // debug!("drawing border: {:?}", (bxs, bxe, bys, bye));
                (true, bxs, bxe, bys, bye)
            } else {
                (false, 0, 0, 0, 0)
            }
        };
        win.refresh_floating_border(is_draw, bxs, bxe, bys, bye)?;  // refresh current border position
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
pub fn refresh_pixel_absolute(x: usize, y: usize) -> Result<(), &'static str> {
    let mut win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    win.refresh_single_pixel(x, y)
}

/// refresh an area using abosolute position, will lock WINDOW_MANAGER
pub fn refresh_area_absolute(xs: usize, xe: usize, ys: usize, ye: usize) -> Result<(), &'static str> {
    let mut win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    win.refresh_area(xs, xe, ys, ye)
}

/// Initialize the window manager, should provide the consumer of keyboard and mouse event, as well as a frame buffer to draw
pub fn init(key_consumer: DFQueueConsumer<Event>, mouse_consumer: DFQueueConsumer<Event>, 
        final_fb: FrameBufferAlpha) -> Result<(), &'static str> {
    debug!("window manager alpha init called");

    // initialize static window manager
    let window_manager = WindowManagerAlpha {
            hide_list: VecDeque::new(),
            show_list: VecDeque::new(),
            active: Weak::new(),
            mouse: Point { x: 0, y: 0 },
            is_show_border: false,
            border_position: RectRegion { xs: 0, xe: 0, ys: 0, ye: 0 },
            terminal_id_counter: 1,
            final_fb: final_fb,
    };
    WINDOW_MANAGER.call_once(|| Mutex::new(window_manager));

    let mut win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    let screen_width = win.final_fb.width;
    let screen_height = win.final_fb.height;
    win.mouse = Point { x: screen_width/2, y: screen_height/2 };  // set mouse to middle
    win.refresh_area(0, screen_width, 0, screen_height)?;

    KernelTaskBuilder::new(window_manager_loop, (key_consumer, mouse_consumer) )
        .name("window_manager_loop".to_string())
        .spawn()?;

    Ok(())
}

/// Window object that should be owned by application
pub struct WindowObjAlpha {
    /// absolute position of this window
    pub x: usize,
    /// absolute position of this window
    pub y: usize,
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
    pub moving_base: (usize, usize),
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
        // even not find a window to pass, that's ok
    }
    Ok(())
}

/// handle mouse event, push it to related window or anyone asked for it
fn cursor_handle_application(mouse_event: MouseEvent) -> Result<(), &'static str> {
    do_refresh_floating_border()?;
    let win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    if let Err(_) = win.pass_mouse_event_to_window(mouse_event) {
        // even not find a window to pass, that's ok
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
pub fn move_cursor(x: isize, y: isize) -> Result<(), &'static str> {
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
pub fn move_cursor_to(nx: usize, ny: usize) -> Result<(), &'static str> {
    let (ox, oy) = get_cursor()?;
    let mut win = WINDOW_MANAGER.try().ok_or("The static window manager was not yet initialized")?.lock();
    win.mouse = Point { x: nx, y: ny };
    // then update region of old mouse
    for x in (ox-MOUSE_MAX_SIZE) as isize .. (ox+MOUSE_MAX_SIZE+1) as isize {
        for y in (oy-MOUSE_MAX_SIZE) as isize .. (oy+MOUSE_MAX_SIZE+1) as isize {
            win.refresh_single_pixel(x as usize, y as usize)?;
        }
    }
    // draw new mouse in the new position
    for x in (nx-MOUSE_MAX_SIZE) as isize .. (nx+MOUSE_MAX_SIZE+1) as isize {
        for y in (ny-MOUSE_MAX_SIZE) as isize .. (ny+MOUSE_MAX_SIZE+1) as isize {
            win.refresh_single_pixel(x as usize, y as usize)?;
        }
    }
    Ok(())
}

/// new window object with given position and size
pub fn new_window<'a>(
    x: usize, y: usize, width: usize, height: usize,
) -> Result<Arc<Mutex<WindowObjAlpha>>, &'static str> {

    // Init the key input producer and consumer
    let consumer = DFQueue::new().into_consumer();
    let producer = consumer.obtain_producer();

    // Init the frame buffer of the window
    let mut framebuffer = FrameBufferAlpha::new(width, height, None)?;
    framebuffer.fill_color(0x80FFFFFF);  // draw with half transparent white

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
    win.set_active(&window_ref)?;
    // win.refresh_window(&window_ref)?;  // do not refresh now for better speed

    Ok(window_ref)
}
