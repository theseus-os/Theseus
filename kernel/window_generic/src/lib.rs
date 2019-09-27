//! This crate defines a `WindowGeneric` structure. This structure contains a `WindowInner` structure which implements the `Window` trait.
//!
//! The `new_window` function creates a new `WindowGeneric` object and adds its inner object to the window manager. The outer `WindowGeneric` object will be returned to the application who creates the window.
//!
//! When a window is dropped, its inner object will be deleted from the window manager.

#![no_std]

extern crate spin;
#[macro_use]
extern crate alloc;
extern crate dfqueue;
extern crate event_types;
#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;
extern crate compositor;
extern crate displayable;
extern crate frame_buffer;
extern crate frame_buffer_compositor;
extern crate frame_buffer_drawer;
extern crate frame_buffer_printer;
extern crate frame_buffer_rgb;
extern crate text_display;
extern crate window;
extern crate window_manager;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::{Arc, Weak};
use alloc::collections::VecDeque;
use compositor::Compositor;
use core::ops::{Deref, DerefMut};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use displayable::Displayable;
use event_types::Event;
use frame_buffer::FrameBuffer;
use frame_buffer_compositor::FRAME_COMPOSITOR;
use frame_buffer_drawer::*;
use frame_buffer_rgb::FrameBufferRGB;
use spin::Mutex;
use text_display::{Cursor, TextDisplay};
use window::Window;
use window_manager::{
    SCREEN_BACKGROUND_COLOR, DESKTOP_FRAME_BUFFER, WINDOW_ACTIVE_COLOR,
    WINDOW_INACTIVE_COLOR, WINDOW_MARGIN, WINDOW_PADDING, WindowList
};

lazy_static! {
    /// The list of all windows in the system.
    pub static ref WINDOWLIST: Mutex<WindowList<WindowInner>> = Mutex::new(
        WindowList{
            background_list: VecDeque::new(),
            active: Weak::new(),
        }
    );
}

/// A window contains a reference to its inner reference owned by the window manager,
/// a consumer of inputs, a list of displayables and a framebuffer.
pub struct WindowGeneric<Buffer: FrameBuffer> {
    /// The inner object of the window.
    pub inner: Arc<Mutex<WindowInner>>,
    /// The key input consumer.
    pub consumer: DFQueueConsumer<Event>,
    /// The components in the window.
    pub components: BTreeMap<String, Component>,
    /// The framebuffer owned by the window.
    pub framebuffer: Buffer,
}

impl<Buffer: FrameBuffer> WindowGeneric<Buffer> {
    /// Cleans the content of a window. The border and padding of the window remain showing.
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
    /// as a tuple of `(width, height)`. It does not include the padding.
    pub fn dimensions(&self) -> (usize, usize) {
        let inner_locked = self.inner.lock();
        inner_locked.get_content_size()
    }

    /// Adds a new displayable to the window.
    /// This function checks if the displayable is in the window, but does not check if it is overlapped with others.
    pub fn add_displayable(
        &mut self,
        key: &str,
        x: usize,
        y: usize,
        displayable: Box<dyn Displayable>,
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

    /// Removes a displayable by its name.
    pub fn remove_displayable(&mut self, name: &str) {
        self.components.remove(name);
    }

    /// Gets a mutable reference to a displayable by its name.
    pub fn get_displayable_mut(&mut self, name: &str) -> Option<&mut Box<dyn Displayable>> {
        let opt = self.components.get_mut(name);
        match opt {
            None => return None,
            Some(component) => {
                return Some(component.get_displayable_mut());
            }
        };
    }

    /// Gets a displayable by its name.
    pub fn get_displayable(&self, name: &str) -> Option<&Box<dyn Displayable>> {
        let opt = self.components.get(name);
        match opt {
            None => return None,
            Some(component) => {
                return Some(component.get_displayable());
            }
        };
    }

    /// Gets the position of a displayable in the window.
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

    /// Gets the content position of the window excluding border and padding.
    pub fn get_content_position(&self) -> (usize, usize) {
        self.inner.lock().get_content_position()
    }

    /// Renders the content of the window to the screen.
    pub fn render(&mut self) -> Result<(), &'static str> {
        let (window_x, window_y) = { self.inner.lock().get_content_position() };
        FRAME_COMPOSITOR.lock().compose(vec![(
            &mut self.framebuffer,
            window_x as i32,
            window_y as i32,
        )])
    }

    /// Prints a string in the window with a text displayable by its name.
    pub fn display_string(&mut self, display_name: &str, slice: &str) -> Result<(), &'static str> {
        let component = self.components.get_mut(display_name).ok_or("")?;
        let (x, y) = component.get_position();
        let displayable = component.get_displayable_mut();

        /* Optimization: if current string is the prefix of the new string, just print the appended characters. */

        if let Some(text_display) = displayable.downcast_mut::<TextDisplay>() {
            text_display.set_text(slice);
            text_display.display(x, y, &mut self.framebuffer)?;
            self.render()?;
        } else {
            return Err("The displayable is not a text displayable");
        }

        Ok(())
    }

    /// Displays a cursor in the window with a text displayable by its name.
    pub fn display_end_cursor(
        &mut self,
        cursor: &mut Cursor,
        display_name: &str,
    ) -> Result<(), &'static str> {
        let component = self.components.get_mut(display_name).ok_or("")?;
        let (x, y) = component.get_position();
        let displayable = component.get_displayable_mut();

        if let Some(text_display) = displayable.downcast_mut::<TextDisplay>() {
            let (col, line) = text_display.get_next_pos();
            text_display.display_cursor(cursor, x, y, col, line, &mut self.framebuffer);
            self.render()?;
        } else {
            return Err("The displayable is not a text displayable");
        }

        Ok(())
    }

    // @Andrew
    /// Resizes a window as (width, height) at (x, y).
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

    /// Gets a key event of the window.
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

