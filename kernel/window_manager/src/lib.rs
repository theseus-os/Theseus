//! This crate acts as a manager of a list of windows. It defines a `WindowManager` structure and an instance of it. 
//!
//! A window manager holds a set of `WindowInner` objects, including an active window, a list of shown windows and a list of hidden windows. The hidden windows are totally overlapped by others.
//!
//! A window manager owns a bottom framebuffer and a top framebuffer. The bottom is the background of the desktop and the top framebuffer contains a floating window border and a mouse arrow. 
//! A window manager also contains a final framebuffer which is mapped to the screen. In refreshing an area, the manager will render all the framebuffers to the final one in order: bottom -> hide list -> showlist -> active -> top.
//!
//! The window manager provides methods to update within some bounding boxes rather than the whole screen for better performance.

#![no_std]

extern crate spin;
#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate mpmc;
extern crate event_types;
extern crate compositor;
extern crate framebuffer;
extern crate framebuffer_compositor;
extern crate framebuffer_drawer;
extern crate keycodes_ascii;
extern crate mod_mgmt;
extern crate mouse_data;
extern crate path;
extern crate scheduler; 
extern crate spawn;
extern crate window_inner;
extern crate shapes;
extern crate color;

use alloc::collections::VecDeque;
use alloc::string::ToString;
use alloc::sync::{Arc, Weak};
use alloc::vec::{Vec};
use compositor::{Compositor, FramebufferUpdates, CompositableRegion};

use mpmc::Queue;
use event_types::{Event, MousePositionEvent};
use framebuffer::{Framebuffer, AlphaPixel};
use color::{Color};
use shapes::{Coord, Rectangle};
use framebuffer_compositor::{FRAME_COMPOSITOR};
use keycodes_ascii::{KeyAction, KeyEvent, Keycode};
use mouse_data::MouseEvent;
use path::Path;
use spin::{Mutex, Once};
use window_inner::{WindowInner, WindowMovingStatus};

/// The instance of the default window manager
pub static WINDOW_MANAGER: Once<Mutex<WindowManager>> = Once::new();

/// The width and height size of mouse in number of pixels.
const MOUSE_POINTER_SIZE_Y: usize = 18;
const MOUSE_POINTER_SIZE_X: usize = 11;
/// The mouse pointer image defined as a 2-D pixel array.
static MOUSE_POINTER_IMAGE: [[Color; MOUSE_POINTER_SIZE_Y]; MOUSE_POINTER_SIZE_X] = {
    const T: Color = color::TRANSPARENT;
    const C: Color = color::BLACK; // Cursor
    const B: Color = color::WHITE; // Border
    [
        [B, B, B, B, B, B, B, B, B, B, B, B, B, B, B, B, T, T],
        [T, B, C, C, C, C, C, C, C, C, C, C, C, C, B, T, T, T],
        [T, T, B, C, C, C, C, C, C, C, C, C, C, B, T, T, T, T],
        [T, T, T, B, C, C, C, C, C, C, C, C, B, T, T, T, T, T],
        [T, T, T, T, B, C, C, C, C, C, C, C, C, B, B, T, T, T],
        [T, T, T, T, T, B, C, C, C, C, C, C, C, C, C, B, B, T],
        [T, T, T, T, T, T, B, C, C, C, C, B, B, C, C, C, C, B],
        [T, T, T, T, T, T, T, B, C, C, B, T, T, B, B, C, B, T],
        [T, T, T, T, T, T, T, T, B, C, B, T, T, T, T, B, B, T],
        [T, T, T, T, T, T, T, T, T, B, B, T, T, T, T, T, T, T],
        [T, T, T, T, T, T, T, T, T, T, B, T, T, T, T, T, T, T],
    ]
};

// the border indicating new window position and size
const WINDOW_BORDER_SIZE: usize = 3;
// border's inner color
const WINDOW_BORDER_COLOR_INNER: Color = Color::new(0x00CA6F1E);

