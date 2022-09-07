//! A `Window` object should be owned by an application. It can display a `Displayable` object in its framebuffer. See `applications/new_window` as a demo to use this library.
//!
//! This library will create a window with default title bar and border. It handles the commonly used interactions like moving
//! the window or close the window. Also, it is responsible to show title bar differently when window is active. 
//!
//! A window can render itself to the screen via a window manager. The window manager will compute the bounding box of the updated part and composites it with other existing windows according to their order.
//!
//! The library
//! frees applications from handling the complicated interaction with window manager, however, advanced users could learn from
//! this library about how to use window manager APIs directly.
//!

#![no_std]
extern crate alloc;
extern crate mpmc;
extern crate event_types;
extern crate spin;
#[macro_use]
extern crate log;
extern crate owning_ref;
extern crate framebuffer;
extern crate framebuffer_drawer;
extern crate mouse;
extern crate window_events;
extern crate shapes;
extern crate color;
extern crate scheduler;

use alloc::sync::Arc;
use mpmc::Queue;
use owning_ref::{MutexGuardRef, MutexGuardRefMut};
use framebuffer::{Framebuffer, AlphaPixel};
use color::{Color};
use shapes::{Coord, Rectangle};
use spin::Mutex;
use window_events::{register_window,WindowToWmEvent,WmToWindowEvent};
use event_types::Event;

/// This struct is the application-facing representation of a window.
/// 
pub struct Window {
    framebuffer: Arc<Mutex<Framebuffer<AlphaPixel>>>,
    /// The event queues
    towm: Queue<WindowToWmEvent>,
    fromwm: Queue<WmToWindowEvent>,
    /// record last result of whether this window is active, to reduce redraw overhead
    last_is_active: bool,
}

impl Window {
    /// Creates a new window to be displayed on screen. 
    /// 
    /// The given `framebuffer` will be filled with the `initial_background` color.
    /// 
    /// The newly-created `Window` will be set as the "active" window that has current focus. 
    /// 
    /// # Arguments: 
    /// * `coordinate`: the position of the window relative to the top-left corner of the screen.
    /// * `width`, `height`: the dimensions of the window in pixels.
    /// * `initial_background`: the default color of the window.
    pub fn new(
        final_framebuffer: &mut Framebuffer<AlphaPixel>, key_consumer: Queue<KeyEvent>, mouse_consumer: Queue<MouseEvent>
    ) -> Result<Window, &'static str> {

/*
        let towm = Queue::with_capacity(1000);
        let fromwm = Queue::with_capacity(100);
        register_window(towm.clone(), fromwm.clone())?;
        towm.push(WindowToWmEvent::AskSize).map_err(|_| "Can't push to wm")?;
        let (width, height) = loop {
            if let Some(WmToWindowEvent::TellSize(wh)) = fromwm.pop(){
                break wh;
            } else{
                debug!("Waiting for TellSize");
                scheduler::schedule();
            }
        };

        debug!("Ask {} {}", width, height);
        // Create a new virtual framebuffer to hold this window's contents only,
        // and fill it with the initial background color.
        let mut framebuffer = Framebuffer::new(width, height, None)?;
        framebuffer.fill(initial_background.into());
        let (width, height) = framebuffer.get_size();
        debug!("Get {} {}", width, height);
*/
        let window = Window {
            framebuffer: final_framebuffer,
            key_consumer,
            mouse_consumer,
            last_is_active: true, // new window is now set as the active window by default 
        };

        // Currently, refresh the whole screen instead of just the new window's bounds
        // wm.refresh_bottom_windows(Some(window_bounding_box), true)?;
        //wm.refresh_bottom_windows(Option::<Rectangle>::None, true)?;
        
        Ok(window)
    }

    pub fn is_active(&self)-> bool{
        self.last_is_active
    }

    pub fn render(&mut self, bounding_box: Option<Rectangle>) -> Result<(), &'static str> {

        // Induced bug rendering attempting to access out of bound memory
        #[cfg(downtime_eval)]
        {
            if bounding_box.unwrap().top_left == Coord::new(150,150) {
                unsafe { *(0x5050DEADBEEF as *mut usize) = 0x5555_5555_5555; }
            }
        }
        //debug!("Rendering Window");
        self.towm.push(WindowToWmEvent::Render(self.framebuffer.clone(), bounding_box)).map_err(|_| "Can't push to wm")
    }

    /// Returns an immutable reference to this window's virtual `Framebuffer`.
    pub fn framebuffer(&self) -> &Arc<Mutex<Framebuffer<AlphaPixel>>> {
        &self.framebuffer
    }

    /// Returns a mutable reference to this window's virtual `Framebuffer`.
    pub fn framebuffer_mut(&mut self) -> &mut Arc<Mutex<Framebuffer<AlphaPixel>>> {
        &mut self.framebuffer
    }

    pub fn area(&self) -> Rectangle {
        let size = self.framebuffer.lock().get_size();
        Rectangle{top_left: Coord{x:0, y:0}, bottom_right: Coord{x: size.0 as isize, y: size.1  as isize}}
    }

    pub fn handle_event(&mut self) -> Result<Option<Event>, &'static str> {
        //debug!("reading events from wm");
        Ok(self.fromwm.pop().map(|x| match x{
            WmToWindowEvent::TellSize(size) => {
                let area = Rectangle{top_left: Coord{x:0, y:0}, bottom_right: Coord{x: size.0 as isize, y: size.1  as isize}};
                Event::WindowResizeEvent(area)
            }
            WmToWindowEvent::KeyboardEvent(e) => Event::new_keyboard_event(e),
            WmToWindowEvent::MouseEvent(e) => Event::MouseMovementEvent(e),
        }))
    }
}

impl Drop for Window{
    fn drop(&mut self){

    }
}
