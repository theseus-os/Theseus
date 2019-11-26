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
extern crate frame_buffer_alpha;
extern crate frame_buffer_compositor;
extern crate frame_buffer_drawer;
extern crate keycodes_ascii;
extern crate mod_mgmt;
extern crate mouse_data;
extern crate path;
extern crate scheduler;
extern crate spawn;
extern crate window;

mod background;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::sync::{Arc, Weak};
use alloc::vec::{IntoIter, Vec};
use compositor::Compositor;
use core::ops::{Deref, DerefMut};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use event_types::{Event, MousePositionEvent};
use frame_buffer::{Coord, FrameBuffer, Pixel, RectArea};
use frame_buffer_alpha::{AlphaPixel, PixelMixer, BLACK};
use frame_buffer_compositor::{FrameBufferBlocks, FRAME_COMPOSITOR, Block};
use keycodes_ascii::{KeyAction, KeyEvent, Keycode};
use mouse_data::MouseEvent;
use path::Path;
use spawn::{ApplicationTaskBuilder, KernelTaskBuilder};
use spin::{Mutex, Once};
use window::WindowProfile;

/// The alpha window manager
pub static WINDOW_MANAGER: Once<Mutex<WindowManagerAlpha<WindowProfileAlpha>>> = Once::new();

// The half size of mouse in number of pixels, the actual size of pointer is 1+2*`MOUSE_POINTER_HALF_SIZE`
const MOUSE_POINTER_HALF_SIZE: usize = 7;
// Transparent pixel
const T: AlphaPixel = 0xFF000000;
// Opaque white
const O: AlphaPixel = 0x00FFFFFF;
// Opaque blue
const B: AlphaPixel = 0x00000FF;
// the mouse picture
static MOUSE_BASIC: [[AlphaPixel; 2 * MOUSE_POINTER_HALF_SIZE + 1];
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
const WINDOW_BORDER_COLOR_INNER: AlphaPixel = 0x00CA6F1E;
// border's outer color
const WINDOW_BORDER_COLOR_OUTTER: AlphaPixel = 0xFFFFFFFF;

/// window manager with overlapping and alpha enabled
pub struct WindowManagerAlpha<U: WindowProfile> {
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
    final_fb: Box<dyn FrameBuffer>,
    /// the frame buffer that it should print on
    top_fb: Box<dyn FrameBuffer>,
    /// if it this is true, do not refresh whole screen until someone calls "refresh_area_absolute"
    delay_refresh_first_time: bool,
}

impl<U: WindowProfile> WindowManagerAlpha<U> {
    /// set one window to active, push last active (if exists) to top of show_list. if `refresh` is `true`, will then refresh the window's area
    pub fn set_active(
        &mut self,
        objref: &Arc<Mutex<U>>,
        refresh: bool,
    ) -> Result<(), &'static str> {
        let (start, end) = {
            let winobj = objref.lock();
            let start = winobj.get_position();
            let (width, height) = winobj.get_content_size();
            let end = start + (width as isize, height as isize);
            (start, end)
        };
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
        if refresh {
            let window = objref.lock();
            
            // Wenqiu
            // self.refresh_area(start, end)?;
            let frame_buffer_blocks = FrameBufferBlocks {
                framebuffer: window.framebuffer(),
                coordinate: window.coordinate(),
                blocks: None
            };
            FRAME_COMPOSITOR.lock().composite(vec![frame_buffer_blocks].into_iter())?;
            // self.refresh_area(start, end)?;
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
                if let Some(window) = self.show_list.remove(0) {
                    self.active = window;
                } else if let Some(window) = self.hide_list.remove(0) {
                    self.active = window;
                } else {
                    self.active = Weak::new(); // delete reference
                }
                self.refresh_window(area)?;
                return Ok(());
            }
        }
        match self.is_window_in_show_list(&objref) {
            Some(i) => {
                self.show_list.remove(i);
                self.refresh_window(area)?;
                return Ok(());
            }
            None => {}
        }
        match self.is_window_in_hide_list(&objref) {
            Some(i) => {
                self.hide_list.remove(i);
                self.refresh_window(area)?;
                return Ok(());
            }
            None => {}
        }
        Err("cannot find this window")
    }

    /// Recompute single pixel within show_list in a reduced complexity, by compute pixels under it only if it is not opaque
    fn recompute_single_pixel_show_list(&self, coordinate: Coord, idx: usize) -> AlphaPixel {
        if idx >= self.show_list.len() {
            // screen should be 1280*1080 but background figure is just 640*540
            // larger screen size will be black border and smaller screen size will see part of the background picture
            if (coordinate.x as usize) < 2 * background::BACKGROUND_WIDTH
                && (coordinate.y as usize) < 2 * background::BACKGROUND_HEIGHT
            {
                return background::BACKGROUND[coordinate.y as usize / 2]
                    [coordinate.x as usize / 2]
                    .into();
            }
            return BLACK; // return black
        }
        if let Some(now_winobj) = self.show_list[idx].upgrade() {
            // first get current color, to determine whether further get colors below
            let top = {
                let winobj = now_winobj.lock();
                let win_coord = winobj.get_position();
                let relative = coordinate - win_coord;
                let mut ret = T; // defult is transparent
                if winobj.contains(relative) {
                    let top = match winobj.get_pixel(relative) {
                        Ok(m) => m,
                        Err(_) => T, // transparent
                    };
                    if top.get_alpha() == 0 {
                        // totally opaque, so not waste computation
                        return top;
                    }
                    ret = top;
                }
                ret
            };
            let bottom = self.recompute_single_pixel_show_list(coordinate, idx + 1);
            return top.alpha_mix(bottom);
        } else {
            // need to delete this one, since the owner has been dropped, but here is immutable >.<
            // self.show_list.remove(idx);
            return self.recompute_single_pixel_show_list(coordinate, idx + 1);
        }
    }

    /// refresh one pixel on frame buffer
    fn refresh_single_pixel_with_buffer(&mut self, coordinate: Coord) -> Result<(), &'static str> {
        if !self.final_fb.contains(coordinate) {
            return Ok(());
        }
        let scoordinate = coordinate;
        if let Some(current_active) = self.active.upgrade() {
            let current_active_win = current_active.lock();
            let current_coord = current_active_win.get_position();

            let relative = scoordinate - current_coord;
            if current_active_win.contains(relative) {
                let top = current_active_win.get_pixel(relative)?;
                if top.get_alpha() == 0 {
                    // totally opaque, so not waste computation
                    self.final_fb.overwrite_pixel(coordinate, top);
                } else {
                    let bottom = self.recompute_single_pixel_show_list(coordinate, 0);
                    self.final_fb.overwrite_pixel(coordinate, top.alpha_mix(bottom));
                }
            } else {
                let pixel = self.recompute_single_pixel_show_list(coordinate, 0);
                self.final_fb.overwrite_pixel(coordinate, pixel);
            }
        } else {
            // nothing is active now
            let pixel = self.recompute_single_pixel_show_list(coordinate, 0);
            self.final_fb.overwrite_pixel(coordinate, pixel);
        }

        // then draw border
        if let Some(repositioned_border) = &self.repositioned_border {
            let (start, end) = {
                let r = &repositioned_border;
                (r.start, r.end)
            };
            let s_end_1 = end - (1, 1);
            let window_border_size = WINDOW_BORDER_SIZE as isize;

            let x_in = scoordinate.x >= start.x - window_border_size
                && scoordinate.x <= s_end_1.x + window_border_size;
            let y_in = scoordinate.y >= start.y - window_border_size
                && scoordinate.y <= s_end_1.y + window_border_size;
            let left = (start.x - scoordinate.x) as usize <= WINDOW_BORDER_SIZE && y_in;
            let right = (scoordinate.x - s_end_1.x) as usize <= WINDOW_BORDER_SIZE && y_in;
            let top = (start.y - scoordinate.y) as usize <= WINDOW_BORDER_SIZE && x_in;
            let bottom = (scoordinate.y - s_end_1.y) as usize <= WINDOW_BORDER_SIZE && x_in;
            let f32_window_border_size = WINDOW_BORDER_SIZE as f32;

            if left {
                if top {
                    // top-left
                    let dcoordinate = start - scoordinate;
                    // let dx = x_start - sx; let dy = y_start - sy;
                    if (dcoordinate.x + dcoordinate.y) as usize <= WINDOW_BORDER_SIZE {
                        self.final_fb.draw_pixel(
                            coordinate,
                            WINDOW_BORDER_COLOR_OUTTER.color_mix(
                                WINDOW_BORDER_COLOR_INNER,
                                (dcoordinate.x + dcoordinate.y) as usize as f32
                                    / f32_window_border_size,
                            ),
                        );
                    }
                } else if bottom {
                    // left-bottom
                    let dcoordinate =
                        Coord::new(start.x - scoordinate.x, scoordinate.y - s_end_1.y);
                    if (dcoordinate.x + dcoordinate.y) as usize <= WINDOW_BORDER_SIZE {
                        self.final_fb.draw_pixel(
                            coordinate,
                            WINDOW_BORDER_COLOR_OUTTER.color_mix(
                                WINDOW_BORDER_COLOR_INNER,
                                (dcoordinate.x + dcoordinate.y) as usize as f32
                                    / f32_window_border_size,
                            ),
                        );
                    }
                } else {
                    // only left
                    self.final_fb.draw_pixel(
                        coordinate,
                        WINDOW_BORDER_COLOR_OUTTER.color_mix(
                            WINDOW_BORDER_COLOR_INNER,
                            (start.x - scoordinate.x) as usize as f32 / f32_window_border_size,
                        ),
                    );
                }
            } else if right {
                if top {
                    // top-right
                    let dcoordinate =
                        Coord::new(scoordinate.x - s_end_1.x, start.y - scoordinate.y);
                    if (dcoordinate.x + dcoordinate.y) as usize <= WINDOW_BORDER_SIZE {
                        self.final_fb.draw_pixel(
                            coordinate,
                            WINDOW_BORDER_COLOR_OUTTER.color_mix(
                                WINDOW_BORDER_COLOR_INNER,
                                (dcoordinate.x + dcoordinate.y) as usize as f32
                                    / f32_window_border_size,
                            ),
                        );
                    }
                } else if bottom {
                    // bottom-right
                    let dcoordinate = scoordinate - s_end_1;
                    if (dcoordinate.x + dcoordinate.y) as usize <= WINDOW_BORDER_SIZE {
                        self.final_fb.draw_pixel(
                            coordinate,
                            WINDOW_BORDER_COLOR_OUTTER.color_mix(
                                WINDOW_BORDER_COLOR_INNER,
                                (dcoordinate.x + dcoordinate.y) as usize as f32
                                    / f32_window_border_size,
                            ),
                        );
                    }
                } else {
                    // only right
                    self.final_fb.draw_pixel(
                        coordinate,
                        WINDOW_BORDER_COLOR_OUTTER.color_mix(
                            WINDOW_BORDER_COLOR_INNER,
                            (scoordinate.x - s_end_1.x) as usize as f32 / f32_window_border_size,
                        ),
                    );
                }
            } else if top {
                // only top
                self.final_fb.draw_pixel(
                    coordinate,
                    WINDOW_BORDER_COLOR_OUTTER.color_mix(
                        WINDOW_BORDER_COLOR_INNER,
                        (start.y - scoordinate.y) as usize as f32 / f32_window_border_size,
                    ),
                );
            } else if bottom {
                // only bottom
                self.final_fb.draw_pixel(
                    coordinate,
                    WINDOW_BORDER_COLOR_OUTTER.color_mix(
                        WINDOW_BORDER_COLOR_INNER,
                        (coordinate.y - s_end_1.y) as usize as f32 / f32_window_border_size,
                    ),
                );
            }
        }
 
        // // finally draw mouse
        // let mcoordinate = { &self.mouse };
        // if ((scoordinate.x - mcoordinate.x) as usize <= MOUSE_POINTER_HALF_SIZE
        //     || (mcoordinate.x - scoordinate.x) as usize <= MOUSE_POINTER_HALF_SIZE)
        //     && ((scoordinate.y - mcoordinate.y) as usize <= MOUSE_POINTER_HALF_SIZE
        //         || (mcoordinate.y - scoordinate.y) as usize <= MOUSE_POINTER_HALF_SIZE)
        // {
        //     self.final_fb.draw_pixel(
        //         coordinate,
        //         MOUSE_BASIC
        //             [(MOUSE_POINTER_HALF_SIZE as isize + coordinate.x - mcoordinate.x) as usize]
        //             [(MOUSE_POINTER_HALF_SIZE as isize + coordinate.y - mcoordinate.y) as usize],
        //     );
        // }
        Ok(())
    }

    pub fn refresh_desktop_top_pixel(&mut self, coordinate: Coord, new: bool) {
        // finally draw mouse
        let mcoordinate = { &self.mouse };
        if ((coordinate.x - mcoordinate.x) as usize <= MOUSE_POINTER_HALF_SIZE
            || (mcoordinate.x - coordinate.x) as usize <= MOUSE_POINTER_HALF_SIZE)
            && ((coordinate.y - mcoordinate.y) as usize <= MOUSE_POINTER_HALF_SIZE
                || (mcoordinate.y - coordinate.y) as usize <= MOUSE_POINTER_HALF_SIZE)
        {
            
            if new {
                self.top_fb.overwrite_pixel(
                    coordinate,
                    MOUSE_BASIC
                        [(MOUSE_POINTER_HALF_SIZE as isize + coordinate.x - mcoordinate.x) as usize]
                        [(MOUSE_POINTER_HALF_SIZE as isize + coordinate.y - mcoordinate.y) as usize],
                );
            } else {
                self.top_fb.overwrite_pixel(coordinate, 0xFF000000);
            }
        } 
    }
    
    /// recompute single pixel value and refresh it on screen
    pub fn refresh_single_pixel(&mut self, coordinate: Coord) -> Result<(), &'static str> {
        let (width, height) = self.final_fb.get_size();
        if (coordinate.x as usize) < width && (coordinate.y as usize) < height {
            return self.refresh_single_pixel_with_buffer(coordinate);
        }
        return Ok(()); // don't need to update this pixel because it is not displayed on the screen
    }

    /// refresh an area by recompute every pixel in this region and update on the screen
    pub fn refresh_area(&mut self, mut start: Coord, mut end: Coord) -> Result<(), &'static str> {
        let background_fb = FrameBufferBlocks {
            framebuffer: self.final_fb.deref(),
            coordinate: Coord::new(0, 0),
            blocks: None
        };

        FRAME_COMPOSITOR.lock().composite(vec![background_fb].into_iter())?;
               
    Ok(())
        
