//! Window manager that simulates a desktop environment.
//! *note: since window overlap is not yet supported, any application that asks for a window that would overlap
//! with an existing window will be returned an error
//!
//! Applications request window objects from the window manager through either of two functions:
//! - default_new_window() will provide a window of default size (centered, fills majority of screen)
//! - new_window() provides a new window whose dimensions the caller must specify
//!
//! Windows can be resized by calling resize().
//! Window can be deleted when it is dropped or by calling WindowObj.delete().
//! Once an active window is deleted or set as inactive, the next window in the background list will become active.
//! The orde of windows is based on the last time it was active. The one which was active most recently is the top of the background list
//!
//! The WINDOW_ALLOCATOR is used by the WindowManager itself to track and modify the existing windows

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
extern crate frame_buffer_printer;
extern crate text_display;
#[macro_use]
extern crate lazy_static;
extern crate displayable;
extern crate font;
extern crate window;
extern crate window_manager;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::{String, ToString};
use alloc::sync::{Arc, Weak};
use alloc::boxed::Box;
use compositor::Compositor;
use core::ops::{Deref, DerefMut};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use displayable::Displayable;
use event_types::Event;
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH};
use frame_buffer::FrameBuffer;
use frame_buffer_compositor::FRAME_COMPOSITOR;
use frame_buffer_drawer::*;
use spin::{Mutex, Once};
use text_display::{Cursor, TextDisplay};
use window::Window;
use window_manager::{WINDOWLIST, SCREEN_FRAME_BUFFER, WINDOW_MARGIN, WINDOW_PADDING, WINDOW_INACTIVE_COLOR, WINDOW_ACTIVE_COLOR, SCREEN_BACKGROUND_COLOR};


/// A window contains a reference to its inner reference owned by the window manager,
/// a consumer of inputs, a list of displayables and a framebuffer
pub struct WindowObj {
    pub inner: Arc<Mutex<Box<Window>>>,
    pub consumer: DFQueueConsumer<Event>,
    pub components: BTreeMap<String, Component>,
    /// the framebuffer owned by the window
    pub framebuffer: FrameBuffer,
}

