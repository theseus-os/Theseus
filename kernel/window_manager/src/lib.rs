//! A window manager that simulates a basic desktop environment with an alpha (transparency) channel.
//!
//! Multiple windows that overlap each other will be show according to conventional alpha blending techniques,
//! e.g., a transparent window will be rendered with the windows "beneath" it also being visible.
//!
//! Applications can create new window objects for themselves by invoking the `new_window()` function.
//! There are three groups that each window can be in: `active`, `show_list` and `hide_list`:
//! - `active`: not really a group, just a single window that is currently "active", i.e., receives all keyboard input events.
//! - `show_list`: windows that are currently shown on screen, ordered by their z-axis depth.
//! - `hide_list`: windows that are currently not being shown on screen, but may be shown later.
//!

#![no_std]

extern crate spin;
#[macro_use]
extern crate alloc;
extern crate dfqueue;
extern crate event_types;
#[macro_use]
extern crate log;
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
extern crate window;
extern crate window_generic;

mod background;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use compositor::Compositor;
use core::ops::{Deref, DerefMut};
use dfqueue::{DFQueue, DFQueueConsumer};
use event_types::{Event, MousePositionEvent};
use frame_buffer::{Coord, FrameBuffer, Pixel, RectArea};
use frame_buffer_compositor::{FrameBufferBlocks, FRAME_COMPOSITOR};
use keycodes_ascii::{KeyAction, KeyEvent, Keycode};
use mouse_data::MouseEvent;
use path::Path;
use spawn::{ApplicationTaskBuilder, KernelTaskBuilder};
use spin::{Mutex, Once};
use window::Window;
use window_generic::WindowGeneric;

/// The alpha window manager
pub static WINDOW_MANAGER: Once<Mutex<WindowManager<WindowGeneric>>> = Once::new();

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

/// window manager with overlapping and alpha enabled
pub struct WindowManager<U: Window> {
    /// those window currently not shown on screen
    hide_list: VecDeque<Weak<Mutex<U>>>,
    /// those window shown on screen that may overlapping each other
    show_list: VecDeque<Weak<Mutex<U>>>,
    /// the only active window, receiving all keyboard events (except for those remained for WM)
    active: Weak<Mutex<U>>, // this one is not in show_list
    /// current mouse position
    mouse: Coord,
    /// If a window is being repositioned (e.g., by dragging it), this is the position of that window's border
    repositioned_border: Option<RectArea>,
    /// the frame buffer that it should print on
    bottom_fb: Box<dyn FrameBuffer>,
    /// the frame buffer that it should print on
    top_fb: Box<dyn FrameBuffer>,
}

