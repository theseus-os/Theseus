//! Window manager that simulates a desktop environment with alpha channel.
//! windows overlapped each other would obey the rules of alpha channel composition
//!
//! Applications request window objects from the window manager through:
//! - new_window(x, y, width, height) provides a new window whose dimensions the caller must specify
//!
//! **TODO**: Windows can be resized by calling resize().
//! **TODO**: Window can be deleted when it is dropped or by calling WindowObj.delete().
//! There are three types of window: `active`, `show_list` and `hide_list`
//! - `active` window is the only one active, who gets all keyboard event
//! - `show_list` windows are shown by their up-down relationships, with overlapped part using alpha channel composition
//! - `hide_list` windows are those not current shown but will be invoked to show later
//! Window object holder could set itself to active, pushing the last active window (if exists) to the top of show_list
//!

#![no_std]

extern crate spin;
extern crate alloc;
extern crate dfqueue;
extern crate event_types;
#[macro_use]
extern crate log;
extern crate frame_buffer_alpha;
#[macro_use]
extern crate lazy_static;
extern crate spawn;
extern crate path;
extern crate mouse_data;
extern crate keycodes_ascii;

mod background;
use path::Path;
use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::ops::Deref;
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use event_types::{Event, MousePositionEvent};
use frame_buffer_alpha::{ FrameBufferAlpha, Pixel, FINAL_FRAME_BUFFER, alpha_mix, color_mix };
use spin::{Mutex};
use spawn::{KernelTaskBuilder, ApplicationTaskBuilder};
use mouse_data::MouseEvent;
use keycodes_ascii::{KeyEvent};

lazy_static! {
    /// The list of all windows in the system.
    static ref WINDOW_MANAGER: Mutex<WindowManagerAlpha> = Mutex::new(
        WindowManagerAlpha {
            hide_list: VecDeque::new(),
            show_list: VecDeque::new(),
            active: Weak::new(),
            cursor: (0, 0),
            is_show_border: false,
            border_position: (0, 0, 0, 0),
        }
    );
}

