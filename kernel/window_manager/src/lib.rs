//! This crate acts as a manager of a list of windows. It defines a `WindowGeneric` structure. The structure contains a `WindowProfile` structure which implements the `Window` trait.
//!
//! The `WindowProfile` structure wraps information required by the window manager including the border, location and size of a window. The manager holds a `WindowList` instance which maintains a list of references to `WindowProfile`s. It can add new windows to the list and switch among them.
//!
//! The `WindowGeneric` structure consists of its profile, components, framebuffer and events consumer. An application invokes the `new_window` function to create a `WindowGeneric` object and get a reference to it. The window manager would add the profile to the window list in creating a new `WindowGeneric` object and the profile would be deleted when the object is dropped.
//!
//! An application can create displayables, add them to its window, and display them by their names. A displayable usually acts as a component of a window and can display itself in the window. For example, a text displayable is a block of text which can display with specific color and font in a window.

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
extern crate window_list;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::{Arc, Weak};
use alloc::collections::VecDeque;
use alloc::vec::{IntoIter};
use compositor::Compositor;
use core::ops::{Deref, DerefMut};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use displayable::Displayable;
use event_types::Event;
use frame_buffer::{FrameBuffer, Coord, Pixel};
use frame_buffer_compositor::{FRAME_COMPOSITOR, FrameBufferBlocks};
use frame_buffer_drawer::*;
use frame_buffer_rgb::FrameBufferRGB;
use spin::{Mutex, Once};
use window::{Window, WindowProfile};
pub use window_list::{
    SCREEN_BACKGROUND_COLOR, WINDOW_ACTIVE_COLOR,
    WINDOW_INACTIVE_COLOR, WINDOW_MARGIN, WINDOW_PADDING, WindowList
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
    /// A window manager which maintains a list of window profiles.
    pub static ref WINDOWLIST: Mutex<WindowList<WindowProfileGeneric>> = Mutex::new(
        WindowList{
            background_list: VecDeque::new(),
            active: Weak::new(),
        }
    );
}

/// A window contains a reference to its inner reference owned by the window manager,
/// a consumer of inputs, a list of displayables and a framebuffer.
pub struct WindowGeneric<Buffer: FrameBuffer> {
    /// The profile object of the window.
    pub profile: Arc<Mutex<WindowProfileGeneric>>,
    /// The key input consumer.
    pub consumer: DFQueueConsumer<Event>,
    /// The components in the window.
    pub components: BTreeMap<String, Component>,
    /// The framebuffer owned by the window.
    pub framebuffer: Buffer,
}

impl<Buffer: FrameBuffer> Window for WindowGeneric<Buffer> {

    fn consumer(&mut self) -> &mut DFQueueConsumer<Event> {
        &mut self.consumer
    }

    fn framebuffer(&mut self) -> Option<&mut dyn FrameBuffer> {
        Some(&mut self.framebuffer)
    }

    fn get_background(&self) -> Pixel { SCREEN_BACKGROUND_COLOR }

