//! Window manager that simulates a desktop environment.
//! The manager matains a list of background windows and an active window.
//! Once an active window is deleted or set as inactive, the next window in the background list will become active.
//! The order of windows is based on the last time it was active. The one which was active most recently is the top of the background list.
//!
//! The WINDOW_ALLOCATOR is used by the `WindowManager` itself to track and modify existing windows

#![no_std]

extern crate alloc;
extern crate event_types;
extern crate frame_buffer;
extern crate frame_buffer_printer;
extern crate frame_buffer_rgb;
extern crate spin;
#[macro_use]
extern crate lazy_static;
extern crate window;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::sync::{Arc, Weak};
use event_types::Event;
use frame_buffer_rgb::FrameBufferRGB;
use spin::{Mutex, Once};
pub use window::Window;

// 10 pixel gap between windows
pub const WINDOW_MARGIN: usize = 10;
// 2 pixel padding within a window
pub const WINDOW_PADDING: usize = 2;
// The border color of an active window
pub const WINDOW_ACTIVE_COLOR: u32 = 0xFFFFFF;
// The border color of an inactive window
pub const WINDOW_INACTIVE_COLOR: u32 = 0x343C37;
// The background color of the screen
pub const SCREEN_BACKGROUND_COLOR: u32 = 0x000000;

// A framebuffer owned by the window manager.
// This framebuffer is responsible for display borders and gaps between windows. Windows owned by applications cannot get access to their borders.
// All the display behaviors of borders are controled by the window manager.
pub static SCREEN_FRAME_BUFFER: Once<Arc<Mutex<FrameBufferRGB>>> = Once::new();

/// Initialize the window manager. 
/// Currently the framebuffer is of type `FrameBufferRGB`. In the future we would be able to have window manager of different FrameBuffers.
pub fn init() -> Result<(), &'static str> {
    let (screen_width, screen_height) = frame_buffer::get_screen_size()?;
    let framebuffer = FrameBufferRGB::new(screen_width, screen_height, None)?;
    SCREEN_FRAME_BUFFER.call_once(|| Arc::new(Mutex::new(framebuffer)));
    Ok(())
}

lazy_static! {
    /// The list of all windows in the system.
    pub static ref WINDOWLIST: Mutex<WindowList> = Mutex::new(
        WindowList{
            background_list: VecDeque::new(),
            active: Weak::new(),
        }
    );
}

/// The window list.
/// It contains a list of allocated window and a reference to the active window
pub struct WindowList {
    // The list of inactive windows. Their order is based on the last time they were active. The first window is the window which was active most recently.
    background_list: VecDeque<Weak<Mutex<Box<dyn Window>>>>,
    // A weak pointer to the active window.
    active: Weak<Mutex<Box<dyn Window>>>,
}

/// Puts an input event into the active window (i.e. a keypress event, resize event, etc.).
/// If the caller wants to put an event into a specific window, use put_event_into_app().
pub fn send_event_to_active(event: Event) -> Result<(), &'static str> {
    let window_list = WINDOWLIST.lock();
    let active_ref = window_list.active.upgrade(); // grabs a pointer to the active WindowInner
    if let Some(window) = active_ref {
        let mut window = window.lock();
        window.key_producer().enqueue(event);
    }
    Ok(())
}

impl WindowList {
    /// Adds and actives a new window to the list.
    pub fn add_active(
        &mut self,
        inner_ref: &Arc<Mutex<Box<dyn Window>>>,
    ) -> Result<(), &'static str> {
        // // inactive all other windows and active the new one
        // for item in self.list.iter_mut(){
        //     let ref_opt = item.upgrade();
        //     if let Some(reference) = ref_opt {
        //         reference.lock().active(false)?;
        //     }
        // }
        if let Some(current_active) = self.active.upgrade() {
            current_active.lock().active(false)?;
            let weak_ref = self.active.clone();
            self.background_list.push_front(weak_ref);
        }

        inner_ref.lock().active(true)?;
        self.active = Arc::downgrade(inner_ref);

        Ok(())
    }

    // Deletes a window.
    pub fn delete(&mut self, inner: &Arc<Mutex<Box<dyn Window>>>) -> Result<(), &'static str> {
        // If the window is active, delete it and active the next top window
        if let Some(current_active) = self.active.upgrade() {
            if Arc::ptr_eq(&(current_active), inner) {
                active_window(0, false)?;
                return Ok(());
            }
        }

        if let Some(index) = self.get_bgwindow_index(&inner) {
            {
                let window_ref = &self.background_list[index];
                let window = window_ref.upgrade();
                if let Some(window) = window {
                    window.lock().key_producer().enqueue(Event::ExitEvent);
                }
            }
            self.background_list.remove(index);
        }

        inner.lock().clean()?;

        Ok(())
    }

    // gets the index of an inactive window in the background window list.
    fn get_bgwindow_index(&self, inner: &Arc<Mutex<Box<dyn Window>>>) -> Option<usize> {
        let mut i = 0;
        for item in self.background_list.iter() {
            if let Some(item_ptr) = item.upgrade() {
                if Arc::ptr_eq(&(item_ptr), inner) {
                    break;
                }
            }
            i += 1;
        }

        if i < self.background_list.len() {
            return Some(i);
        } else {
            return None;
        }
    }

    // // check if an area specified by (x, y, width, height) overlaps with an existing window
    // fn check_overlap(&mut self, inner:&Arc<Mutex<WindowInner>>, x:usize, y:usize, width:usize, height:usize) -> bool {
    //     let mut len = self.allocated.len();
    //     let mut i = 0;
    //     while i < len {
    //         {
    //             let mut void = false;
    //             if let Some(reference) = self.allocated.get(i) {
    //                 if let Some(allocated_ref) = reference.upgrade() {
    //                     if !Arc::ptr_eq(&allocated_ref, inner) {
    //                         if allocated_ref.lock().is_overlapped(x, y, width, height) {
    //                             return true;
    //                         }
    //                     }
    //                     i += 1;
    //                 } else {
    //                     void = true;
    //                 }
    //             }
    //             if void {
    //                 self.list.remove(i);
    //                 len -= 1;
    //             }
    //         }
    //     }
    //     false
    // }

    // return a reference to the next window of current active window
    // fn next(&mut self) -> Option<Arc<Mutex<WindowInner>>> {
    //     // let mut current_active = false;
    //     // for item in self.list.iter_mut(){
    //     //     let reference = item.upgrade();
    //     //     if let Some(window) = reference {
    //     //         if window.lock().active {
    //     //             current_active = true;
    //     //         } else if current_active {
    //     //             return Some(window)
    //     //         }
    //     //     }
    //     // }

    //     // if current_active {
    //     //     for item in self.list.iter_mut(){
    //     //         let reference = item.upgrade();
    //     //         if let Some(window) = reference {
    //     //             return Some(window)
    //     //         }
    //     //     }
    //     // }
    //     if let Some(weak_ref) = self.list.pop_front() {
    //         return weak_ref.upgrade();
    //     }

    //     None
    //
}