/*        let (width, height) = self.final_fb.get_size();
        start.x = core::cmp::max(start.x, 0);
        end.x = core::cmp::min(end.x, width as isize);
        start.y = core::cmp::max(start.y, 0);
        end.y = core::cmp::min(end.y, height as isize);
        if start.x <= end.x && start.y <= end.y {
            for y in start.y..end.y {
                for x in start.x..end.x {
                    self.refresh_single_pixel_with_buffer(Coord::new(x, y))?;
                }
            }
        }
        Ok(())*/
    }

    pub fn refresh_cursor(&self, update_coords: &[Coord]) -> Result<(), &'static str> {

        let (width, height) = self.final_fb.get_size();
        
        let background_fb = FrameBufferBlocks {
            framebuffer: self.final_fb.deref(),
            coordinate: Coord::new(0, 0),
            blocks: None
        };

        FRAME_COMPOSITOR.lock().composite_pixels(vec![background_fb].into_iter(), update_coords)?;

        for window_ref in &self.show_list {
            if let Some(window_mutex) = window_ref.upgrade() {
                let window = window_mutex.lock();
                let framebuffer = window.framebuffer();
                let (width, height) = window.get_content_size();
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
            let (width, height) = window.get_content_size();
            let buffer_blocks = FrameBufferBlocks {
                framebuffer: framebuffer.deref(),
                coordinate: window.coordinate(),
                // blocks: Some(
                //     self.get_cursor_cache_block(window.get_position(), width, height).into_iter()
                // )
                blocks: None
            }; 

            FRAME_COMPOSITOR.lock().composite_pixels(vec![buffer_blocks].into_iter(), update_coords)?;
        }
       
        let (width, height) = self.top_fb.get_size();
        
        let top_buffer = FrameBufferBlocks {
            framebuffer: self.top_fb.deref(),
            coordinate: Coord::new(0, 0),
            blocks: None
        };
        FRAME_COMPOSITOR.lock().composite_pixels(vec![top_buffer].into_iter(), update_coords)?;

        Ok(())

    }

    pub fn refresh_background(&self, area: Option<RectArea>) -> Result<(), &'static str> {
        let blocks = match area.as_ref() {
            Some(area_ref) => {
                Some(
                frame_buffer_compositor::get_blocks(self.final_fb.deref(), *area_ref).into_iter()
                )
            },
            None => None
        };

        let bg_buffer = FrameBufferBlocks {
            framebuffer: self.final_fb.deref(),
            coordinate: Coord::new(0, 0),
            blocks: blocks
        }; 

        FRAME_COMPOSITOR.lock().composite(vec![bg_buffer].into_iter())
    }

    pub fn refresh_background_pixels(&self, pixels: &[Coord]) -> Result<(), &'static str> {
        let bg_buffer = FrameBufferBlocks {
            framebuffer: self.final_fb.deref(),
            coordinate: Coord::new(0, 0),
            blocks: None
        }; 

        FRAME_COMPOSITOR.lock().composite_pixels(vec![bg_buffer].into_iter(), pixels)
    }

    pub fn refresh_top(&self, area: Option<RectArea>) -> Result<(), &'static str> {
        let blocks = match area.as_ref() {
            Some(area_ref) => {
                Some(
                frame_buffer_compositor::get_blocks(self.top_fb.deref(), *area_ref).into_iter()
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

    pub fn refresh_top_pixels(&self, pixels: &[Coord]) -> Result<(), &'static str> {
        let top_buffer = FrameBufferBlocks {
            framebuffer: self.top_fb.deref(),
            coordinate: Coord::new(0, 0),
            blocks: None
        }; 

        FRAME_COMPOSITOR.lock().composite_pixels(vec![top_buffer].into_iter(), pixels)
    }

    pub fn refresh_window(&self, area: Option<RectArea>) -> Result<(), &'static str> {
        for window_ref in &self.hide_list {
            if let Some(window_mutex) = window_ref.upgrade() {
                let window = window_mutex.lock();
                let framebuffer = window.framebuffer();
                //let (width, height) = window.get_content_size();

                let win_coordinate = window.coordinate();
                let blocks = match area.as_ref() {
                    Some(area_ref) => {
                        let relative_area = *area_ref - win_coordinate;
                        Some(
                        frame_buffer_compositor::get_blocks(framebuffer.deref(), relative_area).into_iter()
                        )
                    },
                    None => None
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
                let blocks = match area.as_ref() {
                    Some(area_ref) => {
                        let relative_area = *area_ref - win_coordinate;
                        Some(
                        frame_buffer_compositor::get_blocks(framebuffer.deref(), relative_area).into_iter()
                        )
                    },
                    None => None
                };

                let buffer_blocks = FrameBufferBlocks {
                    framebuffer: framebuffer.deref(),
                    coordinate: win_coordinate,
                    blocks: blocks
                };

                FRAME_COMPOSITOR.lock().composite(vec![buffer_blocks].into_iter())?;
           }
        }

        if let Some(window_mutex) = self.active.upgrade() {
            let window = window_mutex.lock();
            let framebuffer = window.framebuffer();
            
            let win_coordinate = window.coordinate();
            let blocks = match area.as_ref() {
                Some(area_ref) => {
                    let relative_area = *area_ref - win_coordinate;
                    Some(
                    frame_buffer_compositor::get_blocks(framebuffer.deref(), relative_area).into_iter()
                    )
                },
                None => None
            };

            let buffer_blocks = FrameBufferBlocks {
                framebuffer: framebuffer.deref(),
                coordinate: window.coordinate(),
                // blocks: Some(
                //     self.get_cursor_cache_block(window.get_position(), width, height).into_iter()
                // )
                blocks: blocks
            }; 

            FRAME_COMPOSITOR.lock().composite(vec![buffer_blocks].into_iter())?;
        }
       

        Ok(())

    }

    pub fn refresh_window_pixels(&self, pixels: &[Coord]) -> Result<(), &'static str> {
        for window_ref in &self.hide_list {
            if let Some(window_mutex) = window_ref.upgrade() {
                let window = window_mutex.lock();
                let framebuffer = window.framebuffer();
                //let (width, height) = window.get_content_size();

                let win_coordinate = window.coordinate();


                let buffer_blocks = FrameBufferBlocks {
                    framebuffer: framebuffer.deref(),
                    coordinate: win_coordinate,
                    blocks: None
                };

                FRAME_COMPOSITOR.lock().composite_pixels(vec![buffer_blocks].into_iter(), pixels)?;
           }
        }
        
        for window_ref in &self.show_list {
            if let Some(window_mutex) = window_ref.upgrade() {
                let window = window_mutex.lock();
                let framebuffer = window.framebuffer();
                //let (width, height) = window.get_content_size();

                let win_coordinate = window.coordinate();


                let buffer_blocks = FrameBufferBlocks {
                    framebuffer: framebuffer.deref(),
                    coordinate: win_coordinate,
                    blocks: None
                };

                FRAME_COMPOSITOR.lock().composite_pixels(vec![buffer_blocks].into_iter(), pixels)?;
           }
        }

        if let Some(window_mutex) = self.active.upgrade() {
            let window = window_mutex.lock();
            let framebuffer = window.framebuffer();
            
            let win_coordinate = window.coordinate();

            let buffer_blocks = FrameBufferBlocks {
                framebuffer: framebuffer.deref(),
                coordinate: window.coordinate(),
                // blocks: Some(
                //     self.get_cursor_cache_block(window.get_position(), width, height).into_iter()
                // )
                blocks: None
            }; 

            FRAME_COMPOSITOR.lock().composite_pixels(vec![buffer_blocks].into_iter(), pixels)?;
        }
       

        Ok(())

    }
    
    /// refresh an rectangle border
    fn refresh_rect_border(
        &mut self,
        mut start: Coord,
        mut end: Coord,
    ) -> Result<(), &'static str> {
        let (width, height) = self.final_fb.get_size();
        start.x = core::cmp::max(start.x, 0);
        end.x = core::cmp::min(end.x, width as isize);
        start.y = core::cmp::max(start.y, 0);
        end.y = core::cmp::min(end.y, height as isize);

        if start.x <= end.x {
            if start.y < height as isize {
                for x in start.x..end.x {
                    self.refresh_single_pixel_with_buffer(Coord::new(x, start.y))?;
                }
            }
            if end.y > 0 {
                for x in start.x..end.x {
                    self.refresh_single_pixel_with_buffer(Coord::new(x, end.y - 1))?;
                }
            }
        }
        if start.y <= end.y {
            if start.x < width as isize {
                for y in start.y..end.y {
                    self.refresh_single_pixel_with_buffer(Coord::new(start.x, y))?;
                }
            }
            if end.x > 0 {
                for y in start.y..end.y {
                    self.refresh_single_pixel_with_buffer(Coord::new(end.x - 1, y))?;
                }
            }
        }
        Ok(())
    }

        /// refresh an rectangle border
    fn refresh_rect_border_moving(
        &mut self,
        mut start: Coord,
        mut end: Coord,
    ) -> Result<(), &'static str> {
        let (width, height) = self.final_fb.get_size();
        start.x = core::cmp::max(start.x, 0);
        end.x = core::cmp::min(end.x, width as isize);
        start.y = core::cmp::max(start.y, 0);
        end.y = core::cmp::min(end.y, height as isize);

        let mut pixels = Vec::new();
        frame_buffer_drawer::draw_rectangle(
            self.top_fb.deref_mut(),
            start,
            (end.x - start.x) as usize,
            (end.y - start.y) as usize,
            0xFF0000
        );

        if start.x <= end.x {
            if start.y < height as isize {
                for x in start.x..end.x {
                    pixels.push(Coord::new(x, start.y));
                }
            }
            if end.y > 0 {
                for x in start.x..end.x {
                    pixels.push(Coord::new(x, end.y - 1));
                }
            }
        }
        if start.y <= end.y {
            if start.x < width as isize {
                for y in start.y..end.y {
                    pixels.push(Coord::new(start.x, y));
                }
            }
            if end.x > 0 {
                for y in start.y..end.y {
                    pixels.push(Coord::new(end.x - 1, y));
                }
            }
        }
        self.refresh_cursor(pixels.as_slice())
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
        // Update if the old floating border is to be cleared or there is a new floating border
        let old_area = self.repositioned_border.clone();
        // first clear old border if exists
        if let Some(repositioned_border) = &self.repositioned_border {
            // if show {
            //     // otherwise don't change position
            //     self.repositioned_border = Some(RectArea { start, end });
            // } else {
            //     self.repositioned_border = None;
            // }
            let pixels = self.draw_floating_border(repositioned_border.start, repositioned_border.end, T);
            self.refresh_background_pixels(pixels.as_slice())?;
            self.refresh_window_pixels(pixels.as_slice())?;
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

    /// optimized refresh area for less computation up to 2x
    /// it works like this: first refresh all the region of new position, which let user see the new content faster
    /// then it try to reduce computation by only refresh the region that is in old one but NOT in new one.
    fn refresh_area_with_old_new(
        &mut self,
        old_start: Coord,
        old_end: Coord,
        new_start: Coord,
        new_end: Coord,
    ) -> Result<(), &'static str> {
        // first refresh new area for better user experience
        self.refresh_area(new_start, new_end)?;
        // then refresh old area that not overlapped by new one
        let x_not_overlapped = new_start.x >= old_end.x || new_end.x <= old_start.x;
        let y_not_overlapped = new_start.y >= old_end.y || new_end.y <= old_start.y;
        if x_not_overlapped || y_not_overlapped {
            // have to refresh all
            self.refresh_area(old_start, old_end)?;
        } else {
            // there existes overlapped region, optimize them!
            let mut greater_x_start = old_start.x; // this should be the larger one between `old_x_start` and `new_x_start`
            let mut smaller_x_end = old_end.x; // this should be the smaller one between `old_x_end` and `new_x_end`
            if old_start.x < new_start.x {
                self.refresh_area(old_start, old_end)?;
                greater_x_start = new_start.x;
            }
            if old_end.x > new_end.x {
                self.refresh_area(
                    Coord::new(new_end.x, old_start.y),
                    Coord::new(old_end.x, old_end.y),
                )?;
                smaller_x_end = new_end.x;
            }
            if old_start.y < new_start.y {
                self.refresh_area(
                    Coord::new(greater_x_start, old_start.y),
                    Coord::new(smaller_x_end, new_start.y),
                )?;
            }
            if old_end.y > new_end.y {
                self.refresh_area(
                    Coord::new(greater_x_start, new_end.y),
                    Coord::new(smaller_x_end, old_end.y),
                )?;
            }
        }
        Ok(())
    }

    /// private function: take active window's base position and current mouse, move the window with delta
    fn move_active_window(&mut self) -> Result<(), &'static str> {
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
            self.refresh_background(None)?;
            self.refresh_window(None)?;
            self.refresh_top(None)?;
            // then try to reduce time on refresh old ones
            // self.refresh_area_with_old_new(old_start, old_end, new_start, new_end)?;
        } else {
            return Err("cannot fid active window to move");
        }
        Ok(())
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

    fn get_cursor_cache_block(&self, origin: Coord, width: usize, height: usize) -> Vec<Block>{
        let coordinate = self.mouse - origin;

        let (start_x, start_y) = {
            let x = coordinate.x - 1;
            let y = coordinate.y - 1;
            (
                core::cmp::max(x, 0) as usize,
                core::cmp::max(y, 0) as usize,
            )
        };
        
        let mut mouse_blocks = Vec::new();

        if start_y >= height || start_y >= width {
            return mouse_blocks;
        }

        let index = start_y as usize / frame_buffer_compositor::CACHE_BLOCK_HEIGHT;
        let width = core::cmp::min(start_x + 9, width) - start_x;
        
        mouse_blocks.push(
            Block::new(index, start_x, width)
        );
        if (index + 1) * frame_buffer_compositor::CACHE_BLOCK_HEIGHT <= start_y + 9 {
            mouse_blocks.push(
                Block::new((index + 1), start_x, width)
            );
        }

        mouse_blocks
    }

    fn render(&self, framebuffer: &dyn FrameBuffer, blocks: Option<IntoIter<Block>>) -> Result<(), &'static str>{
        let mut bufferlist = Vec::new();
        bufferlist.push(
            FrameBufferBlocks {
                framebuffer: framebuffer,
                coordinate: Coord::new(0, 0),
                blocks: blocks,
            }
        );

        FRAME_COMPOSITOR
            .lock()
            .composite(bufferlist.into_iter())

    }

}