/// Window manager structure which maintains a list of windows and a mouse.
pub struct WindowManager {
    /// those window currently not shown on screen
    hide_list: VecDeque<Weak<Mutex<WindowInner>>>,
    /// those window shown on screen that may overlapping each other
    show_list: VecDeque<Weak<Mutex<WindowInner>>>,
    /// the only active window, receiving all keyboard events (except for those remained for WM)
    active: Weak<Mutex<WindowInner>>, // this one is not in show_list
    /// current mouse position
    mouse: Coord,
    /// If a window is being repositioned (e.g., by dragging it), this is the position of that window's border
    repositioned_border: Option<Rectangle>,
    /// The bottom framebuffer typically contains the background/wallpaper image, 
    /// which is displayed by default when no other windows exist on top of it.
    bottom_fb: Framebuffer<AlphaPixel>,
    /// The top framebuffer is used for overlaying visual elements atop the rest of the windows, 
    /// e.g., the mouse pointer, the border of a window being dragged/moved, etc. 
    top_fb: Framebuffer<AlphaPixel>,
    /// The final framebuffer which is mapped to the screen (the actual display device).
    pub final_fb: Framebuffer<AlphaPixel>,
}

impl WindowManager {
    /// Sets one window as active, push last active (if exists) to top of show_list. if `refresh` is `true`, will then refresh the window's area.
    /// Returns whether this window is the first active window in the manager.
    /// 
    /// TODO FIXME: (kevinaboos) remove this dumb notion of "first active". This is a bad hack. 
    pub fn set_active(
        &mut self,
        inner_ref: &Arc<Mutex<WindowInner>>,
        refresh: bool,
    ) -> Result<bool, &'static str> {
        // if it is currently actived, just return
        let first_active = match self.active.upgrade() {
            Some(current_active) => {
                if Arc::ptr_eq(&(current_active), inner_ref) {
                    return Ok(true); // do nothing
                } else {
                    // save this to show_list
                    self.show_list.push_front(self.active.clone());
                }
                false
            }
            None => true,
        };
        
        match self.is_window_in_show_list(&inner_ref) {
            // remove item in current list
            Some(i) => {
                self.show_list.remove(i);
            }
            None => {}
        }
        match self.is_window_in_hide_list(&inner_ref) {
            // remove item in current list
            Some(i) => {
                self.hide_list.remove(i);
            }
            None => {}
        }
        self.active = Arc::downgrade(inner_ref);
        let area = {
            let window = inner_ref.lock();
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
        Ok(first_active)
    }

