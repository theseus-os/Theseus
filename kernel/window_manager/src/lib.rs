//! This crate acts as a manager of a list of windows. It defines a `WindowManager` structure and an instance of it. 
//!
//! A window manager holds a set of `WindowView` objects, including an active window, a list of shown windows and a list of hidden windows. The hidden windows are totally overlapped by others.
//!
//! A window manager owns a bottom framebuffer and a top framebuffer. The bottom is the background of the desktop and the top framebuffer contains a floating window border and a mouse arrow. In refreshing an area, the manager will render all the framebuffers in order: bottom -> hide list -> showlist -> active -> top.
//!
//! The window manager provides methods to update a rectangle area or several pixels for better performance.

#![no_std]

extern crate spin;
#[macro_use]
extern crate alloc;
extern crate mpmc;
extern crate event_types;
extern crate compositor;
extern crate frame_buffer;
extern crate frame_buffer_compositor;
extern crate frame_buffer_drawer;
extern crate keycodes_ascii;
extern crate mod_mgmt;
extern crate mouse_data;
extern crate path;
extern crate scheduler; 
extern crate spawn;
extern crate window_view;

mod background;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::sync::{Arc, Weak};
use alloc::vec::{Vec, IntoIter};
use compositor::{Compositor, FrameBufferBlocks};
use core::ops::{Deref, DerefMut};
use mpmc::Queue;
use event_types::{Event, MousePositionEvent};
use frame_buffer::{Coord, FrameBuffer, Pixel, Rectangle};
use frame_buffer_compositor::{FRAME_COMPOSITOR};
use keycodes_ascii::{KeyAction, KeyEvent, Keycode};
use mouse_data::MouseEvent;
use path::Path;
use spawn::{ApplicationTaskBuilder, KernelTaskBuilder};
use spin::{Mutex, Once};
use window_view::WindowView;

/// The instance of the default window manager
pub static WINDOW_MANAGER: Once<Mutex<WindowManager>> = Once::new();

// The half size of mouse in number of pixels, the actual size of pointer is 1+2*`MOUSE_POINTER_HALF_SIZE`
const MOUSE_POINTER_HALF_SIZE: usize = 7;
// Transparent pixel
const T: Pixel = 0xFF000000;
// Opaque white
const O: Pixel = 0x00FFFFFF;
// Opaque blue
const B: Pixel = 0x00000FF;
// the mouse picture
static MOUSE_BASIC: [[Pixel; 2 * MOUSE_POINTER_HALF_SIZE + 1];
    2 * MOUSE_POINTER_HALF_SIZE + 1] = [
    [T, T, T, T, T, T, T, T, T, T, T, T, T, T, T],
    [T, T, T, T, T, T, T, T, T, T, T, T, T, T, T],
    [T, T, T, T, T, T, T, T, T, T, T, T, T, T, T],
    [T, T, T, T, T, T, T, T, T, T, T, T, T, T, T],
    [T, T, T, T, T, T, T, T, T, T, T, T, T, T, T],
    [T, T, T, T, T, T, T, T, T, T, T, T, T, T, T],
    [T, T, T, T, T, T, B, B, B, B, B, B, B, B, B],
    [T, T, T, T, T, T, B, O, O, O, O, O, O, B, T],
    [T, T, T, T, T, T, B, O, O, O, O, O, B, T, T],
    [T, T, T, T, T, T, B, O, O, O, O, B, T, T, T],
    [T, T, T, T, T, T, B, O, O, O, O, B, T, T, T],
    [T, T, T, T, T, T, B, O, O, B, B, O, B, T, T],
    [T, T, T, T, T, T, B, O, B, T, T, B, O, B, T],
    [T, T, T, T, T, T, B, B, T, T, T, T, B, O, B],
    [T, T, T, T, T, T, B, T, T, T, T, T, T, B, B],
];

// the border indicating new window position and size
const WINDOW_BORDER_SIZE: usize = 3;
// border's inner color
const WINDOW_BORDER_COLOR_INNER: Pixel = 0x00CA6F1E;