impl WindowObj {
    /// clean the content of a window. The border and padding of the window remain showing
    pub fn clean(&mut self) -> Result<(), &'static str> {
        let (width, height) = self.inner.lock().get_content_size();
        fill_rectangle(
            &mut self.framebuffer,
            0,
            0,
            width,
            height,
            SCREEN_BACKGROUND_COLOR,
        );
        self.render()
    }

    /// Returns the content dimensions of this window,
    /// as a tuple of `(width, height)`. It does not include the padding
    pub fn dimensions(&self) -> (usize, usize) {
        let inner_locked = self.inner.lock();
        inner_locked.get_content_size()
    }

    /// Add a new displayable structure to the window
    /// We check if the displayable is in the window. But we do not check if it is overlapped with others
    pub fn add_displayable(
        &mut self,
        key: &str,
        x: usize,
        y: usize,
        displayable: TextDisplay,
    ) -> Result<(), &'static str> {
        let key = key.to_string();
        let (width, height) = displayable.get_size();
        let inner = self.inner.lock();

        if !inner.check_in_content(x, y)
            || !inner.check_in_content(x + width, y)
            || !inner.check_in_content(x, y + height)
            || !inner.check_in_content(x + width, y + height)
        {
            return Err("The displayable does not fit the window size.");
        }

        let component = Component {
            x: x,
            y: y,
            displayable: displayable,
        };

        self.components.insert(key, component);
        Ok(())
    }

    /// Remove a displayable according to its name
    pub fn remove_displayable(&mut self, name: &str) {
        self.components.remove(name);
    }

    /// Get a displayable of the name
    pub fn get_displayable_mut(&mut self, name: &str) -> Option<&mut TextDisplay> {
        let opt = self.components.get_mut(name);
        match opt {
            None => return None,
            Some(component) => {
                return Some(component.get_displayable_mut());
            }
        };
    }

    /// Get a displayable of the name
    pub fn get_displayable(&self, name: &str) -> Option<&TextDisplay> {
        let opt = self.components.get(name);
        match opt {
            None => return None,
            Some(component) => {
                return Some(component.get_displayable());
            }
        };
    }

    /// Get the position of a displayable in the window
    pub fn get_displayable_position(&self, key: &str) -> Result<(usize, usize), &'static str> {
        let opt = self.components.get(key);
        match opt {
            None => {
                return Err("No such displayable");
            }
            Some(component) => {
                return Ok(component.get_position());
            }
        };
    }

    /// Get the content position of the window excluding border and padding
    pub fn get_content_position(&self) -> (usize, usize) {
        self.inner.lock().get_content_position()
    }

    pub fn inner(&self) -> &Arc<Mutex<Box<Window>>> {
        &self.inner
    }

    // /// draw a pixel in a window
    // pub fn draw_pixel(&mut self, x:usize, y:usize, color:u32) -> Result<(), &'static str> {
    //     draw_pixel(&mut self.framebuffer, x, y, color);
    //     let (content_x, content_y) = self.inner.lock().get_content_position();
    //     FrameCompositor::compose(
    //         vec![(&self.framebuffer, content_x, content_y)]
    //     )
    // }

    // /// draw a line in a window
    // pub fn draw_line(&mut self, start_x:usize, start_y:usize, end_x:usize, end_y:usize, color:u32) -> Result<(), &'static str> {
    //     draw_line(&mut self.framebuffer, start_x as i32, start_y as i32,
    //         end_x as i32, end_y as i32, color);
    //     let (content_x, content_y) = self.inner.lock().get_content_position();
    //     FrameCompositor::compose(
    //         vec![(&self.framebuffer, content_x, content_y)]
    //     )
    // }

    // /// draw a rectangle in a window
    // pub fn draw_rectangle(&mut self, x:usize, y:usize, width:usize, height:usize, color:u32)
    //     -> Result<(), &'static str> {
    //     draw_rectangle(&mut self.framebuffer, x, y, width, height,
    //             color);
    //     let (content_x, content_y) = self.inner.lock().get_content_position();
    //     FrameCompositor::compose(
    //         vec![(&self.framebuffer, content_x, content_y)]
    //     )
    // }

    // /// fill a rectangle in a window
    // pub fn fill_rectangle(&mut self, x:usize, y:usize, width:usize, height:usize, color:u32)
    //     -> Result<(), &'static str> {
    //     fill_rectangle(&mut self.framebuffer, x, y, width, height,
    //             color);
    //     let (content_x, content_y) = self.inner.lock().get_content_position();
    //     FrameCompositor::compose(
    //         vec![(&self.framebuffer, content_x, content_y)]
    //     )
    // }

    /// Display the content in the framebuffer of the window on the screen
    pub fn render(&mut self) -> Result<(), &'static str> {
        let (window_x, window_y) = { self.inner.lock().get_content_position() };
        FRAME_COMPOSITOR.lock().compose(vec![(
            &mut self.framebuffer,
            window_x as i32,
            window_y as i32,
        )])
    }

    /// print a string in the window with a text displayable.
    pub fn display_string(
        &mut self,
        display_name: &str,
        slice: &str,
        font_color: u32,
        bg_color: u32,
    ) -> Result<(), &'static str> {
        let displayable = self
            .components
            .get_mut(display_name)
            .ok_or("")?
            .get_displayable_mut();
        displayable.display(slice, 0, 0, font_color, bg_color, &mut self.framebuffer)?;
        self.render()?;

        Ok(())
    }

    /// display a cursor in the window with a text displayable. position is the absolute position of the cursor
    pub fn display_cursor(
        &mut self,
        display_name: &str,
        font_color: u32,
        bg_color: u32,
    ) -> Result<(), &'static str> {
        let displayable = self
            .components
            .get_mut(display_name)
            .ok_or("")?
            .get_displayable_mut();
        displayable.display_cursor(0, 0, font_color, bg_color, &mut self.framebuffer);
        self.render()?;
        Ok(())
    }

    // @Andrew
    /// resize a window as (width, height) at (x, y)
    pub fn resize(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    ) -> Result<(), &'static str> {
        // checks for overlap
        // {
        //     let inner = self.inner.clone();
        //     let mut allocator = try!(WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")).lock();
        //     match allocator.check_overlap(&inner, x,y,width,height) {
        //         true => {return Err("cannot resize because requested resize will cause overlap")}
        //         false => { }
        //     }
        // }

        self.clean()?;
        let mut inner = self.inner.lock();
        match inner.resize(x, y, width, height) {
            Ok(percent) => {
                for (_key, item) in self.components.iter_mut() {
                    let (x, y) = item.get_position();
                    let (width, height) = item.get_displayable().get_size();
                    item.resize(
                        x * percent.0 / 100,
                        y * percent.1 / 100,
                        width * percent.0 / 100,
                        height * percent.1 / 100,
                    );
                }
                inner
                    .key_producer()
                    .enqueue(Event::new_resize_event(x, y, width, height));
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    /// Get a key event of the window
    pub fn get_key_event(&self) -> Option<Event> {
        let event_opt = self.consumer.peek();
        if let Some(event) = event_opt {
            event.mark_completed();
            let event_data = event.deref().clone();
            Some(event_data)
        } else {
            None
        }
    }
}

// The structure is owned by the window manager. It contains the information of a window but under the control of the manager
pub struct WindowInner {
    // the upper left x-coordinate of the window
    pub x: usize,
    // the upper left y-coordinate of the window
    pub y: usize,
    // the width of the window
    pub width: usize,
    // the height of the window
    pub height: usize,
    // whether the window is active
    pub active: bool,
    // a consumer of key input events to the window
    pub padding: usize,
    // the producer accepting a key event
    pub key_producer: DFQueueProducer<Event>,
}

impl Window for WindowInner {
    //clean the window on the screen including the border and padding
    fn clean(&self) -> Result<(), &'static str> {
        let buffer_ref = match SCREEN_FRAME_BUFFER.try() {
            Some(buffer) => buffer,
            None => return Err("Fail to get the virtual frame buffer"),
        };
        let mut buffer_lock = buffer_ref.lock();
        let buffer = buffer_lock.deref_mut();
        draw_rectangle(
            buffer,
            self.x,
            self.y,
            self.width,
            self.height,
            SCREEN_BACKGROUND_COLOR,
        );
        FRAME_COMPOSITOR.lock().compose(vec![(buffer, 0, 0)])
    }

    // //check if the window is overlapped with any existing window
    // fn is_overlapped(&self, x:usize, y:usize, width:usize, height:usize) -> bool {
    //     return self.check_in_area(x, y) && self.check_in_area(x, y + height)
    //         && self.check_in_area(x + width, y) && self.check_in_area(x + width, y + height)
    // }

    // // check if the pixel is within the window
    // fn check_in_area(&self, x:usize, y:usize) -> bool {
    //     return x >= self.x && x <= self.x + self.width
    //             && y >= self.y && y <= self.y + self.height;
    // }

    // check if the pixel is within the window exluding the border and padding
    fn check_in_content(&self, x: usize, y: usize) -> bool {
        return x <= self.width - 2 * self.padding && y <= self.height - 2 * self.padding;
    }

    // active or inactive a window
    fn active(&mut self, active: bool) -> Result<(), &'static str> {
        self.active = active;
        self.draw_border(WINDOW_ACTIVE_COLOR)?;
        Ok(())
    }

    // draw the border of the window
    fn draw_border(&self, color: u32) -> Result<(), &'static str> {
        let buffer_ref = match SCREEN_FRAME_BUFFER.try() {
            Some(buffer) => buffer,
            None => return Err("Fail to get the virtual frame buffer"),
        };
        let mut buffer_lock = buffer_ref.lock();
        let buffer = buffer_lock.deref_mut();
        draw_rectangle(buffer, self.x, self.y, self.width, self.height, color);
        FRAME_COMPOSITOR.lock().compose(vec![(buffer, 0, 0)])
    }

    // adjust the size of a window
    fn resize(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    ) -> Result<(usize, usize), &'static str> {
        self.draw_border(SCREEN_BACKGROUND_COLOR)?;
        let percent = (
            (width - self.padding) * 100 / (self.width - self.padding),
            (height - self.padding) * 100 / (self.height - self.padding),
        );
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height;
        self.draw_border(get_border_color(self.active))?;
        Ok(percent)
    }

    // get the size of content without padding
    fn get_content_size(&self) -> (usize, usize) {
        (
            self.width - 2 * self.padding,
            self.height - 2 * self.padding,
        )
    }

    // get the position of content without padding
    fn get_content_position(&self) -> (usize, usize) {
        (self.x + self.padding, self.y + self.padding)
    }

    fn key_producer(&mut self) -> &mut DFQueueProducer<Event> {
        &mut self.key_producer
    }

}