/// Picks the next window in the background list and set it as active.
/// The order of windows in the background is based on the last time they are active.
/// The next window is the one which was active most recently.
pub fn switch_to_next() -> Result<(), &'static str> {
    active_window(0, true)
}

/// Sets the specified window in the background list as active.
pub fn switch_to(window: &Arc<Mutex<Box<dyn Window>>>) -> Result<(), &'static str> {
    if let Some(index) = WINDOWLIST.lock().get_bgwindow_index(window) {
        active_window(index, true)?;
    }

    Ok(())
}

// Actives a window in the background list.
// # Arguments
// * `index`: the index of the window in the background list.
// * `set_back_current`: whether to keep current active window in the background list. Delete current window if `set_back_current` is false.
fn active_window(index: usize, set_back_current: bool) -> Result<(), &'static str> {
    let mut window_list = WINDOWLIST.lock();

    if let Some(window) = window_list.active.upgrade() {
        let mut current = window.lock();
        if set_back_current {
            (*current).active(false)?;
            let old_active = window_list.active.clone();
            window_list.background_list.push_front(old_active);
        } else {
            (*current).clean()?;
        }
    }

    if let Some(active) = window_list.background_list.remove(index) {
        window_list.active = active;
        if let Some(window) = window_list.active.upgrade() {
            let mut current = window.lock();
            (*current).active(true)?;
        }
    }

    Ok(())
}

/*  Following two functions can be used to systematically resize windows forcibly
/// Readjusts remaining windows after a window is deleted to maximize screen usage
pub fn adjust_window_after_deletion() -> Result<(), &'static str> {
    let mut allocator = try!(WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")).lock();
    let num_windows = allocator.deref_mut().allocated.len();
    // one gap between each window and one gap between the edge windows and the frame buffer boundary
    let window_height = (frame_buffer::FRAME_BUFFER_HEIGHT - WINDOW_MARGIN * (num_windows + 1))/(num_windows);
    let window_width = frame_buffer::FRAME_BUFFER_WIDTH - 2 * WINDOW_MARGIN; // fill the width of the screen with a slight gap at the boundaries
    let mut height_index = WINDOW_MARGIN; // start resizing the windows after the first gap

    // Resizes the windows vertically
    for window_inner_ref in allocator.deref_mut().allocated.iter_mut() {
        let strong_window_ptr = window_inner_ref.upgrade();
        if let Some(window_inner_ptr) = strong_window_ptr {
            let mut locked_window_ptr = window_inner_ptr.lock();
            locked_window_ptr.resize(WINDOW_MARGIN, height_index, window_width, window_height)?;
            locked_window_ptr.key_producer.enqueue(Event::DisplayEvent); // refreshes display after resize
            height_index += window_height + WINDOW_MARGIN; // advance to the height index of the next window
        }
    }
    Ok(())
/// Adjusts the windows preemptively so that we can add a new window directly below the old ones to maximize screen usage without overlap
pub fn adjust_windows_before_addition() -> Result<(usize, usize, usize), &'static str> {
    let mut allocator = try!(WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")).lock();
    let num_windows = allocator.deref_mut().allocated.len();
    // one gap between each window and one gap between the edge windows and the frame buffer boundary
    let window_height = (display::FRAME_BUFFER_HEIGHT - WINDOW_MARGIN * (num_windows + 2))/(num_windows + 1);
    let window_width = frame_buffer::FRAME_BUFFER_WIDTH - 2 * WINDOW_MARGIN; // refreshes display after resize
    let mut height_index = WINDOW_MARGIN; // start resizing the windows after the first gap

    if num_windows >=1  {
        // Resizes the windows vertically
        for window_inner_ref in allocator.deref_mut().allocated.iter_mut() {
            let strong_ptr = window_inner_ref.upgrade();
            if let Some(window_inner_ptr) = strong_ptr {
                let mut locked_window_ptr = window_inner_ptr.lock();
                locked_window_ptr.resize(WINDOW_MARGIN, height_index, window_width, window_height)?;
                locked_window_ptr.key_producer.enqueue(Event::DisplayEvent); // refreshes window after
                height_index += window_height + WINDOW_MARGIN; // advance to the height index of the next window
            }
        }
    }


    return Ok((height_index, window_width, window_height)); // returns the index at which the new window should be drawn
}

*/