/// Window manager structure which maintains a list of windows and a mouse.
pub struct WindowManager {
    /// those window currently not shown on screen
    hide_list: VecDeque<Weak<Mutex<WindowView>>>,
    /// those window shown on screen that may overlapping each other
    show_list: VecDeque<Weak<Mutex<WindowView>>>,
    /// the only active window, receiving all keyboard events (except for those remained for WM)
    active: Weak<Mutex<WindowView>>, // this one is not in show_list
    /// current mouse position
    mouse: Coord,
    /// If a window is being repositioned (e.g., by dragging it), this is the position of that window's border
    repositioned_border: Option<Rectangle>,
    /// the frame buffer that it should print on
    bottom_fb: Box<dyn FrameBuffer + Send>,
    /// the frame buffer that it should print on
    top_fb: Box<dyn FrameBuffer + Send>,
}

impl WindowManager {
    /// set one window to active, push last active (if exists) to top of show_list. if `refresh` is `true`, will then refresh the window's area
    pub fn set_active(
        &mut self,
        objref: &Arc<Mutex<WindowView>>,
        refresh: bool,
    ) -> Result<(), &'static str> {
        // if it is currently actived, just return
        if let Some(current_active) = self.active.upgrade() {
            if Arc::ptr_eq(&(current_active), objref) {
                return Ok(()); // do nothing
            } else {
                // save this to show_list
                self.show_list.push_front(self.active.clone());
                self.active = Weak::new();
            }
        }
        match self.is_window_in_show_list(&objref) {
            // remove item in current list
            Some(i) => {
                self.show_list.remove(i);
            }
            None => {}
        }
        match self.is_window_in_hide_list(&objref) {
            // remove item in current list
            Some(i) => {
                self.hide_list.remove(i);
            }
            None => {}
        }
        self.active = Arc::downgrade(objref);
        let area = {
            let window = objref.lock();
            let top_left = window.get_position();
            let (width, height) = window.get_size();          
            Rectangle {
                top_left: top_left,
                bottom_right: top_left + (width as isize, height as isize)
            }
        };
        if refresh {
            self.refresh_bottom_windows(Some(area), true)?;
        }
        Ok(())
    }

    /// Return the index of a window if it is in the show list
    fn is_window_in_show_list(&mut self, objref: &Arc<Mutex<WindowView>>) -> Option<usize> {
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

    /// Return the index of a window if it is in the hide list
    fn is_window_in_hide_list(&mut self, objref: &Arc<Mutex<WindowView>>) -> Option<usize> {
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

    /// delete a window and refresh its region
    pub fn delete_window(&mut self, objref: &Arc<Mutex<WindowView>>) -> Result<(), &'static str> {
        let (top_left, bottom_right) = {
            let view = objref.lock();
            let top_left = view.get_position();
            let (width, height) = view.get_size();
            let bottom_right = top_left + (width as isize, height as isize);
            (top_left, bottom_right)
        };
        let area = Some(
            Rectangle {
                top_left: top_left,
                bottom_right: bottom_right
            }
        );
        if let Some(current_active) = self.active.upgrade() {
            if Arc::ptr_eq(&(current_active), objref) {
                self.refresh_bottom_windows(area, false)?;
                if let Some(window) = self.show_list.remove(0) {
                    self.active = window;
                } else if let Some(window) = self.hide_list.remove(0) {
                    self.active = window;
                } else {
                    self.active = Weak::new(); // delete reference
                }
                return Ok(());
            }
        }
        match self.is_window_in_show_list(&objref) {
            Some(i) => {
                self.show_list.remove(i);
                self.refresh_windows(area, true)?;
                return Ok(());
            }
            None => {}
        }
        match self.is_window_in_hide_list(&objref) {
            Some(i) => {
                self.hide_list.remove(i);
                self.refresh_windows(area, true)?;
                return Ok(());
            }
            None => {}
        }
        Err("cannot find this window")
    }

    /// Refresh the pixels in `update_coords`. Only render the bottom final framebuffer and windows. Ignore the top buffer.
    pub fn refresh_bottom_windows_pixels(&self, pixels: IntoIter<Coord>) -> Result<(), &'static str> {
        let bottom_fb = FrameBufferBlocks {
            framebuffer: self.bottom_fb.deref(),
            coordinate: Coord::new(0, 0),
            blocks: Some(pixels.clone())
        };

        FRAME_COMPOSITOR.lock().composite_pixels(vec![bottom_fb].into_iter())?;

        for window_ref in &self.hide_list {
            if let Some(window_mutex) = window_ref.upgrade() {
                let window = window_mutex.lock();
                let framebuffer = window.framebuffer.deref();
                let buffer_blocks = FrameBufferBlocks {
                    framebuffer: framebuffer.deref(),
                    coordinate: window.get_position(),
                    blocks: Some(pixels.clone())
                };

                FRAME_COMPOSITOR.lock().composite_pixels(vec![buffer_blocks].into_iter())?;
           }
        }

        for window_ref in &self.show_list {
            if let Some(window_mutex) = window_ref.upgrade() {
                let window = window_mutex.lock();
                let framebuffer = window.framebuffer.deref();
                let buffer_blocks = FrameBufferBlocks {
                    framebuffer: framebuffer.deref(),
                    coordinate: window.get_position(),
                    blocks: Some(pixels.clone())
                };

                FRAME_COMPOSITOR.lock().composite_pixels(vec![buffer_blocks].into_iter())?;
           }
        }

        if let Some(window_mutex) = self.active.upgrade() {
            let window = window_mutex.lock();
            let framebuffer = window.framebuffer.deref();
            let buffer_blocks = FrameBufferBlocks {
                framebuffer: framebuffer.deref(),
                coordinate: window.get_position(),
                blocks: Some(pixels)
            }; 

            FRAME_COMPOSITOR.lock().composite_pixels(vec![buffer_blocks].into_iter())?;
        }

        Ok(())
    }

    /// Refresh the pixels in the top framebuffer
    pub fn refresh_top_pixels(&self, pixels: IntoIter<Coord>) -> Result<(), &'static str> {
        let top_buffer = FrameBufferBlocks {
            framebuffer: self.top_fb.deref(),
            coordinate: Coord::new(0, 0),
            blocks: Some(pixels)
        }; 

        FRAME_COMPOSITOR.lock().composite_pixels(vec![top_buffer].into_iter())
    }

    /// Refresh the all the pixels including the bottom framebuffer, the windows and the top framebuffer.
    pub fn refresh_pixels(&self, pixels: IntoIter<Coord>) -> Result<(), &'static str> {
        self.refresh_bottom_windows_pixels(pixels.clone())?;
        self.refresh_top_pixels(pixels)?;
        Ok(())
    }

    /// Refresh the part of every window in `area`. Refresh the whole screen if area is None. 
    pub fn refresh_windows(&self, area: Option<Rectangle>, active: bool) -> Result<(), &'static str> {
        let update_all = area.is_none();

        let mut max_update_area = match area {
            Some(area) => {area},
            None => {
                Rectangle {
                    top_left: Coord::new(0, 0),
                    bottom_right: Coord::new(0, 0),
                }
            }
        };

        for window_ref in &self.hide_list {
            if let Some(window_mutex) = window_ref.upgrade() {
                let window = window_mutex.lock();
                let framebuffer = window.framebuffer.deref();
                //let (width, height) = window.get_size();

                let win_coordinate = window.get_position();
                let blocks = if !update_all {
                    let mut relative_area = max_update_area - win_coordinate;
                    let blocks = frame_buffer_compositor::get_blocks(framebuffer.deref(), &mut relative_area).into_iter();
                    max_update_area = relative_area + win_coordinate;
                    Some(blocks)
                } else {
                    None
                };

                let buffer_blocks = FrameBufferBlocks {
                    framebuffer: framebuffer.deref(),
                    coordinate: win_coordinate,
                    blocks: blocks
                };

                FRAME_COMPOSITOR.lock().composite(vec![buffer_blocks].into_iter())?;
           }
        }

        for window_ref in &self.show_list {
            if let Some(window_mutex) = window_ref.upgrade() {
                let window = window_mutex.lock();
                let framebuffer = window.framebuffer.deref();
                //let (width, height) = window.get_size();

                let win_coordinate = window.get_position();
                let blocks = if !update_all {
                    let mut relative_area = max_update_area - win_coordinate;
                    let blocks = frame_buffer_compositor::get_blocks(framebuffer.deref(), &mut relative_area).into_iter();
                    max_update_area = relative_area + win_coordinate;
                    Some(blocks)
                } else {
                    None
                };

                let buffer_blocks = FrameBufferBlocks {
                    framebuffer: framebuffer.deref(),
                    coordinate: win_coordinate,
                    blocks: blocks
                };

                FRAME_COMPOSITOR.lock().composite(vec![buffer_blocks].into_iter())?;
           }
        }

        if active {
            if let Some(window_mutex) = self.active.upgrade() {
                let window = window_mutex.lock();
                let framebuffer = window.framebuffer.deref();
                
                let win_coordinate = window.get_position();
                let blocks = if !update_all {
                    let mut relative_area = max_update_area - win_coordinate;
                    let blocks = frame_buffer_compositor::get_blocks(framebuffer.deref(), &mut relative_area).into_iter();
                    // max_update_area = relative_area + win_coordinate;
                    Some(blocks)
                } else {
                    None
                };

                let buffer_blocks = FrameBufferBlocks {
                    framebuffer: framebuffer.deref(),
                    coordinate: window.get_position(),
                    blocks: blocks
                }; 

                FRAME_COMPOSITOR.lock().composite(vec![buffer_blocks].into_iter())?;
            }
        }
       
        Ok(())

    }

    /// Refresh the part of bottom framebuffer and every window in `area`. Refresh the whole screen if area is None. 
    pub fn refresh_bottom_windows(&self, area: Option<Rectangle>, active: bool) -> Result<(), &'static str> {
        let update_all = area.is_none();
        let mut update_area = Rectangle {
            top_left: Coord::new(0, 0),
            bottom_right: Coord::new(0, 0),
        };

        let blocks = match area {
            Some(a) => {
                update_area = a;
                Some(
                    frame_buffer_compositor::get_blocks(self.bottom_fb.deref(), &mut update_area).into_iter()
                )
            },
            None => None
        };

        let bg_buffer = FrameBufferBlocks {
            framebuffer: self.bottom_fb.deref(),
            coordinate: Coord::new(0, 0),
            blocks: blocks
        }; 

        FRAME_COMPOSITOR.lock().composite(vec![bg_buffer].into_iter())?;

        let area_obj = if update_all{
            None
        } else {
            Some(update_area)
        };

        self.refresh_windows(area_obj, active)
    }

    pub fn refresh_top(&self, area: Option<Rectangle>) -> Result<(), &'static str> {
        let blocks = match area {
            Some(area) => {
                let mut update_area = area;
                Some(
                frame_buffer_compositor::get_blocks(self.top_fb.deref(), &mut update_area).into_iter()
                )
            },
            None => None
        };

        let top_buffer = FrameBufferBlocks {
            framebuffer: self.top_fb.deref(),
            coordinate: Coord::new(0, 0),
            blocks: blocks
        }; 

        FRAME_COMPOSITOR.lock().composite(vec![top_buffer].into_iter())
    }
    
    /// pass keyboard event to currently active window
    fn pass_keyboard_event_to_window(&self, key_event: KeyEvent) -> Result<(), &'static str> {
        if let Some(current_active) = self.active.upgrade() {
            let current_active_win = current_active.lock();
            current_active_win
                .producer
                .push(Event::new_keyboard_event(key_event)).map_err(|_e| "Fail to enqueue the mouse position event")?;
        }
        Err("cannot find window to pass key event")
    }

    /// pass mouse event to the top window that mouse on, may also transfer to those want all events
    fn pass_mouse_event_to_window(&self, mouse_event: MouseEvent) -> Result<(), &'static str> {
        let coordinate = { &self.mouse };
        let mut event: MousePositionEvent = MousePositionEvent {
            coordinate: Coord::new(0, 0),
            gcoordinate: coordinate.clone(),
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
            let current_coordinate = current_active_win.get_position();
            if current_active_win.give_all_mouse_event {
                event.coordinate = *coordinate - current_coordinate;
                current_active_win.producer.push(Event::MousePositionEvent(event.clone())).map_err(|_e| "Fail to enqueue the mouse position event")?;
            }
        }
        // show list
        for i in 0..self.show_list.len() {
            if let Some(now_view_mutex) = self.show_list[i].upgrade() {
                let now_view = now_view_mutex.lock();
                let current_coordinate = now_view.get_position();
                if now_view.give_all_mouse_event {
                    event.coordinate = *coordinate - current_coordinate;
                    now_view.producer.push(Event::MousePositionEvent(event.clone())).map_err(|_e| "Fail to enqueue the mouse position event")?;
                }
            }
        }

        // first check the active one
        if let Some(current_active) = self.active.upgrade() {
            let current_active_win = current_active.lock();
            let current_coordinate = current_active_win.get_position();
            if current_active_win.contains(*coordinate - current_coordinate) {
                event.coordinate = *coordinate - current_coordinate;
                // debug!("pass to active: {}, {}", event.x, event.y);
                current_active_win
                    .producer
                    .push(Event::MousePositionEvent(event)).map_err(|_e| "Fail to enqueue the mouse position event")?;
                return Ok(());
            }
            if current_active_win.is_moving {
                // do not pass the movement event to other windows
                return Ok(());
            }
        }
        // then check show_list
        for i in 0..self.show_list.len() {
            if let Some(now_view_mutex) = self.show_list[i].upgrade() {
                let now_view = now_view_mutex.lock();
                let current_coordinate = now_view.get_position();
                if now_view.contains(*coordinate - current_coordinate) {
                    event.coordinate = *coordinate - current_coordinate;
                    now_view
                        .producer
                        .push(Event::MousePositionEvent(event)).map_err(|_e| "Fail to enqueue the mouse position event")?;
                    return Ok(());
                }
            }
        }
        Err("cannot find window to pass")
    }

    /// refresh the floating border indicating user of new window position and size. `show` indicates whether to show the border or not.
    /// `start` and `end` indicates the top-left and bottom-right corner of the border.
    fn refresh_floating_border(
        &mut self,
        show: bool,
        top_left: Coord,
        bottom_right: Coord,
    ) -> Result<(), &'static str> {
        // first clear old border if exists
        let old_border = self.repositioned_border.clone();
        match old_border {
            Some(border) => {
                let pixels = self.draw_floating_border(border.top_left, border.bottom_right, T);
                self.refresh_bottom_windows_pixels(pixels.into_iter())?;
            },
            None =>{}
        }

        // then draw current border
        if show {
            self.repositioned_border = Some(Rectangle { top_left, bottom_right });
            let pixels = self.draw_floating_border(top_left, bottom_right, WINDOW_BORDER_COLOR_INNER);
            self.refresh_top_pixels(pixels.into_iter())?;
        } else {
            self.repositioned_border = None;
        }

        Ok(())
    }

    /// draw the floating border with color. Return pixels of the border.
    /// `start` and `end` indicates the top-left and bottom-right corner of the border.
    fn draw_floating_border(&mut self, top_left: Coord, bottom_right: Coord, color: Pixel) -> Vec<Coord> {
        let mut pixels = Vec::new();

        for i in 0..(WINDOW_BORDER_SIZE) as isize {
            let width = (bottom_right.x - top_left.x) - 2 * i;
            let height = (bottom_right.y - top_left.y) - 2 * i;
            let coordinate = top_left + (i as isize, i as isize);
            if width <= 0 || height <= 0 {
                break;
            }
            frame_buffer_drawer::draw_rectangle(
                self.top_fb.deref_mut(), 
                coordinate, 
                width as usize, 
                height as usize, 
                color
            );

            for m in 0..width {
                pixels.push(coordinate + (m, 0));
                pixels.push(coordinate + (m, height));
            }            
            
            for m in 1..height - 1 {
                pixels.push(coordinate + (0, m));
                pixels.push(coordinate + (width, m));
            }            
        }

        pixels
    }

    /// take active window's base position and current mouse, move the window with delta
    pub fn move_active_window(&mut self) -> Result<(), &'static str> {
        if let Some(current_active) = self.active.upgrade() {
            let (old_top_left, old_bottom_right, new_top_left, new_bottom_right) = {
                let mut current_active_win = current_active.lock();
                let (current_x, current_y) = {
                    let m = &self.mouse;
                    (m.x as isize, m.y as isize)
                };
                let base = current_active_win.moving_base;
                let (base_x, base_y) = (base.x, base.y);
                let old_top_left = current_active_win.get_position();
                let new_top_left = old_top_left + ((current_x - base_x), (current_y - base_y));
                let (width, height) = current_active_win.get_size();
                let old_bottom_right = old_top_left + (width as isize, height as isize);
                let new_bottom_right = new_top_left + (width as isize, height as isize);
                current_active_win.set_position(new_top_left);
                (old_top_left, old_bottom_right, new_top_left, new_bottom_right)
            };
            self.refresh_bottom_windows(Some(Rectangle{top_left: old_top_left, bottom_right: old_bottom_right}), false)?;
            self.refresh_bottom_windows(Some(Rectangle{top_left: new_top_left, bottom_right: new_bottom_right}), true)?;
            let update_coords = self.get_mouse_coords();
            self.refresh_pixels(update_coords.into_iter())?;
        } else {
            return Err("cannot fid active window to move");
        }
        Ok(())
    }

    /// Move mouse. `relative` indicates the new position relative to current position.
    fn move_mouse(&mut self, relative: Coord) -> Result<(), &'static str> {
        let old = self.mouse;
        let mut new = old + relative;
        
        let (screen_width, screen_height) = self.get_screen_size();
        if new.x < 0 {
            new.x = 0;
        }
        if new.y < 0 {
            new.y = 0;
        }
        if new.x >= (screen_width as isize) {
            new.x = (screen_width as isize) - 1;
        }
        if new.y >= (screen_height as isize) {
            new.y = (screen_height as isize) - 1;
        }
        self.move_mouse_to(new)
    }
    
    // Move mouse to absolute position `new`
    fn move_mouse_to(&mut self, new: Coord) -> Result<(), &'static str> {
        // clear old mouse
        for y in self.mouse.y - MOUSE_POINTER_HALF_SIZE as isize..self.mouse.y + MOUSE_POINTER_HALF_SIZE as isize + 1 {
            for x in
                self.mouse.x - MOUSE_POINTER_HALF_SIZE as isize..self.mouse.x + MOUSE_POINTER_HALF_SIZE as isize + 1
            {
                let coordinate = Coord::new(x, y);
                self.top_fb.overwrite_pixel(coordinate, T);
            }
        }
        let update_coords = self.get_mouse_coords();
        self.refresh_bottom_windows_pixels(update_coords.into_iter())?;

        // draw new mouse
        self.mouse = new;
        for y in new.y - MOUSE_POINTER_HALF_SIZE as isize..new.y + MOUSE_POINTER_HALF_SIZE as isize + 1
        {
            for x in
                new.x - MOUSE_POINTER_HALF_SIZE as isize..new.x + MOUSE_POINTER_HALF_SIZE as isize + 1
            {
                let coordinate = Coord::new(x, y);
                let pixel = MOUSE_BASIC
                            [(MOUSE_POINTER_HALF_SIZE as isize + x - new.x) as usize]
                            [(MOUSE_POINTER_HALF_SIZE as isize + y - new.y) as usize];
                self.top_fb.overwrite_pixel(
                        coordinate,
                        pixel,
                );
            }
        }
        let update_coords = self.get_mouse_coords();
        self.refresh_top_pixels(update_coords.into_iter())?;

        Ok(())
    }

    /// Move the floating border when a window is moving.
    pub fn move_floating_border(&mut self) -> Result<(), &'static str> {
        let (new_x, new_y) = {
            let m = &self.mouse;
            (m.x as isize, m.y as isize)
        };
        
        if let Some(current_active) = self.active.upgrade() {
            let (is_draw, border_start, border_end) = {
                let current_active_win = current_active.lock();
                if current_active_win.is_moving {
                    // move this window
                    // for better performance, while moving window, only border is shown for indication
                    let coordinate = current_active_win.get_position();
                    // let (current_x, current_y) = (coordinate.x, coordinate.y);
                    let base = current_active_win.moving_base;
                    let (base_x, base_y) = (base.x, base.y);
                    let (width, height) = current_active_win.get_size();
                    let border_start = coordinate + (new_x - base_x, new_y - base_y);
                    let border_end = border_start + (width as isize, height as isize);
                    (true, border_start, border_end)
                } else {
                    (false, Coord::new(0, 0), Coord::new(0, 0))
                }
            };
            self.refresh_floating_border(is_draw, border_start, border_end)?;
        } else {
            self.refresh_floating_border(false, Coord::new(0, 0), Coord::new(0, 0))?;
        }

        Ok(())
    }

    /// Whether a window is active
    pub fn is_active(&self, objref: &Arc<Mutex<WindowView>>) -> bool {
        if let Some(current_active) = self.active.upgrade() {
            if Arc::ptr_eq(&(current_active), objref) {
                return true;
            }
        }
        false
    }

    /// Get the screen size of the desktop
    pub fn get_screen_size(&self) -> (usize, usize) {
        self.bottom_fb.get_size()
    }

    /// Get the pixels occupied by current mouse.
    fn get_mouse_coords(&self) -> Vec<Coord> {
        let mut result = Vec::new();
        for i in 6..15 {
            for j in 6..15 {
                if MOUSE_BASIC[i][j] != T {
                    let coordinate = self.mouse - (7, 7) + (j as isize, i as isize);
                    if self.top_fb.contains(coordinate) {
                        result.push(coordinate)
                    }
                }
            }
        }

        result
    }
}