    /// Returns the index of a window if it is in the show list
    fn is_window_in_show_list(&mut self, window: &Arc<Mutex<WindowInner>>) -> Option<usize> {
        let mut i = 0_usize;
        for item in self.show_list.iter() {
            if let Some(item_ptr) = item.upgrade() {
                if Arc::ptr_eq(&(item_ptr), window) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// Returns the index of a window if it is in the hide list
    fn is_window_in_hide_list(&mut self, window: &Arc<Mutex<WindowInner>>) -> Option<usize> {
        let mut i = 0_usize;
        for item in self.hide_list.iter() {
            if let Some(item_ptr) = item.upgrade() {
                if Arc::ptr_eq(&(item_ptr), window) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// delete a window and refresh its region
    pub fn delete_window(&mut self, inner_ref: &Arc<Mutex<WindowInner>>) -> Result<(), &'static str> {
        let (top_left, bottom_right) = {
            let inner = inner_ref.lock();
            let top_left = inner.get_position();
            let (width, height) = inner.get_size();
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
            if Arc::ptr_eq(&current_active, inner_ref) {
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
        
        if let Some(index) = self.is_window_in_show_list(inner_ref) {
            self.show_list.remove(index);
            self.refresh_windows(area)?;
            return Ok(())        
        }

        if let Some(index) = self.is_window_in_hide_list(inner_ref) {
            self.hide_list.remove(index);
            return Ok(())        
        }
        Err("cannot find this window")
    }

    /// Refresh the region in `bounding_box`. Only render the bottom final framebuffer and windows. Ignore the active window if `active` is false.
    pub fn refresh_bottom_windows<B: CompositableRegion + Clone>(
        &mut self, 
        bounding_box: impl IntoIterator<Item = B> + Clone,
        active: bool,
    ) -> Result<(), &'static str> {
        // bottom framebuffer
        let bottom_fb_area = FramebufferUpdates {
            src_framebuffer: &self.bottom_fb,
            coordinate_in_dest_framebuffer: Coord::new(0, 0),
        };

        // list of windows to be updated
        let mut window_ref_list = Vec::new();
        for window in &self.hide_list {
            if let Some(window_ref) = window.upgrade() {
                window_ref_list.push(window_ref);
            }
        }
        for window in &self.show_list {
            if let Some(window_ref) = window.upgrade() {
                window_ref_list.push(window_ref);
            }
        }
        if active {
            if let Some(window_ref) = self.active.upgrade() {
                window_ref_list.push(window_ref)
            }
        }

        // lock windows
        let locked_window_list = &window_ref_list.iter().map(|x| x.lock()).collect::<Vec<_>>();

        // create updated framebuffer info objects
        let window_bufferlist = locked_window_list.iter().map(|window| {
            FramebufferUpdates {
                src_framebuffer: window.framebuffer(),
                coordinate_in_dest_framebuffer: window.get_position(),
            }
        });
        
        let buffer_iter = Some(bottom_fb_area).into_iter().chain(window_bufferlist);
        FRAME_COMPOSITOR.lock().composite(buffer_iter, &mut self.final_fb, bounding_box)?;
        
        Ok(())
    }

    /// Refresh the region of `bounding_box` in the top framebuffer
    pub fn refresh_top<B: CompositableRegion + Clone>(
        &mut self, 
        bounding_box: impl IntoIterator<Item = B> + Clone
    ) -> Result<(), &'static str> {
        let top_buffer = FramebufferUpdates {
            src_framebuffer: &self.top_fb,
            coordinate_in_dest_framebuffer: Coord::new(0, 0),
        }; 

        FRAME_COMPOSITOR.lock().composite(Some(top_buffer), &mut self.final_fb, bounding_box)
    }

    /// Refresh the part in `bounding_box` of every window. `bounding_box` is a region relative to the top-left of the screen. Refresh the whole screen if the bounding box is None.
    pub fn refresh_windows<B: CompositableRegion + Clone>(
        &mut self, 
        bounding_box: impl IntoIterator<Item = B> + Clone,
    ) -> Result<(), &'static str> {
        // reference of windows
        let mut window_ref_list = Vec::new();
        for window in &self.hide_list {
            if let Some(window_ref) = window.upgrade() {
                window_ref_list.push(window_ref);
            }
        }
        for window in &self.show_list {
            if let Some(window_ref) = window.upgrade() {
                window_ref_list.push(window_ref);
            }
        }

        if let Some(window_ref) = self.active.upgrade() {
            window_ref_list.push(window_ref)
        }

        // lock windows
        let locked_window_list = &window_ref_list.iter().map(|x| x.lock()).collect::<Vec<_>>();
        // create updated framebuffer info objects
        let bufferlist = locked_window_list.iter().map(|window| {
            FramebufferUpdates {
                src_framebuffer: window.framebuffer(),
                coordinate_in_dest_framebuffer: window.get_position(),
            }
        });

        FRAME_COMPOSITOR.lock().composite(bufferlist, &mut self.final_fb, bounding_box)
    }


    /// Refresh the part in `bounding_box` of the active window. `bounding_box` is a region relative to the top-left of the screen. Refresh the whole screen if the bounding box is None.
    pub fn refresh_active_window(&mut self, bounding_box: Option<Rectangle>) -> Result<(), &'static str> {
        if let Some(window_ref) = self.active.upgrade() {
            let window = window_ref.lock();
            let buffer_update = FramebufferUpdates {
                src_framebuffer: window.framebuffer(),
                coordinate_in_dest_framebuffer: window.get_position(),
            };
            FRAME_COMPOSITOR.lock().composite(Some(buffer_update), &mut self.final_fb, bounding_box)
        } else {
            Ok(())
        } 
    }
    
    /// Passes the given keyboard event to the currently active window.
    fn pass_keyboard_event_to_window(&self, key_event: KeyEvent) -> Result<(), &'static str> {
        let active_window = self.active.upgrade().ok_or("no window was set as active to receive a keyboard event")?;
        active_window.lock().send_event(Event::new_keyboard_event(key_event))
            .map_err(|_e| "Failed to enqueue the keyboard event; window event queue was full.")?;
        Ok(())
    }

    /// Passes the given mouse event to the window that the mouse is currently over. 
    /// 
    /// If the mouse is not over any window, an error is returned; 
    /// however, this error is quite common and expected when the mouse is not positioned within a window,
    /// and is not a true failure. 
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

        // TODO: FIXME:  improve this logic to just send the mouse event to the top-most window in the entire WM list,
        //               not just necessarily the active one. (For example, scroll wheel events can be sent to non-active windows).


        // first check the active one
        if let Some(current_active) = self.active.upgrade() {
            let current_active_win = current_active.lock();
            let current_coordinate = current_active_win.get_position();
            if current_active_win.contains(*coordinate - current_coordinate) || match current_active_win.moving {
                WindowMovingStatus::Moving(_) => true,
                _ => false,
            }{
                event.coordinate = *coordinate - current_coordinate;
                // debug!("pass to active: {}, {}", event.x, event.y);
                current_active_win.send_event(Event::MousePositionEvent(event))
                    .map_err(|_e| "Failed to enqueue the mouse event; window event queue was full.")?;
                return Ok(());
            }
        }

        // TODO FIXME: (kevinaboos): the logic below here is actually incorrect -- it could send mouse events to an invisible window below others.

        // then check show_list
        for i in 0..self.show_list.len() {
            if let Some(now_inner_mutex) = self.show_list[i].upgrade() {
                let now_inner = now_inner_mutex.lock();
                let current_coordinate = now_inner.get_position();
                if now_inner.contains(*coordinate - current_coordinate) {
                    event.coordinate = *coordinate - current_coordinate;
                    now_inner.send_event(Event::MousePositionEvent(event))
                        .map_err(|_e| "Failed to enqueue the mouse event; window event queue was full.")?;
                    return Ok(());
                }
            }
        }

        Err("the mouse position does not fall within the bounds of any window")
    }

    /// Refresh the floating border, which is used to show the outline of a window while it is being moved. 
    /// `show` indicates whether to show the border or not.
    /// `new_border` defines the rectangular outline of the border.
    fn refresh_floating_border(
        &mut self,
        show: bool,
        new_border: Rectangle,
    ) -> Result<(), &'static str> {
        // first clear old border if exists
        match self.repositioned_border {
            Some(border) => {
                let pixels = self.draw_floating_border(&border, color::TRANSPARENT);
                self.refresh_bottom_windows(pixels.into_iter(), true)?;
            },
            None =>{}
        }

        // then draw current border
        if show {
            let pixels = self.draw_floating_border(&new_border, WINDOW_BORDER_COLOR_INNER);
            self.refresh_top(pixels.into_iter())?;
            self.repositioned_border = Some(new_border);
        } else {
            self.repositioned_border = None;
        }

        Ok(())
    }

