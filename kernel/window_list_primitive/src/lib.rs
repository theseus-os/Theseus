//! Defines a `WindowListPrimitive` structure that maintains a list of active and inactive windows.
//!
//! A primitive window list contains a list of background windows and an active window. It provides methods to switch among them.
//! Once an active window is deleted or set as inactive, the next window in the background list will become active.
//!
//! The order of windows is based on the last time it becomes active. The one which was active most recently is at the top of the background list.
//!
//! The `window_manager_primitve` crate holds an instance of `WindowListPrimitive` for existing windows. In the future, we will implement different kinds of `WindowListPrimitive` and therefore have different kinds of window manager in Theseus.

#![no_std]

extern crate alloc;
extern crate event_types;
extern crate frame_buffer;
extern crate frame_buffer_printer;
extern crate spin;
extern crate window;

use alloc::collections::VecDeque;
use alloc::sync::{Arc, Weak};
use event_types::Event;
use spin::{Mutex};
use window::WindowProfile;

/// 10 pixel gap between windows
pub const WINDOW_MARGIN: usize = 10;
/// 2 pixel padding within a window
pub const WINDOW_PADDING: usize = 2;
/// The border color of an active window
pub const WINDOW_ACTIVE_COLOR: u32 = 0xFFFFFF;
/// The border color of an inactive window
pub const WINDOW_INACTIVE_COLOR: u32 = 0x343C37;
/// The background color of the screen
pub const SCREEN_BACKGROUND_COLOR: u32 = 0x000000;

/// The window list structure.
/// It contains a list of reference to background windows and a reference to the active window.
pub struct WindowListPrimitive<T: WindowProfile> {
    /// The list of inactive windows. Their order is based on the last time they were active. The first window is the window which was active most recently.
    pub background_list: VecDeque<Weak<Mutex<T>>>,
    /// A weak pointer to the active window.
    pub active: Weak<Mutex<T>>,
}

impl<T: WindowProfile> WindowListPrimitive<T> {
    /// Adds a new window to the list and sets it as active.
    pub fn add_active(&mut self, inner_ref: &Arc<Mutex<T>>) -> Result<(), &'static str> {
        if let Some(current_active) = self.active.upgrade() {
            current_active.lock().draw_border(get_border_color(false))?;
            let weak_ref = self.active.clone();
            self.background_list.push_front(weak_ref);
        } 

        inner_ref.lock().draw_border(get_border_color(true))?;
        self.active = Arc::downgrade(inner_ref);