/// set window as active, the active window is always at top, so it will refresh the region of this window
pub fn set_active(objref: &Arc<Mutex<WindowProfileAlpha>>) -> Result<(), &'static str> {
    let mut win = WINDOW_MANAGER
        .try()
        .ok_or("The static window manager was not yet initialized")?
        .lock();
    win.set_active(objref, true)
}

/// whether a window is active
pub fn is_active(objref: &Arc<Mutex<WindowProfileAlpha>>) -> bool {
    match WINDOW_MANAGER
        .try()
        .ok_or("The static window manager was not yet initialized")
    {
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
    let mut win = WINDOW_MANAGER
        .try()
        .ok_or("The static window manager was not yet initialized")?
        .lock();
    let (new_x, new_y) = {
        let m = &win.mouse;
        (m.x as isize, m.y as isize)
    };
    
    if let Some(current_active) = win.active.upgrade() {
        let (is_draw, border_start, border_end) = {
            let current_active_win = current_active.lock();
            if current_active_win.is_moving() {
                // move this window
                // for better performance, while moving window, only border is shown for indication
                let coordinate = current_active_win.get_position();
                // let (current_x, current_y) = (coordinate.x, coordinate.y);
                let base = current_active_win.get_moving_base();
                let (base_x, base_y) = (base.x, base.y);
                let width = current_active_win.width;
                let height = current_active_win.height;
                let border_start = coordinate + (new_x - base_x, new_y - base_y);
                let border_end = border_start + (width as isize, height as isize);
                (true, border_start, border_end)
            } else {
                (false, Coord::new(0, 0), Coord::new(0, 0))
            }
        };
        win.refresh_floating_border(is_draw, border_start, border_end)?;
    } else {
        win.refresh_floating_border(false, Coord::new(0, 0), Coord::new(0, 0))?;
    }

    Ok(())
}

/// execute moving active window action, this will lock WINDOW_MANAGER
pub fn do_move_active_window() -> Result<(), &'static str> {
    let mut win = WINDOW_MANAGER
        .try()
        .ok_or("The static window manager was not yet initialized")?
        .lock();
    win.move_active_window()
}