    /// draw the floating border with `pixel`. Return the list of coordinates of pixels that were updated.
    /// `border` indicates the position of the border as a rectangle.
    /// `color` is the color of the floating border.
    fn draw_floating_border(&mut self, border: &Rectangle, color: Color) -> Vec<Coord> {
        let mut coordinates = Vec::new();
        let pixel = color.into();
        for i in 0..(WINDOW_BORDER_SIZE) as isize {
            let width = (border.bottom_right.x - border.top_left.x) - 2 * i;
            let height = (border.bottom_right.y - border.top_left.y) - 2 * i;
            let coordinate = border.top_left + (i as isize, i as isize);
            if width <= 0 || height <= 0 {
                break;
            }
            framebuffer_drawer::draw_rectangle(
                &mut self.top_fb, 
                coordinate, 
                width as usize, 
                height as usize, 
                pixel
            );

            for m in 0..width {
                coordinates.push(coordinate + (m, 0));
                coordinates.push(coordinate + (m, height));
            }            
            
            for m in 1..height - 1 {
                coordinates.push(coordinate + (0, m));
                coordinates.push(coordinate + (width, m));
            }            
        }

        coordinates
    }

    /// take active window's base position and current mouse, move the window with delta
    pub fn move_active_window(&mut self) -> Result<(), &'static str> {
        if let Some(current_active) = self.active.upgrade() {
            let border = Rectangle { 
                top_left: Coord::new(0, 0), 
                bottom_right: Coord::new(0, 0) 
            };
            self.refresh_floating_border(false, border)?;

            let (old_top_left, old_bottom_right, new_top_left, new_bottom_right) = {
                let mut current_active_win = current_active.lock();
                let (current_x, current_y) = {
                    let m = &self.mouse;
                    (m.x as isize, m.y as isize)
                };
                match current_active_win.moving {
                    WindowMovingStatus::Moving(base) => {
                        let old_top_left = current_active_win.get_position();
                        let new_top_left = old_top_left + ((current_x - base.x), (current_y - base.y));
                        let (width, height) = current_active_win.get_size();
                        let old_bottom_right = old_top_left + (width as isize, height as isize);
                        let new_bottom_right = new_top_left + (width as isize, height as isize);
                        current_active_win.set_position(new_top_left);
                        (old_top_left, old_bottom_right, new_top_left, new_bottom_right)        
                    },
                    WindowMovingStatus::Stationary => {
                        return Err("The window is not moving");
                    }
                }
            };
            self.refresh_bottom_windows(Some(Rectangle{top_left: old_top_left, bottom_right: old_bottom_right}), false)?;

            self.refresh_active_window(Some(Rectangle{top_left: new_top_left, bottom_right: new_bottom_right}))?;
            self.refresh_mouse()?;
        } else {
            return Err("cannot find active window to move");
        }
        Ok(())
    }

    /// Refresh the mouse display
    pub fn refresh_mouse(&mut self) -> Result<(), &'static str> {
        let bounding_box = Some(Rectangle {
            top_left: self.mouse,
            bottom_right: self.mouse + (MOUSE_POINTER_SIZE_X as isize, MOUSE_POINTER_SIZE_Y as isize)
        });
        
        self.refresh_top(bounding_box.into_iter())
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

        // keep mouse pointer border in the screen when it is at the right or bottom side.
        const MOUSE_POINTER_BORDER: isize = 3;
        new.x = core::cmp::min(new.x, screen_width as isize - MOUSE_POINTER_BORDER);
        new.y = core::cmp::min(new.y, screen_height as isize - MOUSE_POINTER_BORDER);
            
        self.move_mouse_to(new)
    }
    
    // Move mouse to absolute position `new`
    fn move_mouse_to(&mut self, new: Coord) -> Result<(), &'static str> {
        // clear old mouse
        for y in self.mouse.y..self.mouse.y + MOUSE_POINTER_SIZE_Y as isize {
            for x in
                self.mouse.x..self.mouse.x + MOUSE_POINTER_SIZE_X as isize {
                let coordinate = Coord::new(x, y);
                self.top_fb.overwrite_pixel(coordinate, color::TRANSPARENT.into());
            }
        }
        let bounding_box = Some(Rectangle {
            top_left: self.mouse,
            bottom_right: self.mouse + (MOUSE_POINTER_SIZE_X as isize, MOUSE_POINTER_SIZE_Y as isize)
        });
        self.refresh_bottom_windows(bounding_box.into_iter(), true)?;

        // draw new mouse
        self.mouse = new;
        for y in new.y..new.y + MOUSE_POINTER_SIZE_Y as isize {
            for x in new.x..new.x + MOUSE_POINTER_SIZE_X as isize {
                let coordinate = Coord::new(x, y);
                let pixel = MOUSE_POINTER_IMAGE[(x - new.x) as usize][(y - new.y) as usize].into();
                self.top_fb.overwrite_pixel(coordinate, pixel);
            }
        }
        self.refresh_mouse()?;

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
                match current_active_win.moving {
                    WindowMovingStatus::Moving(base) => {
                        // move this window
                        // for better performance, while moving window, only border is shown for indication
                        let coordinate = current_active_win.get_position();
                        // let (current_x, current_y) = (coordinate.x, coordinate.y);
                        let (width, height) = current_active_win.get_size();
                        let border_start = coordinate + (new_x - base.x, new_y - base.y);
                        let border_end = border_start + (width as isize, height as isize);
                        (true, border_start, border_end)
                    }
                    WindowMovingStatus::Stationary => (false, Coord::new(0, 0), Coord::new(0, 0)),
                }
            };
            let border = Rectangle {
                top_left: border_start,
                bottom_right: border_end,
            };
            self.refresh_floating_border(is_draw, border)?;
        } else {
            let border = Rectangle {
                top_left: Coord::new(0, 0),
                bottom_right: Coord::new(0, 0),
            };
            self.refresh_floating_border(false, border)?;
        }

        Ok(())
    }

    /// Returns true if the given `window` is the currently active window.
    pub fn is_active(&self, window: &Arc<Mutex<WindowInner>>) -> bool {
        self.active.upgrade()
            .map(|active| Arc::ptr_eq(&active, window))
            .unwrap_or(false)
    }

    /// Returns the `(width, height)` in pixels of the screen itself (the final framebuffer).
    pub fn get_screen_size(&self) -> (usize, usize) {
        self.final_fb.get_size()
    }
}