/// Initialize the window manager, should provide the consumer of keyboard and mouse event, as well as a frame buffer to draw
pub fn init<Buffer: FrameBuffer + Send>(
    mut bg_framebuffer: Buffer,
    mut top_framebuffer: Buffer,
) -> Result<(Queue<Event>, Queue<Event>), &'static str> {
    // initialize the framebuffer
    let (screen_width, screen_height) = bg_framebuffer.get_size();
    for x in 0..screen_width{
        for y in 0..screen_height {
            bg_framebuffer.draw_pixel(
                Coord::new(x as isize, y as isize),
                background::BACKGROUND[y / 2][x / 2]
            )
        }
    }
    top_framebuffer.fill_color(T); 

    // initialize static window manager
    let window_manager = WindowManager {
        hide_list: VecDeque::new(),
        show_list: VecDeque::new(),
        active: Weak::new(),
        mouse: Coord { x: 0, y: 0 },
        repositioned_border: None,
        bottom_fb: Box::new(bg_framebuffer),
        top_fb: Box::new(top_framebuffer),
    };
    WINDOW_MANAGER.call_once(|| Mutex::new(window_manager));

    let mut win = WINDOW_MANAGER
        .try()
        .ok_or("The static window manager was not yet initialized")?
        .lock();
    win.mouse = Coord {
        x: screen_width as isize / 2,
        y: screen_height as isize / 2,
    }; 
    
    win.refresh_bottom_windows(None, false)?;

    // keyinput queue initialization
    let keyboard_event_handling_queue: Queue<Event> = Queue::with_capacity(100);
    let key_consumer = keyboard_event_handling_queue.clone();
    let key_producer = key_consumer.clone();

    // mouse input queue initialization
    let mouse_event_handling_queue: Queue<Event> = Queue::with_capacity(100);
    let mouse_consumer = mouse_event_handling_queue.clone();
    let mouse_producer = mouse_consumer.clone();

    KernelTaskBuilder::new(window_manager_loop, (key_consumer, mouse_consumer))
        .name("window_manager_loop".to_string())
        .spawn()?;
    Ok((key_producer, mouse_producer))
}