// a component contains a displayable and its position
pub struct Component {
    x: usize,
    y: usize,
    displayable: TextDisplay,
}

impl Component {
    // get the displayable
    fn get_displayable(&self) -> &TextDisplay {
        return &(self.displayable);
    }

    // get the displayable
    fn get_displayable_mut(&mut self) -> &mut TextDisplay {
        return &mut (self.displayable);
    }

    // get the position of the displayable
    fn get_position(&self) -> (usize, usize) {
        (self.x, self.y)
    }

    // resize the displayable
    fn resize(&mut self, x: usize, y: usize, width: usize, height: usize) {
        self.x = x;
        self.y = y;
        self.displayable.resize(width, height);
    }
}

// Gets the border color according to the active state
fn get_border_color(active: bool) -> u32 {
    if active {
        WINDOW_ACTIVE_COLOR
    } else {
        WINDOW_INACTIVE_COLOR
    }
}

/// Lets the caller specify the dimensions of the new window and returns a new window
/// Params x,y specify the (x,y) coordinates of the top left corner of the window
/// Params width and height specify dimenions of new window in pixels
pub fn new_window<'a>(
    x: usize,
    y: usize,
    width: usize,
    height: usize,
) -> Result<WindowObj, &'static str> {
    // Check the size of the window
    if width < 2 * WINDOW_PADDING || height < 2 * WINDOW_PADDING {
        return Err("Window size must be greater than the padding");
    }
    // Init the key input producer and consumer
    let consumer = DFQueue::new().into_consumer();
    let producer = consumer.obtain_producer();
    // Init the frame buffer of the window
    let framebuffer = FrameBuffer::new(
        width - 2 * WINDOW_PADDING,
        height - 2 * WINDOW_PADDING,
        None,
    )?;
    let inner = WindowInner {
        x: x,
        y: y,
        width: width,
        height: height,
        active: true,
        padding: WINDOW_PADDING,
        key_producer: producer,
    };

    // // Check if the window overlaps with others
    // let inner_ref = Arc::new(Mutex::new(inner));
    // let overlapped = self.check_overlap(&inner_ref, x, y, width, height);
    // if overlapped  {
    //     return Err("Request area is already allocated");
    // }

    let inner_obj:Box<Window> = Box::new(inner);
    let inner_ref = Arc::new(Mutex::new(inner_obj));

    // add the new window and active it
    // initialize the content of the new window
    inner_ref.lock().clean()?; 
    window_manager::WINDOWLIST.lock().add_active(&inner_ref)?;

    // return the window object
    let window: WindowObj = WindowObj {
        inner: inner_ref,
        //text_buffer:FrameTextBuffer::new(),
        consumer: consumer,
        components: BTreeMap::new(),
        framebuffer: framebuffer,
    };

    Ok(window)
}

/// Applications call this function to request a new window object with a default size (mostly fills screen with WINDOW_MARGIN around all borders)
/// If the caller a specific window size, it should call new_window()
pub fn new_default_window() -> Result<WindowObj, &'static str> {
    let (window_width, window_height) = frame_buffer::get_screen_size()?;
    match new_window(
        WINDOW_MARGIN,
        WINDOW_MARGIN,
        window_width - 2 * WINDOW_MARGIN,
        window_height - 2 * WINDOW_MARGIN,
    ) {
        Ok(new_window) => return Ok(new_window),
        Err(err) => return Err(err),
    }
}

// delete the reference of a window in the manager when the window is dropped
impl Drop for WindowObj {
    fn drop(&mut self) {
        let mut window_list = WINDOWLIST.lock();

        // Switches to a new active window and sets
        // the active pointer field of the window allocator to the new active window
        match window_list.delete(&self.inner) {
            Ok(_) => {}
            Err(err) => error!("Fail to schedule to the next window: {}", err),
        };
    }
}