/// Initialize the window manager. It returns (keyboard_producer, mouse_producer) for the I/O devices.
pub fn init() -> Result<(Queue<Event>, Queue<Event>), &'static str> {
    let final_framebuffer: Framebuffer<AlphaPixel> = framebuffer::init()?;
    let (width, height) = final_framebuffer.get_size();

    let mut bottom_framebuffer = Framebuffer::new(width, height, None)?;
    let mut top_framebuffer = Framebuffer::new(width, height, None)?;
    let (screen_width, screen_height) = bottom_framebuffer.get_size();
    bottom_framebuffer.fill(color::LIGHT_GRAY.into());
    top_framebuffer.fill(color::TRANSPARENT.into()); 

    // the mouse starts in the center of the screen.
    let center = Coord {
        x: screen_width as isize / 2,
        y: screen_height as isize / 2,
    }; 

    // initialize static window manager
    let window_manager = WindowManager {
        hide_list: VecDeque::new(),
        show_list: VecDeque::new(),
        active: Weak::new(),
        mouse: center,
        repositioned_border: None,
        bottom_fb: bottom_framebuffer,
        top_fb: top_framebuffer,
        final_fb: final_framebuffer,
    };
    let _wm = WINDOW_MANAGER.call_once(|| Mutex::new(window_manager));

    // wm.refresh_bottom_windows(None, false)?;

    // keyinput queue initialization
    let key_consumer: Queue<Event> = Queue::with_capacity(100);
    let key_producer = key_consumer.clone();

    // mouse input queue initialization
    let mouse_consumer: Queue<Event> = Queue::with_capacity(100);
    let mouse_producer = mouse_consumer.clone();

    spawn::new_task_builder(window_manager_loop, (key_consumer, mouse_consumer))
        .name("window_manager_loop".to_string())
        .spawn()?;

    Ok((key_producer, mouse_producer))
}