    fn get_displayable_mut(&mut self, display_name: &str) -> Result<&mut Box<dyn Displayable>, &'static str>{
        let component = self.components.get_mut(display_name).ok_or("The displayable does not exist")?;
        Ok(component.get_displayable_mut())
    }

    fn get_displayable(&self, display_name: &str) -> Result<&Box<dyn Displayable>, &'static str>{
        let component = self.components.get(display_name).ok_or("The displayable does not exist")?;
        Ok(component.get_displayable())
    }

    /// Gets the position of a displayable relative to the window.
    fn get_displayable_position(&self, key: &str) -> Result<Coord, &'static str> {
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
        /// Display a displayable in the window by its name.
    fn display(&mut self, display_name: &str) -> Result<(), &'static str> {
        let component = self.components.get_mut(display_name).ok_or("")?;
        let coordinate = component.get_position();
        let displayable = component.get_displayable_mut();
        let blocks = displayable.display_in(
            coordinate, 
            &mut self.framebuffer
        )?;
        self.render(Some(blocks.into_iter()))
    }

    /// Adds a new displayable at `coordinate` relative to the top-left corner of the window.
    fn add_displayable(
        &mut self,
        key: &str,
        coordinate: Coord,
        displayable: Box<dyn Displayable>,
    ) -> Result<(), &'static str> {
        let key = key.to_string();

        /*let (width, height) = displayable.get_size();
        let profile = self.profile.lock();
        if !profile.contains(coordinate)
            || !profile.contains(coordinate + (width as isize, 0))
            || !profile.contains(coordinate + (0, height as isize))
            || !profile.contains(coordinate + (width as isize, height as isize))
        {
            return Err("The displayable does not fit the window size.");
        }*/

        let component = Component {
            coordinate: coordinate,
            displayable: displayable,
        };

        self.components.insert(key, component);
        Ok(())
    }

    fn handle_event(&mut self) -> Result<(), &'static str> {
        //wenqiu: Todo
        Ok(())
    }

        /// Renders the content of the window to the screen.
    /// `blocks` is the information of updated blocks in the form (block_index, block_width). If `blocks` is `None`, the whole window would be refreshed.
    /// The use of `blocks` is described in the `frame_buffer_compositor` crate. 
    fn render(&mut self, blocks: Option<IntoIter<(usize, usize)>>) -> Result<(), &'static str> {
        let coordinate = { self.profile.lock().get_content_position() };
        let frame_buffer_blocks = FrameBufferBlocks {
            framebuffer: &mut self.framebuffer,
            coordinate: coordinate,
            blocks: blocks
        };
        FRAME_COMPOSITOR.lock().composite(vec![frame_buffer_blocks].into_iter())
    }
}

impl<Buffer: FrameBuffer> WindowGeneric<Buffer> {
    /// Clears the content of a window. The border and padding of the window remain showing.
    pub fn clear(&mut self) -> Result<(), &'static str> {
        let (width, height) = self.profile.lock().get_content_size();
        fill_rectangle(
            &mut self.framebuffer,
            Coord::new(0, 0),
            width,
            height,
            SCREEN_BACKGROUND_COLOR,
        );
        self.render(None)
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
    
        /// Returns the content dimensions of this window,
    /// as a tuple of `(width, height)`. It does not include the padding.
    pub fn dimensions(&self) -> (usize, usize) {
        let profile_locked = self.profile.lock();
        profile_locked.get_content_size()
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

    /// Gets the content position relative to the window excluding border and padding relative.
    pub fn get_content_position(&self) -> Coord {
        self.profile.lock().get_content_position()
    }

    // @Andrew
    /// Resizes a window as (width, height) at coordinate relative to the top-left corner of the screen.
    pub fn resize(
        &mut self,
        coordinate: Coord,
        width: usize,
        height: usize,
    ) -> Result<(), &'static str> {
        // checks for overlap
        // {
        //     let inner = self.inner.clone();
        //     let mut allocator = WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")?.lock();
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
                    let coordinate = item.get_position();
                    let (width, height) = item.get_displayable().get_size();
                    item.resize(
                        Coord::new(
                            coordinate.x * percent.0 as isize / 100,
                            coordinate.y * percent.1 as isize / 100
                        ),
                        width * percent.0 / 100,
                        height * percent.1 / 100,
                    );
                }
                profile.events_producer().enqueue(Event::new_resize_event(coordinate, width, height));
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
/// `coordinate` specifies the coordinate of the window relative to the top-left corner of the screen.
/// (width, height) specify the size of the new window.
pub fn new_window(
    coordinate: Coord,
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
    let profile = WindowProfileGeneric {
        coordinate: coordinate,
        width: width,
        height: height,
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
    WINDOWLIST.lock().add_active(&profile_ref)?;

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
    new_window(
        Coord::new(WINDOW_MARGIN as isize, WINDOW_MARGIN as isize),
        window_width - 2 * WINDOW_MARGIN,
        window_height - 2 * WINDOW_MARGIN,
    )
}

/// The structure is owned by the window manager. It contains the information of a window but under the control of the manager
pub struct WindowProfileGeneric {
    /// the top-left corner of window relative to the top-left corner of the screen
    pub coordinate: Coord,
    /// the width of the window
    pub width: usize,
    /// the height of the window
    pub height: usize,
    /// the padding outside the content of the window including the border.
    pub padding: usize,
    /// the producer accepting an event, i.e. a keypress event, resize event, etc.
    pub events_producer: DFQueueProducer<Event>,
}

impl WindowProfile for WindowProfileGeneric {
    fn clear(&mut self) -> Result<(), &'static str> {
        let buffer_ref = match DESKTOP_FRAME_BUFFER.try() {
            Some(buffer) => buffer,
            None => return Err("Fail to get the virtual frame buffer"),
        };
        let mut buffer_lock = buffer_ref.lock();
        let buffer = buffer_lock.deref_mut();
        draw_rectangle(
            buffer,
            self.coordinate,
            self.width,
            self.height,
            SCREEN_BACKGROUND_COLOR,
        );
        let frame_buffer_blocks = FrameBufferBlocks {
            framebuffer: buffer,
            coordinate: Coord { x: 0, y: 0 },
            blocks: None
        };
        FRAME_COMPOSITOR.lock().composite(vec![frame_buffer_blocks].into_iter())
    }

    fn contains(&self, coordinate: Coord) -> bool {
        return coordinate.x <= (self.width - 2 * self.padding) as isize && coordinate.y <= (self.height - 2 * self.padding) as isize;
    }

/*    fn set_active(&mut self, active: bool) -> Result<(), &'static str> {
        self.active = active;
        Ok(())
    }*/

    fn draw_border(&self, color: u32) -> Result<(), &'static str> {
        let buffer_ref = match DESKTOP_FRAME_BUFFER.try() {
            Some(buffer) => buffer,
            None => return Err("Fail to get the virtual frame buffer"),
        };
        let mut buffer_lock = buffer_ref.lock();
        let buffer = buffer_lock.deref_mut();
        draw_rectangle(buffer, self.coordinate, self.width, self.height, color);
        let frame_buffer_blocks = FrameBufferBlocks {
            framebuffer: buffer,
            coordinate: Coord { x: 0, y: 0 },
            blocks: None
        };        
        FRAME_COMPOSITOR.lock().composite(vec![frame_buffer_blocks].into_iter())
    }