impl<U: Window> WindowManager<U> {
    /// set one window to active, push last active (if exists) to top of show_list. if `refresh` is `true`, will then refresh the window's area
    pub fn set_active(
        &mut self,
        objref: &Arc<Mutex<U>>,
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
            let start = window.get_position();
            let (width, height) = window.get_content_size();          
            RectArea {
                start: start,
                end: start + (width as isize, height as isize)
            }
        };
        if refresh {
            self.refresh_bottom_windows(Some(area), true)?;
        }
        Ok(())
    }

    /// Return the index of a window if it is in the show list
    fn is_window_in_show_list(&mut self, objref: &Arc<Mutex<U>>) -> Option<usize> {
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
    fn is_window_in_hide_list(&mut self, objref: &Arc<Mutex<U>>) -> Option<usize> {
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
    pub fn delete_window(&mut self, objref: &Arc<Mutex<U>>) -> Result<(), &'static str> {
        let (start, end) = {
            let winobj = objref.lock();
            let start = winobj.get_position();
            let (width, height) = winobj.get_content_size();
            let end = start + (width as isize, height as isize);
            (start, end)
        };
        let area = Some(
            RectArea {
                start: start,
                end: end
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

    pub fn refresh_bottom_windows_pixels(&self, update_coords: &[Coord]) -> Result<(), &'static str> {
        let bottom_fb = FrameBufferBlocks {
            framebuffer: self.bottom_fb.deref(),
            coordinate: Coord::new(0, 0),
            blocks: None
        };

        FRAME_COMPOSITOR.lock().composite_pixels(vec![bottom_fb].into_iter(), update_coords)?;

        for window_ref in &self.hide_list {
            if let Some(window_mutex) = window_ref.upgrade() {
                let window = window_mutex.lock();
                let framebuffer = window.framebuffer();
                let buffer_blocks = FrameBufferBlocks {
                    framebuffer: framebuffer.deref(),
                    coordinate: window.coordinate(),
                    blocks: None
                };

                FRAME_COMPOSITOR.lock().composite_pixels(vec![buffer_blocks].into_iter(), update_coords)?;
           }
        }

        for window_ref in &self.show_list {
            if let Some(window_mutex) = window_ref.upgrade() {
                let window = window_mutex.lock();
                let framebuffer = window.framebuffer();
                let buffer_blocks = FrameBufferBlocks {
                    framebuffer: framebuffer.deref(),
                    coordinate: window.coordinate(),
                    blocks: None
                };

                FRAME_COMPOSITOR.lock().composite_pixels(vec![buffer_blocks].into_iter(), update_coords)?;
           }
        }

        if let Some(window_mutex) = self.active.upgrade() {
            let window = window_mutex.lock();
            let framebuffer = window.framebuffer();
            let buffer_blocks = FrameBufferBlocks {
                framebuffer: framebuffer.deref(),
                coordinate: window.coordinate(),
                blocks: None
            }; 

            FRAME_COMPOSITOR.lock().composite_pixels(vec![buffer_blocks].into_iter(), update_coords)?;
        }

        Ok(())
    }

    pub fn refresh_top_pixels(&self, pixels: &[Coord]) -> Result<(), &'static str> {
        let top_buffer = FrameBufferBlocks {
            framebuffer: self.top_fb.deref(),
            coordinate: Coord::new(0, 0),
            blocks: None
        }; 

        FRAME_COMPOSITOR.lock().composite_pixels(vec![top_buffer].into_iter(), pixels)
    }

    pub fn refresh_pixels(&self, pixels: &[Coord]) -> Result<(), &'static str> {
        self.refresh_bottom_windows_pixels(pixels)?;
        self.refresh_top_pixels(pixels)?;
        Ok(())
    }

    pub fn refresh_windows(&self, area: Option<RectArea>, active: bool) -> Result<(), &'static str> {
        let update_all = area.is_none();

        let mut max_update_area = match area {
            Some(area) => {area},
            None => {
                RectArea{
                    start: Coord::new(0, 0),
                    end: Coord::new(0, 0),
                }
            }
        };

        for window_ref in &self.hide_list {
            if let Some(window_mutex) = window_ref.upgrade() {
                let window = window_mutex.lock();
                let framebuffer = window.framebuffer();
                //let (width, height) = window.get_content_size();

                let win_coordinate = window.coordinate();
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
                let framebuffer = window.framebuffer();
                //let (width, height) = window.get_content_size();

                let win_coordinate = window.coordinate();
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
                let framebuffer = window.framebuffer();
                
                let win_coordinate = window.coordinate();
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
                    coordinate: window.coordinate(),
                    blocks: blocks
                }; 

                FRAME_COMPOSITOR.lock().composite(vec![buffer_blocks].into_iter())?;
            }
        }
       
        Ok(())

    }

    pub fn refresh_bottom_windows(&self, area: Option<RectArea>, active: bool) -> Result<(), &'static str> {
        let update_all = area.is_none();
        let mut update_area = RectArea{
            start: Coord::new(0, 0),
            end: Coord::new(0, 0),
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

    pub fn refresh_top(&self, area: Option<RectArea>) -> Result<(), &'static str> {
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
            let mut current_active_win = current_active.lock();
            current_active_win
                .events_producer()
                .enqueue(Event::new_keyboard_event(key_event));
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
            let mut current_active_win = current_active.lock();
            let current_coordinate = current_active_win.get_position();
            if current_active_win.give_all_mouse_event() {
                event.coordinate = *coordinate - current_coordinate;
                current_active_win
                    .events_producer()
                    .enqueue(Event::MousePositionEvent(event.clone()));
            }
        }
        // show list
        for i in 0..self.show_list.len() {
            if let Some(now_winobj_mutex) = self.show_list[i].upgrade() {
                let mut now_winobj = now_winobj_mutex.lock();
                let current_coordinate = now_winobj.get_position();
                if now_winobj.give_all_mouse_event() {
                    event.coordinate = *coordinate - current_coordinate;
                    now_winobj
                        .events_producer()
                        .enqueue(Event::MousePositionEvent(event.clone()));
                }
            }
        }

        // first check the active one
        if let Some(current_active) = self.active.upgrade() {
            let mut current_active_win = current_active.lock();
            let current_coordinate = current_active_win.get_position();
            if current_active_win.contains(*coordinate - current_coordinate) {
                event.coordinate = *coordinate - current_coordinate;
                // debug!("pass to active: {}, {}", event.x, event.y);
                current_active_win
                    .events_producer()
                    .enqueue(Event::MousePositionEvent(event));
                return Ok(());
            }
            if current_active_win.is_moving() {
                // do not pass the movement event to other windows
                return Ok(());
            }
        }
        // then check show_list
        for i in 0..self.show_list.len() {
            if let Some(now_winobj_mutex) = self.show_list[i].upgrade() {
                let mut now_winobj = now_winobj_mutex.lock();
                let current_coordinate = now_winobj.get_position();
                if now_winobj.contains(*coordinate - current_coordinate) {
                    event.coordinate = *coordinate - current_coordinate;
                    now_winobj
                        .events_producer()
                        .enqueue(Event::MousePositionEvent(event));
                    return Ok(());
                }
            }
        }
        Err("cannot find window to pass")
    }

    /// refresh the floating border indicating user of new window position and size. `show` indicates whether to show the border or not.
    /// (x_start, x_end, y_start, y_end) indicates the position and size of this border.
    /// if this is not clear, you can simply try this attribute by pressing left mouse on the top bar of any window and move the mouse, then you should see a thin border
    /// this is extremely useful when performance is not enough for re-render the whole window when moving the mouse
    fn refresh_floating_border(
        &mut self,
        show: bool,
        start: Coord,
        end: Coord,
    ) -> Result<(), &'static str> {
        // first clear old border if exists
        let old_area = self.repositioned_border.clone();
        match old_area {
            Some(border) => {
                let pixels = self.draw_floating_border(border.start, border.end, T);
                self.refresh_bottom_windows_pixels(pixels.as_slice())?;
            },
            None =>{}
        }

        // then draw current border
        if show {
            self.repositioned_border = Some(RectArea { start, end });
            let pixels = self.draw_floating_border(start, end, WINDOW_BORDER_COLOR_INNER);
            self.refresh_top_pixels(pixels.as_slice())?;
        } else {
            self.repositioned_border = None;
        }

        Ok(())
    }

    fn draw_floating_border(&mut self, start: Coord, end: Coord, color: Pixel) -> Vec<Coord> {
        let mut pixels = Vec::new();

        for i in 0..(WINDOW_BORDER_SIZE) as isize {
            let width = (end.x - start.x) - 2 * i;
            let height = (end.y - start.y) - 2 * i;
            let coordinate = start + (i as isize, i as isize);
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
            let (old_start, old_end, new_start, new_end) = {
                let mut current_active_win = current_active.lock();
                let (current_x, current_y) = {
                    let m = &self.mouse;
                    (m.x as isize, m.y as isize)
                };
                let base = current_active_win.get_moving_base();
                let (base_x, base_y) = (base.x, base.y);
                let old_start = current_active_win.get_position();
                let new_start = old_start + ((current_x - base_x), (current_y - base_y));
                let (width, height) = current_active_win.get_content_size();
                let old_end = old_start + (width as isize, height as isize);
                let new_end = new_start + (width as isize, height as isize);
                current_active_win.set_position(new_start);
                (old_start, old_end, new_start, new_end)
            };
            self.refresh_bottom_windows(Some(RectArea{start: old_start, end: old_end}), false)?;
            self.refresh_bottom_windows(Some(RectArea{start: new_start, end: new_end}), true)?;
            let update_coords = self.get_cursor_coords();
            self.refresh_pixels(update_coords.as_slice())?;            // self.refresh_top(None)?;
            // then try to reduce time on refresh old ones
            // self.refresh_area_with_old_new(old_start, old_end, new_start, new_end)?;
        } else {
            return Err("cannot fid active window to move");
        }
        Ok(())
    }

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
    
    // move mouse to absolute position
    fn move_mouse_to(&mut self, new: Coord) -> Result<(), &'static str> {
        // clear old mouse
        let mut update_coords = Vec::new();
        for y in self.mouse.y - MOUSE_POINTER_HALF_SIZE as isize..self.mouse.y + MOUSE_POINTER_HALF_SIZE as isize + 1 {
            for x in
                self.mouse.x - MOUSE_POINTER_HALF_SIZE as isize..self.mouse.x + MOUSE_POINTER_HALF_SIZE as isize + 1
            {
                let coordinate = Coord::new(x, y);
                self.top_fb.overwrite_pixel(coordinate, T);
                update_coords.push(coordinate);
            }
        }
        self.refresh_bottom_windows_pixels(update_coords.as_slice())?;

        // draw new mouse
        self.mouse = new;
        update_coords = Vec::new();
        for y in new.y - MOUSE_POINTER_HALF_SIZE as isize..new.y + MOUSE_POINTER_HALF_SIZE as isize + 1
        {
            for x in
                new.x - MOUSE_POINTER_HALF_SIZE as isize..new.x + MOUSE_POINTER_HALF_SIZE as isize + 1
            {
                let coordinate = Coord::new(x, y);
                self.top_fb.overwrite_pixel(
                        coordinate,
                        MOUSE_BASIC
                            [(MOUSE_POINTER_HALF_SIZE as isize + x - new.x) as usize]
                            [(MOUSE_POINTER_HALF_SIZE as isize + y - new.y) as usize],
                );
                update_coords.push(coordinate)
            }
        }
        self.refresh_pixels(update_coords.as_slice())?;

        Ok(())
    }

    pub fn move_floating_border(&mut self) -> Result<(), &'static str> {
        let (new_x, new_y) = {
            let m = &self.mouse;
            (m.x as isize, m.y as isize)
        };
        
        if let Some(current_active) = self.active.upgrade() {
            let (is_draw, border_start, border_end) = {
                let current_active_win = current_active.lock();
                if current_active_win.is_moving() {
                    // move this window
                    // for better performance, while moving window, only border is shown for indication
                    let coordinate = current_active_win.get_position();
                    // let (current_x, current_y) = (coordinate.x, coordinate.y);
                    let base = current_active_win.get_moving_base();
                    let (base_x, base_y) = (base.x, base.y);
                    let (width, height) = current_active_win.get_content_size();
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

    /// whether a window is active
    pub fn is_active(&self, objref: &Arc<Mutex<U>>) -> bool {
        if let Some(current_active) = self.active.upgrade() {
            if Arc::ptr_eq(&(current_active), objref) {
                return true;
            }
        }
        false
    }

    pub fn get_screen_size(&self) -> (usize, usize) {
        self.bottom_fb.get_size()
    }

    fn get_cursor_coords(&self) -> Vec<Coord> {
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
pub fn init<Buffer: FrameBuffer>(
    key_consumer: DFQueueConsumer<Event>,
    mouse_consumer: DFQueueConsumer<Event>,
    mut bg_framebuffer: Buffer,
    top_framebuffer: Buffer,
) -> Result<(), &'static str> {
    debug!("Initializing the window manager alpha (transparency)...");

    let (screen_width, screen_height) = bg_framebuffer.get_size();
    for x in 0..screen_width{
        for y in 0..screen_height {
            bg_framebuffer.draw_pixel(
                Coord::new(x as isize, y as isize),
                background::BACKGROUND[y / 2][x / 2]
            )
        }
    }

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
    
    KernelTaskBuilder::new(window_manager_loop, (key_consumer, mouse_consumer))
        .name("window_manager_loop".to_string())
        .spawn()?;
    Ok(())
}

// handles all keyboard and mouse movement in this window manager
fn window_manager_loop(
    consumer: (DFQueueConsumer<Event>, DFQueueConsumer<Event>),
) -> Result<(), &'static str> {
    let (key_consumer, mouse_consumer) = consumer;

    loop {
        let event = {
            let ev = match key_consumer.peek() {
                Some(ev) => ev,
                _ => match mouse_consumer.peek() {
                    Some(ev) => ev,
                    _ => {
                        scheduler::schedule(); // yield the CPU and try again later
                        continue;
                    }
                },
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
                    let next_event = match mouse_consumer.peek() {
                        Some(ev) => ev,
                        _ => {
                            break;
                        }
                    };
                    match next_event.deref() {
                        &Event::MouseMovementEvent(ref next_mouse_event) => {
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
                    next_event.mark_completed();
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

// handle keyboard event, push it to the active window if exists
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

// handle mouse event, push it to related window or anyone asked for it
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

/// Creates a new window object with given position and size
pub fn new_window<'a>(
    coordinate: Coord,
    framebuffer: Box<dyn FrameBuffer>,
) -> Result<Arc<Mutex<WindowGeneric>>, &'static str> {
    // Init the key input producer and consumer
    let consumer = DFQueue::new().into_consumer();
    let producer = consumer.obtain_producer();

    let (width, height) = framebuffer.get_size();

    // new window object
    let mut window: WindowGeneric = WindowGeneric {
        coordinate: coordinate,
        width: width,
        height: height,
        consumer: consumer,
        producer: producer,
        framebuffer: framebuffer,
        give_all_mouse_event: false,
        is_moving: false,
        moving_base: Coord::new(0, 0), // the point as a base to start moving
    };

    let window_ref = Arc::new(Mutex::new(window));
    Ok(window_ref)
}