/// Creates a new window. Currently the window is of `FrameBufferRGB`. In the future we will be able to create a window of any structure which implements `FrameBuffer`.
/// (x, y) specify the coordinates of the top left corner of the window.
/// (width, height) specify the size of the new window.
pub fn new_window<'a>(
    x: usize,
    y: usize,
    width: usize,
    height: usize,
) -> Result<WindowGeneric<FrameBufferRGB>, &'static str> {
    // check the size of the window
    if width < 2 * WINDOW_PADDING || height < 2 * WINDOW_PADDING {
        return Err("Window size must be greater than the padding");
    }
    // init the key input producer and consumer
    let consumer = DFQueue::new().into_consumer();
    let producer = consumer.obtain_producer();
    // init the frame buffer of the window
    let framebuffer = FrameBufferRGB::new(
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

    let inner_ref = Arc::new(Mutex::new(inner));

    // add the new window and active it
    // initialize the content of the new window
    inner_ref.lock().clean()?;
    WINDOWLIST.lock().add_active(&inner_ref)?;

    // return the window object
    let window: WindowGeneric<FrameBufferRGB> = WindowGeneric {
        inner: inner_ref,
        consumer: consumer,
        components: BTreeMap::new(),
        framebuffer: framebuffer,
    };

    Ok(window)
}

/// Applications call this function to request a new window object with a default size (mostly fills screen with WINDOW_MARGIN around all borders).
pub fn new_default_window() -> Result<WindowGeneric<FrameBufferRGB>, &'static str> {
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

/// The structure is owned by the window manager. It contains the information of a window but under the control of the manager
pub struct WindowInner {
    /// the upper left x-coordinate of the window
    pub x: usize,
    /// the upper left y-coordinate of the window
    pub y: usize,
    /// the width of the window
    pub width: usize,
    /// the height of the window
    pub height: usize,
    /// whether the window is active
    pub active: bool,
    /// the padding outside the content of the window including the border.
    pub padding: usize,
    /// the producer accepting a key event
    pub key_producer: DFQueueProducer<Event>,
}

impl Window for WindowInner {
    fn clean(&self) -> Result<(), &'static str> {
        let buffer_ref = match DESKTOP_FRAME_BUFFER.try() {
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

    fn check_in_content(&self, x: usize, y: usize) -> bool {
        return x <= self.width - 2 * self.padding && y <= self.height - 2 * self.padding;
    }

    fn active(&mut self, active: bool) -> Result<(), &'static str> {
        self.active = active;
        self.draw_border(WINDOW_ACTIVE_COLOR)?;
        Ok(())
    }

    fn draw_border(&self, color: u32) -> Result<(), &'static str> {
        let buffer_ref = match DESKTOP_FRAME_BUFFER.try() {
            Some(buffer) => buffer,
            None => return Err("Fail to get the virtual frame buffer"),
        };
        let mut buffer_lock = buffer_ref.lock();
        let buffer = buffer_lock.deref_mut();
        draw_rectangle(buffer, self.x, self.y, self.width, self.height, color);
        FRAME_COMPOSITOR.lock().compose(vec![(buffer, 0, 0)])
    }

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

    fn get_content_size(&self) -> (usize, usize) {
        (
            self.width - 2 * self.padding,
            self.height - 2 * self.padding,
        )
    }

    fn get_content_position(&self) -> (usize, usize) {
        (self.x + self.padding, self.y + self.padding)
    }

    fn key_producer(&mut self) -> &mut DFQueueProducer<Event> {
        &mut self.key_producer
    }
}

/// A component contains a displayable and its position.
pub struct Component {
    x: usize,
    y: usize,
    displayable: Box<dyn Displayable>,
}

impl Component {
    // gets the displayable
    fn get_displayable(&self) -> &Box<dyn Displayable> {
        return &(self.displayable);
    }

    // gets a mutable reference to the displayable
    fn get_displayable_mut(&mut self) -> &mut Box<dyn Displayable> {
        return &mut (self.displayable);
    }

    // gets the position of the displayable
    fn get_position(&self) -> (usize, usize) {
        (self.x, self.y)
    }

    // resizes the displayable
    fn resize(&mut self, x: usize, y: usize, width: usize, height: usize) {
        self.x = x;
        self.y = y;
        self.displayable.resize(width, height);
    }
}

// gets the border color according to the active state
fn get_border_color(active: bool) -> u32 {
    if active {
        WINDOW_ACTIVE_COLOR
    } else {
        WINDOW_INACTIVE_COLOR
    }
}

// Use a lazy drop scheme instead since window_generic cannot get access to the window_manager. When the window is dropped, the corresponding weak reference will be deleted from the window manager the next time the manager tries to get access to it.
// delete the reference of a window in the manager when a window is dropped.
/*impl<Buffer: FrameBuffer> Drop for WindowGeneric<Buffer> {
    fn drop(&mut self) {
        let mut window_list = WINDOWLIST.lock();

        // Switches to a new active window and sets
        // the active pointer field of the window allocator to the new active window
        match window_list.delete(&self.inner) {
            Ok(_) => {}
            Err(err) => error!("Fail to schedule to the next window: {}", err),
        };
    }
}*/