/// refresh one pixel using absolute position, will lock WINDOW_MANAGER
pub fn refresh_pixel_absolute(coordinate: Coord) -> Result<(), &'static str> {
    let mut win = WINDOW_MANAGER
        .try()
        .ok_or("The static window manager was not yet initialized")?
        .lock();
    win.refresh_single_pixel(coordinate)
}

/// refresh an area using abosolute position, will lock WINDOW_MANAGER
pub fn refresh_area_absolute(start: Coord, end: Coord) -> Result<(), &'static str> {
    let mut win = WINDOW_MANAGER
        .try()
        .ok_or("The static window manager was not yet initialized")?
        .lock();
    if win.delay_refresh_first_time {
        win.delay_refresh_first_time = false;
        let (width, height) = win.final_fb.get_size();
        win.refresh_area_with_old_new(
            Coord::new(0, 0),
            Coord::new(width as isize, height as isize),
            start,
            end,
        )
    } else {
        win.refresh_area(start, end)
    }
}

/// Render the framebuffer of the window manager to the final buffer. Invoke this function after updating a window.
pub fn render(blocks: Option<IntoIter<Block>>) -> Result<(), &'static str> {
    let mut win = WINDOW_MANAGER
        .try()
        .ok_or("The static window manager was not yet initialized")?
        .lock();
    let mut bufferlist = Vec::new();
    // bufferlist.push(
    //     FrameBufferBlocks {
    //         framebuffer: win.final_fb.deref(),
    //         coordinate: Coord::new(0, 0),
    //         blocks: blocks,
    //     }
    // );

    let (width, height) = win.top_fb.get_size();
    bufferlist.push(
        FrameBufferBlocks {
            framebuffer: win.top_fb.deref(),
            coordinate: Coord::new(0, 0),
            blocks: Some(win.get_cursor_cache_block(Coord::new(0, 0), width, height).into_iter()),
        }
    );

    FRAME_COMPOSITOR
        .lock()
        .composite(bufferlist.into_iter())
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
    let delay_refresh_first_time = true;
    let window_manager = WindowManagerAlpha {
        hide_list: VecDeque::new(),
        show_list: VecDeque::new(),
        active: Weak::new(),
        mouse: Coord { x: 0, y: 0 },
        repositioned_border: None,
        final_fb: Box::new(bg_framebuffer),
        top_fb: Box::new(top_framebuffer),
        delay_refresh_first_time: delay_refresh_first_time,
    };
    WINDOW_MANAGER.call_once(|| Mutex::new(window_manager));

    let mut win = WINDOW_MANAGER
        .try()
        .ok_or("The static window manager was not yet initialized")?
        .lock();
    win.mouse = Coord {
        x: screen_width as isize / 2,
        y: screen_height as isize / 2,
    }; // set mouse to middle
    
    //if !delay_refresh_first_time {
    win.refresh_background(None)?;
    
    KernelTaskBuilder::new(window_manager_loop, (key_consumer, mouse_consumer))
        .name("window_manager_loop".to_string())
        .spawn()?;
    Ok(())
}