// The maximum size of cursor
const CURSOR_MAX_SIZE: usize = 7;
const T: Pixel = 0xFF000000;  // transparent
const O: Pixel = 0x00FFFFFF;  // opaque white
const B: Pixel = 0x000000FF;  // opaque black
const CURSOR_BASIC: [[Pixel; 2*CURSOR_MAX_SIZE+1]; 2*CURSOR_MAX_SIZE+1] = [
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

/// indicating new window position and size
const WINDOW_BORDER_SIZE: usize = 3;
const WINDOW_BORDER_COLOR_INNER: Pixel = 0x00CA6F1E;
const WINDOW_BORDER_COLOR_OUTTER: Pixel = 0xFFFFFFFF;

struct WindowManagerAlpha {
    hide_list: VecDeque<Weak<Mutex<WindowObjAlpha>>>,
    show_list: VecDeque<Weak<Mutex<WindowObjAlpha>>>,
    active: Weak<Mutex<WindowObjAlpha>>,  // this one is not in show_list
    cursor: (usize, usize),  // store the current cursor position
    is_show_border: bool,
    border_position: (usize, usize, usize, usize),  // xs, xe, ys, ye, could be minus value
}

/// Window manager object that stores non-critical information
impl WindowManagerAlpha {
    /// push current active to show_list
    pub fn clear_active(&mut self) {
        if let Some(_) = self.active.upgrade() {
            self.show_list.push_front(self.active.clone());
            self.active = Weak::new();
        }
    }

    /// set one window to active, push last active (if exists) to top of show_list
    pub fn set_active(&mut self, objref: &Arc<Mutex<WindowObjAlpha>>) {
        // if it is currently actived, just return
        if let Some(current_active) = self.active.upgrade() {
            if Arc::ptr_eq(&(current_active), objref) {
                return;  // do nothing
            } else {  // save this to show_list
                self.show_list.push_front(self.active.clone());
                self.active = Weak::new();
            }
        }
        self.delete_window(objref);  // remove item in current list
        self.active = Arc::downgrade(objref);
    }

    /// delete one window
    pub fn delete_window(&mut self, objref: &Arc<Mutex<WindowObjAlpha>>) {
        if let Some(current_active) = self.active.upgrade() {
            if Arc::ptr_eq(&(current_active), objref) {
                self.active = Weak::new();  // delete reference
            }
        }
        let mut i = 0_usize;
        for item in self.show_list.iter() {
            if let Some(item_ptr) = item.upgrade() {
                if Arc::ptr_eq(&(item_ptr), objref) {
                    break;
                }
            }
            i += 1;
        }
        if i < self.show_list.len() {
            self.show_list.remove(i);
        }
        let mut i = 0_usize;
        for item in self.hide_list.iter() {
            if let Some(item_ptr) = item.upgrade() {
                if Arc::ptr_eq(&(item_ptr), objref) {
                    break;
                }
            }
            i += 1;
        }
        if i < self.hide_list.len() {
            self.hide_list.remove(i);
        }
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
            let winobj = now_winobj.lock();
            let cx = winobj.x;
            let cy = winobj.y;
            if winobj.framebuffer.check_in_buffer(x - cx, y - cy) {
                let top = match winobj.framebuffer.get_pixel(x - cx, y - cy) {
                    Ok(m) => m,
                    Err(_) => { return self.recompute_single_pixel_show_list(x, y, idx+1); }  // this window as transparent
                };
                if (top >> 24) == 0 {  // totally opaque, so not waste computation
                    return top;
                } else {
                    let bottom = self.recompute_single_pixel_show_list(x, y, idx+1);
                    return alpha_mix(bottom, top);
                }
            } else {
                drop(winobj);
                return self.recompute_single_pixel_show_list(x, y, idx+1);
            }
        } else {  // need to delete this one, since the owner has been dropped, but here is immutable >.<
            // self.show_list.remove(idx);
            return self.recompute_single_pixel_show_list(x, y, idx+1);
        }
    }

    /// refresh one pixel on frame buffer
    fn refresh_single_pixel_with_buffer(& self, x: usize, y: usize, final_fb: &mut FrameBufferAlpha) -> Result<(), &'static str> {
        if ! final_fb.check_in_buffer(x, y) {
            return Ok(());
        }
        if let Some(current_active) = self.active.upgrade() {
            let current_active_win = current_active.lock();
            let cx = current_active_win.x;
            let cy = current_active_win.y;
            if current_active_win.framebuffer.check_in_buffer(x - cx, y - cy) {
                let top = current_active_win.framebuffer.get_pixel(x - cx, y - cy)?;
                if (top >> 24) == 0 {  // totally opaque, so not waste computation
                    final_fb.draw_point(x, y, top);
                } else {
                    let bottom = self.recompute_single_pixel_show_list(x, y, 0);
                    final_fb.draw_point(x, y, alpha_mix(bottom, top));
                }
            } else {
                let pixel = self.recompute_single_pixel_show_list(x, y, 0);
                final_fb.draw_point(x, y, pixel);
            }
        } else {  // nothing is active now
            let pixel = self.recompute_single_pixel_show_list(x, y, 0);
            final_fb.draw_point(x, y, pixel);
        }
        // then draw border
        if self.is_show_border {
            let (xs, xe, ys, ye) = self.border_position;
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
                        final_fb.draw_point_alpha(x, y, color_mix(
                            WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (dx+dy) as f32 / WINDOW_BORDER_SIZE as f32));
                    }
                } else if bottom {  // left-bottom
                    let dx = xs-x; let dy = y-(ye-1);
                    if dx+dy <= WINDOW_BORDER_SIZE {
                        final_fb.draw_point_alpha(x, y, color_mix(
                            WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (dx+dy) as f32 / WINDOW_BORDER_SIZE as f32));
                    }
                } else {  // only left
                    final_fb.draw_point_alpha(x, y, color_mix(
                        WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (xs-x) as f32 / WINDOW_BORDER_SIZE as f32));
                }
            } else if right {
                if top {  // right-top
                    let dx = x-(xe-1); let dy = ys-y;
                    if dx+dy <= WINDOW_BORDER_SIZE {
                        final_fb.draw_point_alpha(x, y, color_mix(
                            WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (dx+dy) as f32 / WINDOW_BORDER_SIZE as f32));
                    }
                } else if bottom {  // right-bottom
                    let dx = x-(xe-1); let dy = y-(ye-1);
                    if dx+dy <= WINDOW_BORDER_SIZE {
                        final_fb.draw_point_alpha(x, y, color_mix(
                            WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (dx+dy) as f32 / WINDOW_BORDER_SIZE as f32));
                    }
                } else {  // only right
                    final_fb.draw_point_alpha(x, y, color_mix(
                        WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (x-(xe-1)) as f32 / WINDOW_BORDER_SIZE as f32));
                }
            } else if top {  // only top
                final_fb.draw_point_alpha(x, y, color_mix(
                    WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (ys-y) as f32 / WINDOW_BORDER_SIZE as f32));
            } else if bottom {  // only bottom
                final_fb.draw_point_alpha(x, y, color_mix(
                    WINDOW_BORDER_COLOR_OUTTER, WINDOW_BORDER_COLOR_INNER, (y-(ye-1)) as f32 / WINDOW_BORDER_SIZE as f32));
            }
        }
        // finally draw cursor
        let (cx, cy) = self.cursor;
        if ((x-cx) <= CURSOR_MAX_SIZE || (cx-x) <= CURSOR_MAX_SIZE) && ((y-cy) <= CURSOR_MAX_SIZE || (cy-y) <= CURSOR_MAX_SIZE) {
            final_fb.draw_point_alpha(x, y, CURSOR_BASIC[CURSOR_MAX_SIZE + x - cx][CURSOR_MAX_SIZE + y - cy]);
        }
        Ok(())
    }

    /// recompute single pixel value and refresh it on screen
    pub fn refresh_single_pixel(& self, x: usize, y: usize) -> Result<(), &'static str> {
        let mut final_fb = FINAL_FRAME_BUFFER
            .try() .ok_or("The final frame buffer was not yet initialized")?.lock();
        self.refresh_single_pixel_with_buffer(x, y, &mut final_fb)
    }

    /// refresh an area
    fn refresh_area(& self, xs: usize, xe: usize, ys: usize, ye: usize) -> Result<(), &'static str> {
        let mut final_fb = FINAL_FRAME_BUFFER
            .try() .ok_or("The final frame buffer was not yet initialized")?.lock();
        for x in xs as isize .. xe as isize {
            for y in ys as isize .. ye as isize {
                self.refresh_single_pixel_with_buffer(x as usize, y as usize, &mut final_fb)?;
            }
        }
        Ok(())
    }

    /// refresh an rectangle border
    fn refresh_rect_border(& self, xs: usize, xe: usize, ys: usize, ye: usize) -> Result<(), &'static str> {
        let mut final_fb = FINAL_FRAME_BUFFER
            .try() .ok_or("The final frame buffer was not yet initialized")?.lock();
        for x in xs as isize .. xe as isize {
            self.refresh_single_pixel_with_buffer(x as usize, ys, &mut final_fb)?;
            self.refresh_single_pixel_with_buffer(x as usize, ye-1, &mut final_fb)?;
        }
        for y in ys as isize .. ye as isize {
            self.refresh_single_pixel_with_buffer(xs, y as usize, &mut final_fb)?;
            self.refresh_single_pixel_with_buffer(xe-1, y as usize, &mut final_fb)?;
        }
        Ok(())
    }

    /// refresh whole window, not recommended for small changes of window
    fn refresh_window(& self, objref: &Arc<Mutex<WindowObjAlpha>>) -> Result<(), &'static str> {
        let winobj = objref.lock();
        let xs = winobj.x;
        let xe = xs + winobj.width;
        let ys = winobj.y;
        let ye = ys + winobj.height;
        // debug!("drawing {:?}", (xs, xe, ys, ye));
        drop(winobj);
        self.refresh_area(xs, xe, ys, ye)
    }

    /// pass keyboard event to currently active window
    fn pass_keyboard_event_to_window(& self, key_event: KeyEvent) -> Result<(), &'static str> {
        if let Some(current_active) = self.active.upgrade() {
            let current_active_win = current_active.lock();
            current_active_win.producer.enqueue(Event::new_input_event(key_event));
        }
        Err("cannot find window to pass key event")
    }

    /// pass mouse event to the top window that cursor on, may also transfer to those want all events
    fn pass_mouse_event_to_window(& self, mouse_event: MouseEvent) -> Result<(), &'static str> {
        let (x, y) = self.cursor;
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
            if let Some(_now_winobj) = self.show_list[i].upgrade() {
                let now_winobj = _now_winobj.lock();
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
        }
        // then check show_list
        for i in 0..self.show_list.len() {
            if let Some(_now_winobj) = self.show_list[i].upgrade() {
                let now_winobj = _now_winobj.lock();
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
            let (oxs, oxe, oys, oye) = self.border_position;
            if show {  // otherwise don't change position
                self.border_position = (xs, xe, ys, ye);
            }
            for i in 0..WINDOW_BORDER_SIZE+1 {
                self.refresh_rect_border(oxs-i, oxe+i, oys-i, oye+i)?;
            }
        }
        // then draw current border
        if show {
            self.border_position = (xs, xe, ys, ye);
            for i in 0..WINDOW_BORDER_SIZE+1 {
                self.refresh_rect_border(xs-i, xe+i, ys-i, ye+i)?;
            }
        }
        Ok(())
    }

    /// optimized refresh area for less computation up to 2x
    fn refresh_area_with_old_new(& self, 
            _oxs: usize, _oxe: usize, _oys: usize, _oye: usize, 
            _nxs: usize, _nxe: usize, _nys: usize, _nye: usize) -> Result<(), &'static str> {
        let oxs = _oxs as isize; let oxe = _oxe as isize; let oys = _oys as isize; let oye = _oye as isize;
        let nxs = _nxs as isize; let nxe = _nxe as isize; let nys = _nys as isize; let nye = _nye as isize;
        // first refresh new area for better user experience
        self.refresh_area(_nxs, _nxe, _nys, _nye)?;
        // then refresh old area that not overlapped by new one
        if nxs >= oxe || nxe <= oxs || nys >= oye || nye <= oys {  // have to refresh all
            self.refresh_area(_oxs, _oxe, _oys, _oye)?;
        } else {  // there existes overlapped region, optimize them!
            let mut _cxs = _oxs; let mut _cxe = _oxe;
            if oxs < nxs { self.refresh_area(_oxs, _nxs, _oys, _oye)?; _cxs = _nxs; }
            if oxe > nxe { self.refresh_area(_nxe, _oxe, _oys, _oye)?; _cxe = _nxe; }
            if oys < nys { self.refresh_area(_cxs, _cxe, _oys, _nys)?; }
            if oye > nye { self.refresh_area(_cxs, _cxe, _nye, _oye)?; }
        }
        Ok(())
    }

    /// private function: take active window's base position and current cursor, move the window with delta
    fn move_active_window(& self) -> Result<(), &'static str> {
        if let Some(current_active) = self.active.upgrade() {
            let mut current_active_win = current_active.lock();
            let (cx, cy) = self.cursor;
            let (bx, by) = current_active_win.moving_base;
            let ox = current_active_win.x;
            let oy = current_active_win.y;
            let nx = ox + cx - bx;
            let ny = oy + cy - by;
            let width = current_active_win.width;
            let height = current_active_win.height;
            current_active_win.x = nx;
            current_active_win.y = ny;
            drop(current_active_win);
            // then try to reduce time on refresh old ones
            self.refresh_area_with_old_new(ox, ox + width, oy, oy + height, nx, nx + width, ny, ny + height)?;
        } else {
            return Err("cannot fid active window to move");
        }
        Ok(())
    }
}

