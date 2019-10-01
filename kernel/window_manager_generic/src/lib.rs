//! This crate defines a `WindowGeneric` structure. This structure contains a `WindowProfile` structure which implements the `Window` trait.
//!
//! This crate holds a instance of a window manager which maintains a list of `WindowProfile`s.
//!
//! The `new_window` function creates a new `WindowGeneric` object and adds its profile object to the window manager. The outer `WindowGeneric` object will be returned to the application who creates the window.
//!
//! When a window is dropped, its profile object will be deleted from the window manager.

#![no_std]

extern crate spin;
#[macro_use]
extern crate alloc;
extern crate dfqueue;
extern crate event_types;
#[macro_use]
extern crate lazy_static;
extern crate compositor;
extern crate displayable;
extern crate frame_buffer;
extern crate frame_buffer_compositor;
extern crate frame_buffer_drawer;
extern crate frame_buffer_printer;
extern crate frame_buffer_rgb;
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
use frame_buffer::{FrameBuffer, RelativeCoord, AbsoluteCoord, ICoord};
use frame_buffer_compositor::FRAME_COMPOSITOR;
use frame_buffer_drawer::*;
use frame_buffer_rgb::FrameBufferRGB;
use spin::{Mutex, Once};
use window::Window;
use window_manager::{
    SCREEN_BACKGROUND_COLOR, WINDOW_ACTIVE_COLOR,
    WINDOW_INACTIVE_COLOR, WINDOW_MARGIN, WINDOW_PADDING, WindowManager
};

/// A framebuffer owned by the window manager.
/// This framebuffer is responsible for displaying borders and gaps between windows. Windows owned by applications cannot get access to their borders.
/// All the display behaviors of borders are controled by the window manager.
pub static DESKTOP_FRAME_BUFFER: Once<Arc<Mutex<FrameBufferRGB>>> = Once::new();

/// Initializes the window manager. 
/// Currently the framebuffer is of type `FrameBufferRGB`. In the future we would be able to have window manager of different `FrameBuffer`s.
pub fn init() -> Result<(), &'static str> {
    let (screen_width, screen_height) = frame_buffer::get_screen_size()?;
    let framebuffer = FrameBufferRGB::new(screen_width, screen_height, None)?;
    DESKTOP_FRAME_BUFFER.call_once(|| Arc::new(Mutex::new(framebuffer)));
    Ok(())
}

lazy_static! {
    /// A window manager which maintains a list of generic windows.
    pub static ref WINDOW_MANAGER_GENERIC: Mutex<WindowManager<WindowProfile>> = Mutex::new(
        WindowManager{
            background_list: VecDeque::new(),
            active: Weak::new(),
        }
    );
}

/// A window contains a reference to its inner reference owned by the window manager,
/// a consumer of inputs, a list of displayables and a framebuffer.
pub struct WindowGeneric<Buffer: FrameBuffer> {
    /// The profile object of the window.
    pub profile: Arc<Mutex<WindowProfile>>,
    /// The key input consumer.
    pub consumer: DFQueueConsumer<Event>,
    /// The components in the window.
    pub components: BTreeMap<String, Component>,
    /// The framebuffer owned by the window.
    pub framebuffer: Buffer,
}