        Ok(())
    }

    /// Deletes a window from the list.
    pub fn delete(&mut self, inner: &Arc<Mutex<T>>) -> Result<(), &'static str> {
        // if the window is active, delete it and active the next top window
        if let Some(current_active) = self.active.upgrade() {
            if Arc::ptr_eq(&(current_active), inner) {
                self.set_active(0, false)?;
                return Ok(());
            }
        }

        if let Some(index) = self.get_bgwindow_index(&inner) {
            {
                let window_ref = &self.background_list[index];
                let window = window_ref.upgrade();
                if let Some(window) = window {
                    window.lock().events_producer().enqueue(Event::ExitEvent);
                }
            }
            self.background_list.remove(index);
        }

        inner.lock().clear()?;

        Ok(())
    }

    // gets the index of an inactive window in the background window list.
    fn get_bgwindow_index(&self, inner: &Arc<Mutex<T>>) -> Option<usize> {
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

    // Sets a window in the background list as active.
    // # Arguments
    // * `index`: the index of the window in the background list.
    // * `set_current_back`: whether to keep current active window in the background list. Delete current window if `set_current_back` is false.
    fn set_active(&mut self, index: usize, set_current_back: bool) -> Result<(), &'static str> {
        if let Some(window) = self.active.upgrade() {
            let mut current = window.lock();
            if set_current_back {
                (*current).draw_border(get_border_color(false))?;
                let old_active = self.active.clone();
                self.background_list.push_front(old_active);
            } else {
                (*current).clear()?;
            }
        }

        if let Some(active) = self.background_list.remove(index) {
            self.active = active;
            if let Some(window) = self.active.upgrade() {
                let current = window.lock();
                (*current).draw_border(get_border_color(true))?;
            }
        }

        Ok(())
    }

    /// Picks the next window in the background list and sets it as active.
    /// The order of windows in the background is based on the last time they are active.
    /// The next window is the one which was active most recently.
    pub fn switch_to_next(&mut self) -> Result<(), &'static str> {
        self.set_active(0, true)
    }

    /// Sets the specified window in the background list as active.
    pub fn switch_to(&mut self, window: &Arc<Mutex<T>>) -> Result<(), &'static str> {
        if let Some(index) = self.get_bgwindow_index(window) {
            self.set_active(index, true)?;
        }

        Ok(())
    }

    /// Puts an event into the active window (i.e. a keypress event, resize event, etc.).
    pub fn send_event_to_active(&mut self, event: Event) -> Result<(), &'static str> {
        let active_ref = self.active.upgrade(); // grabs a pointer to the active WindowProfile
        if let Some(window) = active_ref {
            let mut window = window.lock();
            window.events_producer().enqueue(event);
        }
        Ok(())
    }
    /*// check if an area specified by (x, y, width, height) overlaps with an existing window
    fn check_overlap(&mut self, inner:&Arc<Mutex<WindowProfile>>, x:usize, y:usize, width:usize, height:usize) -> bool {
        let mut len = self.allocated.len();
        let mut i = 0;
        while i < len {
            {
                let mut void = false;
                if let Some(reference) = self.allocated.get(i) {
                    if let Some(allocated_ref) = reference.upgrade() {
                        if !Arc::ptr_eq(&allocated_ref, inner) {
                            if allocated_ref.lock().is_overlapped(x, y, width, height) {
                                return true;
                            }
                        }
                        i += 1;
                    } else {
                        void = true;
                    }
                }
                if void {
                    self.list.remove(i);
                    len -= 1;
                }
            }
        }
        false
    }

    // return a reference to the next window of current active window
    fn next(&mut self) -> Option<Arc<Mutex<WindowProfile>>> {
        // let mut current_active = false;
        // for item in self.list.iter_mut(){
        //     let reference = item.upgrade();
        //     if let Some(window) = reference {
        //         if window.lock().active {
        //             current_active = true;
        //         } else if current_active {
        //             return Some(window)
        //         }
        //     }
        // }

        // if current_active {
        //     for item in self.list.iter_mut(){
        //         let reference = item.upgrade();
        //         if let Some(window) = reference {
        //             return Some(window)
        //         }
        //     }
        // }
        if let Some(weak_ref) = self.list.pop_front() {
            return weak_ref.upgrade();
        }

        None
    }*/


}

/*  Following two functions can be used to systematically resize windows forcibly
/// Readjusts remaining windows after a window is deleted to maximize screen usage
pub fn adjust_window_after_deletion() -> Result<(), &'static str> {
    let mut allocator = WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")?.lock();
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
            locked_window_ptr.events_producer.enqueue(Event::DisplayEvent); // refreshes display after resize
            height_index += window_height + WINDOW_MARGIN; // advance to the height index of the next window
        }
    }
    Ok(())
/// Adjusts the windows preemptively so that we can add a new window directly below the old ones to maximize screen usage without overlap
pub fn adjust_windows_before_addition() -> Result<(usize, usize, usize), &'static str> {
    let mut allocator = WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")?.lock();
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
                locked_window_ptr.events_producer.enqueue(Event::DisplayEvent); // refreshes window after
                height_index += window_height + WINDOW_MARGIN; // advance to the height index of the next window
            }
        }
    }


    return Ok((height_index, window_width, window_height)); // returns the index at which the new window should be drawn
}

*/

// gets the border color according to the active state
fn get_border_color(active: bool) -> u32 {
    if active {
        WINDOW_ACTIVE_COLOR
    } else {
        WINDOW_INACTIVE_COLOR
    }
}