/// refresh the floating border display, will lock WINDOW_MANAGER
pub fn do_refresh_floating_border() -> Result<(), &'static str> {
    let mut win = WINDOW_MANAGER.lock();
    let (nx, ny) = win.cursor;
    if let Some(current_active) = win.active.upgrade() {
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
            drop(current_active_win);
            win.refresh_floating_border(true, bxs, bxe, bys, bye)?;  // refresh current border position
        } else {
            drop(current_active_win);
            win.refresh_floating_border(false, 0, 0, 0, 0)?;  // hide border
        }
    } else {
        win.refresh_floating_border(false, 0, 0, 0, 0)?;  // hide border
    }
    Ok(())
}

/// execute moving active window action, will lock WINDOW_MANAGER
pub fn do_move_active_window() -> Result<(), &'static str> {
    let win = WINDOW_MANAGER.lock();
    win.move_active_window()
}

/// refresh one pixel using absolute position, will lock WINDOW_MANAGER
pub fn refresh_pixel_absolute(x: usize, y: usize) -> Result<(), &'static str> {
    let win = WINDOW_MANAGER.lock();
    win.refresh_single_pixel(x, y)
}

/// refresh an area using abosolute position, will lock WINDOW_MANAGER
pub fn refresh_area_absolute(xs: usize, xe: usize, ys: usize, ye: usize) -> Result<(), &'static str> {
    let win = WINDOW_MANAGER.lock();
    win.refresh_area(xs, xe, ys, ye)
}