impl<Buffer: FrameBuffer> WindowGeneric<Buffer> {
    /// Clears the content of a window. The border and padding of the window remain showing.
    pub fn clear(&mut self) -> Result<(), &'static str> {
        let (width, height) = self.profile.lock().get_content_size();
        fill_rectangle(
            &mut self.framebuffer,
            AbsoluteCoord::new(0, 0),
            width,
            height,
            SCREEN_BACKGROUND_COLOR,
        );
        self.render()
    }

    /// Returns the content dimensions of this window,
    /// as a tuple of `(width, height)`. It does not include the padding.
    pub fn dimensions(&self) -> (usize, usize) {
        let profile_locked = self.profile.lock();
        profile_locked.get_content_size()
    }

    /// Adds a new displayable at `coordinate` relative to the window.
    /// This function checks if the displayable is in the window, but does not check if it is overlapped with others.
    pub fn add_displayable(
        &mut self,
        key: &str,
        coordinate: RelativeCoord,
        displayable: Box<dyn Displayable>,
    ) -> Result<(), &'static str> {
        let key = key.to_string();
        let (width, height) = displayable.get_size();
        let profile = self.profile.lock();

        if !profile.contains_coordinate(coordinate)
            || !profile.contains_coordinate(coordinate + (width, 0))
            || !profile.contains_coordinate(coordinate + (0, height))
            || !profile.contains_coordinate(coordinate + (width, height))
        {
            return Err("The displayable does not fit the window size.");
        }

        let component = Component {
            coordinate: coordinate,
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
        opt.map(|component| {
            component.get_displayable_mut()
        })
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

    /// Gets the position of a displayable relative to the window.
    pub fn get_displayable_position(&self, key: &str) -> Result<RelativeCoord, &'static str> {
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

    /// Gets the content position relative to the window excluding border and padding relative.
    pub fn get_content_position(&self) -> RelativeCoord {
        self.profile.lock().get_content_position()
    }

    /// Renders the content of the window to the screen.
    pub fn render(&mut self) -> Result<(), &'static str> {
        let (window_x, window_y) = { self.profile.lock().get_content_position().value() };
        let coordinate = ICoord {
            x: window_x as i32,
            y: window_y as i32,
        };
        FRAME_COMPOSITOR.lock().composite(vec![(
            &mut self.framebuffer,
            coordinate
        )])
    }

    /// Gets a reference to a displayable of type `T` which implements the `Displayable` trait by its name. Returns error if the displayable is not of type `T` or does not exist.
    pub fn get_concrete_display<T: Displayable>(&self, display_name: &str) -> Result<&T, &'static str> {
        if let Some(component) = self.components.get(display_name) {
            let displayable = component.get_displayable();
            if let Some(concrete_display) = displayable.downcast_ref::<T>() {
                return Ok(concrete_display)
            } else {
                return Err("The displayable is not of this type");
            }
        } else {
            return Err("The displayable does not exist");
        }
    }

    /// Gets a mutable reference to a displayable of type `T` which implements the `Displayable` trait by its name. Returns error if the displayable is not of type `T` or does not exist.
    pub fn get_concrete_display_mut<T: Displayable>(&mut self, display_name: &str) -> Result<&mut T, &'static str> {
        if let Some(component) = self.components.get_mut(display_name) {
            let displayable = component.get_displayable_mut();
            if let Some(concrete_display) = displayable.downcast_mut::<T>() {
                return Ok(concrete_display)
            } else {
                return Err("The displayable is not of this type");
            }
        } else {
            return Err("The displayable does not exist");
        }
    }


    /// Display a displayable in the window by its name.
    pub fn display(&mut self, display_name: &str) -> Result<(), &'static str> {
        let component = self.components.get_mut(display_name).ok_or("")?;
        let coordinate = component.get_position();
        let displayable = component.get_displayable_mut();
        displayable.display(
            AbsoluteCoord(coordinate.to_ucoord()), 
            &mut self.framebuffer
        )?;
        self.render()
    }

    // @Andrew
    /// Resizes a window as (width, height) at coordinate relative to the screen.
    pub fn resize(
        &mut self,
        coordinate: RelativeCoord,
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

        self.clear()?;
        let mut profile = self.profile.lock();
        match profile.resize(coordinate, width, height) {
            Ok(percent) => {
                for (_key, item) in self.components.iter_mut() {
                    let (x, y) = item.get_position().value();
                    let (width, height) = item.get_displayable().get_size();
                    item.resize(
                        RelativeCoord::new(
                            x * percent.0 / 100,
                            y * percent.1 / 100
                        ),
                        width * percent.0 / 100,
                        height * percent.1 / 100,
                    );
                }
                let (x, y) = coordinate.value();
                profile.events_producer().enqueue(Event::new_resize_event(x, y, width, height));
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    /// Gets a event of the window.
    pub fn get_event(&self) -> Option<Event> {
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
/// `coordinate` specifies the coordinate relative to the top left corner of the window.
/// (width, height) specify the size of the new window.
pub fn new_window(
    coordinate: RelativeCoord,
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
    let profile = WindowProfile {
        coordinate: coordinate,
        width: width,
        height: height,
        active: true,
        padding: WINDOW_PADDING,
        events_producer: producer,
    };

    // // Check if the window overlaps with others
    // let inner_ref = Arc::new(Mutex::new(inner));
    // let overlapped = self.check_overlap(&inner_ref, x, y, width, height);
    // if overlapped  {
    //     return Err("Request area is already allocated");
    // }

    let profile_ref = Arc::new(Mutex::new(profile));

    // add the new window and active it
    // initialize the content of the new window
    profile_ref.lock().clear()?;
    WINDOW_MANAGER_GENERIC.lock().add_active(&profile_ref)?;

    // return the window object
    let window: WindowGeneric<FrameBufferRGB> = WindowGeneric {
        profile: profile_ref,
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
        RelativeCoord::new(WINDOW_MARGIN, WINDOW_MARGIN),
        window_width - 2 * WINDOW_MARGIN,
        window_height - 2 * WINDOW_MARGIN,
    ) {
        Ok(new_window) => return Ok(new_window),
        Err(err) => return Err(err),
    }
}

/// The structure is owned by the window manager. It contains the information of a window but under the control of the manager
pub struct WindowProfile {
    /// the coordinate of the window relative to the screen
    pub coordinate: RelativeCoord,
    /// the width of the window
    pub width: usize,
    /// the height of the window
    pub height: usize,
    /// whether the window is active
    pub active: bool,
    /// the padding outside the content of the window including the border.
    pub padding: usize,
    /// the producer accepting an event, i.e. a keypress event, resize event, etc.
    pub events_producer: DFQueueProducer<Event>,
}

impl Window for WindowProfile {
    fn clear(&self) -> Result<(), &'static str> {
        let buffer_ref = match DESKTOP_FRAME_BUFFER.try() {
            Some(buffer) => buffer,
            None => return Err("Fail to get the virtual frame buffer"),
        };
        let mut buffer_lock = buffer_ref.lock();
        let buffer = buffer_lock.deref_mut();
        draw_rectangle(
            buffer,
            AbsoluteCoord(self.coordinate.to_ucoord()),
            self.width,
            self.height,
            SCREEN_BACKGROUND_COLOR,
        );
        let coordinate = ICoord { x: 0, y: 0 };
        FRAME_COMPOSITOR.lock().composite(vec![(buffer, coordinate)])
    }

    fn contains_coordinate(&self, point: RelativeCoord) -> bool {
        let (x, y) = point.value();
        return x <= self.width - 2 * self.padding && y <= self.height - 2 * self.padding;
    }

    fn set_active(&mut self, active: bool) -> Result<(), &'static str> {
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
        draw_rectangle(buffer, AbsoluteCoord(self.coordinate.to_ucoord()), self.width, self.height, color);
        let coordinate = ICoord { x: 0, y: 0 };
        FRAME_COMPOSITOR.lock().composite(vec![(buffer, coordinate)])
    }

    fn resize(
        &mut self,
        coordinate: RelativeCoord,
        width: usize,
        height: usize,
    ) -> Result<(usize, usize), &'static str> {
        self.draw_border(SCREEN_BACKGROUND_COLOR)?;
        let percent = (
            (width - self.padding) * 100 / (self.width - self.padding),
            (height - self.padding) * 100 / (self.height - self.padding),
        );
        self.coordinate = coordinate;
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

    fn get_content_position(&self) -> RelativeCoord {
        self.coordinate + (self.padding, self.padding)
    }

    fn events_producer(&mut self) -> &mut DFQueueProducer<Event> {
        &mut self.events_producer
    }
}

/// A component contains a displayable and its coordinate relative to the window.
pub struct Component {
    coordinate: RelativeCoord,
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

    // gets the coordinate of the displayable relative to the window
    fn get_position(&self) -> RelativeCoord {
        self.coordinate
    }

    // resizes the displayable as (width, height) at `coordinate` relative to the window
    fn resize(&mut self, coordinate: RelativeCoord, width: usize, height: usize) {
        self.coordinate = coordinate;
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

// Use a lazy drop scheme instead since window_manager_generic cannot get access to the window_manager. When the window is dropped, the corresponding weak reference will be deleted from the window manager the next time the manager tries to get access to it.
// delete the reference of a window in the manager when a window is dropped.
/*impl<Buffer: FrameBuffer> Drop for WindowGeneric<Buffer> {
    fn drop(&mut self) {
        let mut window_list = WINDOW_MANAGER_GENERIC.lock();

        // Switches to a new active window and sets
        // the active pointer field of the window allocator to the new active window
        match window_list.delete(&self.inner) {
            Ok(_) => {}
            Err(err) => error!("Fail to schedule to the next window: {}", err),
        };
    }
}*/