/// handles all keyboard and mouse movement in this window manager
fn window_manager_loop(
    consumer: (Queue<Event>, Queue<Event>),
) -> Result<(), &'static str> {
    let (key_consumer, mouse_consumer) = consumer;

    loop {
        let event = {
            let ev = match key_consumer.pop() {
                Some(ev) => ev,
                _ => match mouse_consumer.pop() {
                    Some(ev) => ev,
                    _ => {
                        scheduler::schedule(); // yield the CPU and try again later
                        continue;
                    }
                },
            };
            let event = ev.clone();
            //ev.mark_completed();
            event
        };

        // event could be either key input or mouse input
        match event {
            Event::ExitEvent => {
                return Ok(());
            }
            Event::KeyboardEvent(ref input_event) => {
                let key_input = input_event.key_event;
                keyboard_handle_application(key_input)?;
            }
            Event::MouseMovementEvent(ref mouse_event) => {
                // mouse::mouse_to_print(&mouse_event);
                let mouse_displacement = &mouse_event.displacement;
                let mut x = (mouse_displacement.x as i8) as isize;
                let mut y = (mouse_displacement.y as i8) as isize;
                // need to combine mouse events if there pending a lot
                loop {
                    let next_event = match mouse_consumer.pop() {
                        Some(ev) => ev,
                        _ => {
                            break;
                        }
                    };
                    match next_event {
                        Event::MouseMovementEvent(ref next_mouse_event) => {
                            if next_mouse_event.mousemove.scrolling_up
                                == mouse_event.mousemove.scrolling_up
                                && next_mouse_event.mousemove.scrolling_down
                                    == mouse_event.mousemove.scrolling_down
                                && next_mouse_event.buttonact.left_button_hold
                                    == mouse_event.buttonact.left_button_hold
                                && next_mouse_event.buttonact.right_button_hold
                                    == mouse_event.buttonact.right_button_hold
                                && next_mouse_event.buttonact.fourth_button_hold
                                    == mouse_event.buttonact.fourth_button_hold
                                && next_mouse_event.buttonact.fifth_button_hold
                                    == mouse_event.buttonact.fifth_button_hold
                            {
                                x += (next_mouse_event.displacement.x as i8) as isize;
                                y += (next_mouse_event.displacement.y as i8) as isize;
                            }
                        }
                        _ => {
                            break;
                        }
                    }
                    // next_event.mark_completed();
                }
                if x != 0 || y != 0 {
                    let mut wm = WINDOW_MANAGER
                        .try()
                        .ok_or("The static window manager was not yet initialized")?
                        .lock();
                    wm.move_mouse(
                        Coord::new(x as isize, -(y as isize))
                    )?;
                }
                cursor_handle_application(*mouse_event)?; // tell the event to application, or moving window
            }
            _ => {}
        }
    }
}