    fn resize(
        &mut self,
        coordinate: Coord,
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
        self.draw_border(WINDOW_ACTIVE_COLOR)?;
        Ok(percent)
    }

    fn get_content_size(&self) -> (usize, usize) {
        (
            self.width - 2 * self.padding,
            self.height - 2 * self.padding,
        )
    }

    fn get_content_position(&self) -> Coord {
        self.coordinate + (self.padding as isize, self.padding as isize)
    }

    fn events_producer(&mut self) -> &mut DFQueueProducer<Event> {
        &mut self.events_producer
    }
    
    fn set_position(&mut self, coordinate: Coord) {
        self.coordinate = coordinate;       
    }

    fn get_moving_base(&self) -> Coord {
        Coord::new(0, 0)
    }

    fn is_moving(&self) -> bool {
        false
    }

    fn set_give_all_mouse_event(&mut self, flag: bool) {
        // TODO
    }

    fn give_all_mouse_event(&mut self) -> bool {
        false
    }

    fn set_moving_base(&mut self, coordinate: Coord) {
        // TODO
    }

    fn set_is_moving(&mut self, moving: bool) {
        // TODO
    }
}

/// A component contains a displayable and its coordinate relative to the top-left corner of the window.
pub struct Component {
    coordinate: Coord,
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

    // gets the coordinate of the displayable relative to the top-left corner of the window
    fn get_position(&self) -> Coord {
        self.coordinate
    }

    // resizes the displayable as (width, height) at `coordinate` relative to the top-left corner of the window
    fn resize(&mut self, coordinate: Coord, width: usize, height: usize) {
        self.coordinate = coordinate;
        self.displayable.resize(width, height);
    }
}


// Use a lazy drop scheme instead since window_manager cannot get access to the window_manager. When the window is dropped, the corresponding weak reference will be deleted from the window manager the next time the manager tries to get access to it.
// delete the reference of a window in the manager when a window is dropped.
/*impl<Buffer: FrameBuffer> Drop for WindowGeneric<Buffer> {
    fn drop(&mut self) {
        let mut window_list = window_manager.lock();

        // Switches to a new active window and sets
        // the active pointer field of the window allocator to the new active window
        match window_list.delete(&self.inner) {
            Ok(_) => {}
            Err(err) => error!("Fail to schedule to the next window: {}", err),
        };
    }
}*/