/// handles all keyboard and mouse movement in this window manager
fn window_manager_loop(
    (key_consumer, mouse_consumer): (Queue<Event>, Queue<Event>),
) -> Result<(), &'static str> {
    loop {
        let event_opt = key_consumer.pop()
            .or_else(||mouse_consumer.pop())
            .or_else(||{
                scheduler::schedule();
                None
            });

        if let Some(event) = event_opt {
            // Currently, the window manager only cares about keyboard or mouse events
            match event {
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

                    while let Some(next_event) = mouse_consumer.pop() {
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
                            .get()
                            .ok_or("The static window manager was not yet initialized")?
                            .lock();
                        wm.move_mouse(
                            Coord::new(x as isize, -(y as isize))
                        )?;
                    }
                    cursor_handle_application(*mouse_event)?; // tell the event to application, or moving window
                }
                _other => {
                    trace!("WINDOW_MANAGER: ignoring unexpected event: {:?}", _other);
                }
            }
        }
    }
}

/// handle keyboard event, push it to the active window if one exists
fn keyboard_handle_application(key_input: KeyEvent) -> Result<(), &'static str> {
    let win_mgr = WINDOW_MANAGER.get().ok_or("The window manager was not yet initialized")?;
    
    // First, we handle keyboard shortcuts understood by the window manager.
    
    // "Super + Arrow" will resize and move windows to the specified half of the screen (left, right, top, or bottom)
    if key_input.modifiers.is_super_key() && key_input.action == KeyAction::Pressed {
        let screen_dimensions = win_mgr.lock().get_screen_size();
        let (width, height) = (screen_dimensions.0 as isize, screen_dimensions.1 as isize);
        let new_position: Option<Rectangle> = match key_input.keycode {
            Keycode::Left => Some(Rectangle {
                top_left:     Coord { x: 0, y: 0 },
                bottom_right: Coord { x: width / 2, y: height },
            }),
            Keycode::Right => Some(Rectangle {
                top_left:     Coord { x: width / 2, y: 0 },
                bottom_right: Coord { x: width, y: height },
            }),
            Keycode::Up => Some(Rectangle {
                top_left:     Coord { x: 0, y: 0 },
                bottom_right: Coord { x: width, y: height / 2 },
            }),
            Keycode::Down => Some(Rectangle {
                top_left:     Coord { x: 0, y: height / 2 },
                bottom_right: Coord { x: width, y: height },
            }),
            _ => None,
        };
        
        if let Some(position) = new_position {
            let mut wm = win_mgr.lock();
            if let Some(active_window) = wm.active.upgrade() {
                debug!("window_manager: resizing active window to {:?}", new_position);
                active_window.lock().resize(position)?;

                // force refresh the entire screen for now
                // TODO: perform a proper screen refresh here: only refresh the area that contained the active_window's old bounds.
                wm.refresh_bottom_windows(Option::<Rectangle>::None, true)?;
            }
        }

        return Ok(());
    }

    // Spawn a new terminal via Ctrl+Alt+T
    if key_input.modifiers.is_control()
        && key_input.modifiers.is_alt()
        && key_input.keycode == Keycode::T
        && key_input.action == KeyAction::Pressed
    {
        // Because this task (the window manager loop) runs in a kernel-only namespace,
        // we have to create a new application namespace in order to be able to actually spawn a shell.

        let new_app_namespace = mod_mgmt::create_application_namespace(None)?;
        let shell_objfile = new_app_namespace.dir().get_file_starting_with("shell-")
            .ok_or("Couldn't find shell application file to run upon Ctrl+Alt+T")?;
        let path = Path::new(shell_objfile.lock().get_absolute_path());
        spawn::new_application_task_builder(path, Some(new_app_namespace))?
            .name(format!("shell"))
            .spawn()?;

        debug!("window_manager: spawned new shell app in new app namespace.");
        return Ok(());
    }

    // Any keyboard event unhandled above should be passed to the active window.
    if let Err(_e) = win_mgr.lock().pass_keyboard_event_to_window(key_input) {
        warn!("window_manager: failed to pass keyboard event to active window. Error: {:?}", _e);
        // If no window is currently active, then something might be potentially wrong, 
        // but we can likely recover in the future when another window becomes active.
        // Thus, we don't need to return a hard error here. 
    }
    Ok(())
}

/// handle mouse event, push it to related window or anyone asked for it
fn cursor_handle_application(mouse_event: MouseEvent) -> Result<(), &'static str> {
    let wm = WINDOW_MANAGER.get().ok_or("The static window manager was not yet initialized")?.lock();
    if let Err(_) = wm.pass_mouse_event_to_window(mouse_event) {
        // the mouse event should be passed to the window that satisfies:
        // 1. the mouse position is currently in the window area
        // 2. the window is the top one (active window or show_list windows) under the mouse pointer
        // if no window is found in this position, that is system background area. Add logic to handle those events later
    }
    Ok(())
}