/// Window object that should be owned by the manager. It implements the `WindowProfile` trait.
pub struct WindowProfileAlpha {
    /// The position of the top-left corner of the window.
    /// It is relative to the top-left corner of the screen.
    pub coordinate: Coord,
    /// The width of the window.
    pub width: usize,
    /// The height of the window.
    pub height: usize,
    /// event consumer that could be used to get event input given to this window
    pub consumer: DFQueueConsumer<Event>, // event input
    producer: DFQueueProducer<Event>, // event output used by window manager
    /// frame buffer of this window
    pub framebuffer: Box<dyn FrameBuffer>,

    /// if true, window manager will send all mouse event to this window, otherwise only when mouse is on this window does it send.
    /// This is extremely helpful when application wants to know mouse movement outside itself, because by default window manager only sends mouse event
    /// when mouse is in the window's region. This is used when user move the window, to receive mouse event when mouse is out of the current window.
    pub give_all_mouse_event: bool,
    /// whether in moving state, only available when it is active. This is set when user press on the title bar (except for the buttons),
    /// and keeping mouse pressed when moving the mouse.
    pub is_moving: bool,
    /// the base position of window moving action, should be the mouse position when `is_moving` is set to true
    pub moving_base: Coord,
}

impl WindowProfile for WindowProfileAlpha {
    fn clear(&mut self) -> Result<(), &'static str> {
        self.framebuffer.fill_color(0x80FFFFFF);
        Ok(())
    }

    fn draw_border(&self, _color: u32) -> Result<(), &'static str> {
        // this window uses WindowComponents instead of border
        Ok(())
    }

    fn contains(&self, coordinate: Coord) -> bool {
        self.framebuffer.contains(coordinate)
    }

    fn resize(
        &mut self,
        _coordinate: Coord,
        _width: usize,
        _height: usize,
    ) -> Result<(usize, usize), &'static str> {
        // TODO: This system hasn't implemented resize
        Ok((0, 0))
    }

    fn get_content_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn get_position(&self) -> Coord {
        self.coordinate
    }

    fn events_producer(&mut self) -> &mut DFQueueProducer<Event> {
        &mut self.producer
    }

    fn set_position(&mut self, coordinate: Coord) {
        self.coordinate = coordinate;
    }

    fn get_moving_base(&self) -> Coord {
        self.moving_base
    }

    fn set_moving_base(&mut self, coordinate: Coord) {
        self.moving_base = coordinate
    }

    fn is_moving(&self) -> bool {
        self.is_moving
    }

    fn set_is_moving(&mut self, moving: bool) {
        self.is_moving = moving;   
    }

    fn set_give_all_mouse_event(&mut self, flag: bool) {
        self.give_all_mouse_event = flag;
    }

    fn give_all_mouse_event(&mut self) -> bool {
        false
    }

    fn get_pixel(&self, coordinate: Coord) -> Result<Pixel, &'static str> {
        self.framebuffer.get_pixel(coordinate)
    }

    fn framebuffer(&self) -> &dyn FrameBuffer {
        self.framebuffer.deref()
    }

    fn framebuffer_mut(&mut self) -> &mut dyn FrameBuffer {
        self.framebuffer.deref_mut()
    }

    fn coordinate(&self) -> Coord {
        self.coordinate
    }

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
                    move_mouse(x as isize, -(y as isize))?;
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
    do_refresh_floating_border()?;

    let wm = WINDOW_MANAGER
        .try()
        .ok_or("The static window manager was not yet initialized")?
        .lock();

    if let Err(_) = wm.pass_mouse_event_to_window(mouse_event) {
        // the mouse event should be passed to the window that satisfies:
        // 1. the mouse position is currently in the window area
        // 2. the window is the top one (active window or show_list windows) under the mouse pointer
        // if no window is found in this position, that is system background area. Add logic to handle those events later
    }
    Ok(())
}