/// Initialize the window manager
pub fn init(key_comsumer: DFQueueConsumer<Event>, mouse_comsumer: DFQueueConsumer<Event>) -> Result<(), &'static str> {
    let (screen_width, screen_height) = frame_buffer_alpha::get_screen_size()?;
    debug!("window manager alpha init called");

    WINDOW_MANAGER.lock().cursor = (screen_width/2, screen_height/2);  // set cursor to middle

    WINDOW_MANAGER.lock().refresh_area(0, screen_width, 0, screen_height)?;

    KernelTaskBuilder::new(window_manager_loop, (key_comsumer, mouse_comsumer) )
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
    /// the base position of window moving action, should be the cursor position when `is_moving` is set to true
    pub moving_base: (usize, usize),
}

// delete the reference of a window in the manager when the window is dropped
impl Drop for WindowObjAlpha {
    fn drop(&mut self) {

        debug!("WindowObjAlpha drop called");

        // let mut window_list = WINDOWLIST.lock();

        // // Switches to a new active window and sets
        // // the active pointer field of the window allocator to the new active window
        // match window_list.delete(&self.inner) {
        //     Ok(_) => {}
        //     Err(err) => error!("Fail to schedule to the next window: {}", err),
        // };
    }
}

/// handles all keyboard and mouse movement in this window manager
fn window_manager_loop( consumer: (DFQueueConsumer<Event>, DFQueueConsumer<Event>) ) -> Result<(), &'static str> {
    let (key_consumer, mouse_consumer) = consumer;
    
    loop {

        let _event = match key_consumer.peek() {
            Some(ev) => ev,
            _ => { match mouse_consumer.peek() {
                Some(ev) => ev,
                _ => { continue; }
            } }
        };
        let event: Event = _event.clone();
        _event.mark_completed();
        drop(_event);

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
                // need to combine cursor events if there pending a lot
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

fn keyboard_handle_application(key_input: KeyEvent) -> Result<(), &'static str> {
    let win = WINDOW_MANAGER.lock();
    match win.pass_keyboard_event_to_window(key_input) {  // even not find a window to pass, that's ok
        Ok(_) => { },
        Err(_) => { },
    }
    Ok(())
}