/// handle keyboard event, push it to the active window if exists
fn keyboard_handle_application(key_input: KeyEvent) -> Result<(), &'static str> {
    // Check for WM-level actions here, e.g., spawning a new terminal via Ctrl+Alt+T
    if key_input.modifiers.control
        && key_input.keycode == Keycode::T
        && key_input.action == KeyAction::Pressed
    {
        // Since the WM currently runs in the kernel, we need to create a new application namespace for the terminal
        use mod_mgmt::{metadata::CrateType, CrateNamespace, NamespaceDir};
        let default_kernel_namespace = mod_mgmt::get_default_namespace()
            .ok_or("default CrateNamespace not yet initialized")?;
        let new_app_namespace_name = CrateType::Application.namespace_name().to_string();
        let new_app_namespace_dir = mod_mgmt::get_namespaces_directory()
            .and_then(|ns_dir| ns_dir.lock().get_dir(&new_app_namespace_name))
            .ok_or("Couldn't find the directory to create a new application CrateNamespace")?;
        let new_app_namespace = Arc::new(CrateNamespace::new(
            new_app_namespace_name,
            NamespaceDir::new(new_app_namespace_dir),
            Some(default_kernel_namespace.clone()),
        ));

        let task_name: String = format!("shell");
        let args: Vec<String> = vec![]; // shell::main() does not accept any arguments
        let terminal_obj_file = new_app_namespace
            .dir()
            .get_file_starting_with("shell-")
            .ok_or("Couldn't find shell application file to run upon Ctrl+Alt+T")?;
        let path = Path::new(terminal_obj_file.lock().get_absolute_path());
        ApplicationTaskBuilder::new(path)
            .argument(args)
            .name(task_name)
            .namespace(new_app_namespace)
            .spawn()?;
    }
    // then pass them to window
    let win = WINDOW_MANAGER
        .try()
        .ok_or("The static window manager was not yet initialized")?
        .lock();
    if let Err(_) = win.pass_keyboard_event_to_window(key_input) {
        // note that keyboard event should be passed to currently active window
        // if no window is active now, this function will return Err, but that's OK for now.
        // This part could be used to add logic when no active window is present, how to handle keyboards, but just leave blank now
    }
    Ok(())
}

/// handle mouse event, push it to related window or anyone asked for it
fn cursor_handle_application(mouse_event: MouseEvent) -> Result<(), &'static str> {
    let mut wm = WINDOW_MANAGER
        .try()
        .ok_or("The static window manager was not yet initialized")?
        .lock();
    wm.move_floating_border()?;

    if let Err(_) = wm.pass_mouse_event_to_window(mouse_event) {
        // the mouse event should be passed to the window that satisfies:
        // 1. the mouse position is currently in the window area
        // 2. the window is the top one (active window or show_list windows) under the mouse pointer
        // if no window is found in this position, that is system background area. Add logic to handle those events later
    }
    Ok(())
}