/// return the screen size of current window manager as (width, height)
pub fn get_screen_size() -> Result<(usize, usize), &'static str> {
    let win = WINDOW_MANAGER
        .try()
        .ok_or("The static window manager was not yet initialized")?
        .lock();
    Ok(win.final_fb.get_size())
}

/// return current absolute position of mouse as (x, y)
pub fn get_cursor() -> Result<Coord, &'static str> {
    let win = WINDOW_MANAGER
        .try()
        .ok_or("The static window manager was not yet initialized")?
        .lock();
    Ok(win.mouse)
}

// move mouse with delta, this will refresh mouse position
fn move_mouse(x: isize, y: isize) -> Result<(), &'static str> {
    let old = get_cursor()?;
    let mut new = old + (x, y);
    let (screen_width, screen_height) = get_screen_size()?;
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
    move_mouse_to(new)?;
    Ok(())
}

// move mouse to absolute position
fn move_mouse_to(new: Coord) -> Result<(), &'static str> {
    let old = get_cursor()?;
    let mut win = WINDOW_MANAGER
        .try()
        .ok_or("The static window manager was not yet initialized")?
        .lock();

    // then update region of old mouse
    for y in old.y - MOUSE_POINTER_HALF_SIZE as isize..old.y + MOUSE_POINTER_HALF_SIZE as isize + 1
    {
        for x in
            old.x - MOUSE_POINTER_HALF_SIZE as isize..old.x + MOUSE_POINTER_HALF_SIZE as isize + 1
        {
            win.refresh_desktop_top_pixel(Coord::new(x, y), false);
        }
    }

    let update_coords = win.get_cursor_coords();
    win.refresh_cursor(update_coords.as_slice())?;

    win.mouse = new;
    // draw new mouse in the new position
    for y in new.y - MOUSE_POINTER_HALF_SIZE as isize..new.y + MOUSE_POINTER_HALF_SIZE as isize + 1
    {
        for x in
            new.x - MOUSE_POINTER_HALF_SIZE as isize..new.x + MOUSE_POINTER_HALF_SIZE as isize + 1
        {
            win.refresh_desktop_top_pixel(Coord::new(x, y), true);
        }
    }
    let (width, height) = win.top_fb.get_size();
    let new_blocks = win.get_cursor_cache_block(Coord::new(0, 0), width, height);

    let update_coords = win.get_cursor_coords();
    win.refresh_cursor(update_coords.as_slice())?;

//    win.render(win.top_fb.deref(), Some(new_blocks.into_iter()))?;

    Ok(())
}

/// Creates a new window object with given position and size
pub fn new_window<'a>(
    coordinate: Coord,
    framebuffer: Box<dyn FrameBuffer>,
) -> Result<Arc<Mutex<WindowProfileAlpha>>, &'static str> {
    // Init the key input producer and consumer
    let consumer = DFQueue::new().into_consumer();
    let producer = consumer.obtain_producer();

    let (width, height) = framebuffer.get_size();

    // new window object
    let mut window: WindowProfileAlpha = WindowProfileAlpha {
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

    window.clear()?;
    let window_ref = Arc::new(Mutex::new(window));
    let mut win = WINDOW_MANAGER
        .try()
        .ok_or("The static window manager was not yet initialized")?
        .lock();
    win.set_active(&window_ref, false)?; // do not refresh now for better speed

    Ok(window_ref)
}