fn cursor_handle_application(mouse_event: MouseEvent) -> Result<(), &'static str> {
    do_refresh_floating_border()?;
    let win = WINDOW_MANAGER.lock();
    match win.pass_mouse_event_to_window(mouse_event) {  // even not find a window to pass, that's ok
        Ok(_) => { },
        Err(_) => { },
    }
    Ok(())
}

/// return the screen size of current window manager
pub fn get_screen_size() -> Result<(usize, usize), &'static str> {
    frame_buffer_alpha::get_screen_size()
}

/// return current absolute position of cursor
pub fn get_cursor() -> (usize, usize) {
    let win = WINDOW_MANAGER.lock();
    win.cursor
}

/// move mouse with delta, this will refresh cursor position
pub fn move_cursor(x: isize, y: isize) -> Result<(), &'static str> {
    let (ox, oy) = get_cursor();
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
    let (ox, oy) = get_cursor();
    let mut win = WINDOW_MANAGER.lock();
    win.cursor = (nx, ny);
    // then update region of old mouse
    for x in (ox-CURSOR_MAX_SIZE) as isize .. (ox+CURSOR_MAX_SIZE+1) as isize {
        for y in (oy-CURSOR_MAX_SIZE) as isize .. (oy+CURSOR_MAX_SIZE+1) as isize {
            win.refresh_single_pixel(x as usize, y as usize)?;
        }
    }
    // draw new mouse in the new position
    for x in (nx-CURSOR_MAX_SIZE) as isize .. (nx+CURSOR_MAX_SIZE+1) as isize {
        for y in (ny-CURSOR_MAX_SIZE) as isize .. (ny+CURSOR_MAX_SIZE+1) as isize {
            win.refresh_single_pixel(x as usize, y as usize)?;
        }
    }
    Ok(())
}

/// new window object
pub fn new_window<'a>(
    x: usize, y: usize, width: usize, height: usize,
) -> Result<Arc<Mutex<WindowObjAlpha>>, &'static str> {

    // Init the key input producer and consumer
    let consumer = DFQueue::new().into_consumer();
    let producer = consumer.obtain_producer();

    // Init the frame buffer of the window
    let mut framebuffer = FrameBufferAlpha::new(width, height, None)?;
    framebuffer.fullfill_color(0x80FFFFFF);  // draw with half transparent white

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
    let mut win = WINDOW_MANAGER.lock();
    win.set_active(&window_ref);
    win.refresh_window(&window_ref)?;  // do not refresh now for better speed

    Ok(window_ref)
}

fn test_create_window(x: &'static str, y: &'static str, width: &'static str, height: &'static str) -> Result<(), &'static str> {
    let mut arg1 : Vec<String> = Vec::new();
    arg1.push(String::from("new_window"));
    arg1.push(String::from(x));
    arg1.push(String::from(y));
    arg1.push(String::from(width));
    arg1.push(String::from(height));
    ApplicationTaskBuilder::new(Path::new(String::from("new_window"))).argument(
        arg1
    ).spawn()?;
    Ok(())
}

/// testcase of window manager
pub fn test() -> Result<(), &'static str> {
    // let mut vec = Vec::new(["100"]);
    test_create_window("100", "100", "200", "200")?;
    test_create_window("400", "400", "400", "400")?;
    test_create_window("400", "100", "100", "200")?;
    test_create_window("350", "200", "200", "400")?;
    Ok(())
}
