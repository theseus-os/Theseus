//! Window manager that simulates a desktop environment. 
//! *note: since window overlap is not yet supported, any application that asks for a window that would overlap
//! with an existing window will be returned an error
//! 
//! Applications request window objects from the window manager through either of two functions:
//! - default_new_window() will provide a window of default size (centered, fills majority of screen)
//! - new_window() provides a new window whose dimensions the caller must specify
//! 
//! Windows can be resized by calling resize()
//! Window can be by passing the window object to delete() or directly through WindowObj.delete(), though the 2nd option requires updating the active pointer manually
//! 
//! The window object is designed as follows:
//! The application's window provides a screen area for the application to display its content into. The application can select
//! from various Displayables such as TextDisplay (multiple of which can be displayed into the application's window at a time)
//! 
//! The WINDOW_ALLOCATOR is used by the WindowManager itself to track and modify the existing windows

#![no_std]

extern crate spin;
#[macro_use] extern crate alloc;
extern crate dfqueue;
extern crate event_types;
#[macro_use] extern crate log;
extern crate frame_buffer;
extern crate frame_buffer_drawer;
extern crate frame_buffer_printer;
#[macro_use] extern crate lazy_static;
extern crate displayable;
extern crate font;

use spin::{Once, Mutex};
use alloc::collections::{VecDeque, BTreeMap};
use core::ops::{Deref, DerefMut};
use dfqueue::{DFQueue,DFQueueConsumer,DFQueueProducer};
use alloc::sync::{Arc, Weak};
use frame_buffer::{FrameCompositor, Compositor, FrameBuffer};
use frame_buffer_drawer::*;
use frame_buffer_printer::print_by_bytes;
use event_types::Event;
use alloc::string::{String, ToString};
use displayable::{TextDisplay, Cursor};
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH};

lazy_static! {
    /// The list of all Tasks in the system.
    static ref WINDOWLIST: Mutex<WindowList> = Mutex::new(
        WindowList{
            list:VecDeque::new(),
            active:Weak::new(),
        }
    );
}

// A framebuffer owned by the window manager. 
// This framebuffer is responsible for display borders. Windows owned by applications cannot get access to their borders. 
// All the display behaviors of borders are controled by the window manager
static SCREEN_FRAME_BUFFER:Once<Arc<Mutex<FrameBuffer>>> = Once::new();

// 10 pixel gap between windows 
const WINDOW_MARGIN: usize = 10;
// 2 pixel padding within a window
const WINDOW_PADDING: usize = 2;
// The border color of an active window 
const WINDOW_ACTIVE_COLOR: u32 = 0xFFFFFF;
// The border color of an inactive window
const WINDOW_INACTIVE_COLOR: u32 = 0x343C37;
// The background color of the screen
const SCREEN_BACKGROUND_COLOR: u32 = 0x000000;

//Wenqiu
/// The window allocator.
/// It contains a list of allocated window and a reference to the active window
struct WindowList {
    list: VecDeque<Weak<Mutex<WindowInner>>>, 
    // a weak pointer directly to the active WindowInner so we don't have to search for the active window when we need it quickly
    // this weak pointer is set in the WindowAllocator's switch(), delete(), and allocate() functions
    active: Weak<Mutex<WindowInner>>, 
}

pub fn init() -> Result<(), &'static str> {
    let (screen_width, screen_height) = frame_buffer::get_screen_size()?;
    let framebuffer = FrameBuffer::new(screen_width, screen_height, None)?;
    SCREEN_FRAME_BUFFER.call_once(|| {
        Arc::new(Mutex::new(framebuffer))
    });
    Ok(())
}

/// Applications call this function to request a new window object with a default size (mostly fills screen with WINDOW_MARGIN around all borders)
/// If the caller a specific window size, it should call new_window()
pub fn new_default_window() -> Result<WindowObj, &'static str> {
    let (window_width, window_height) = frame_buffer::get_screen_size()?;
    match new_window(WINDOW_MARGIN, WINDOW_MARGIN, window_width - 2* WINDOW_MARGIN, window_height - 2* WINDOW_MARGIN) {
        Ok(new_window) => {return Ok(new_window)}
        Err(err) => {return Err(err)}
    }
}

/// Lets the caller specify the dimensions of the new window and returns a new window
/// Params x,y specify the (x,y) coordinates of the top left corner of the window
/// Params width and height specify dimenions of new window in pixels
pub fn new_window<'a>(x:usize, y:usize, width:usize, height:usize) -> Result<WindowObj, &'static str>{
    // Check the size of the window
    if width < 2 * WINDOW_PADDING || height < 2 * WINDOW_PADDING {
        return Err("Window size must be greater than the padding");
    }

    // Init the key input producer and consumer
    let consumer = DFQueue::new().into_consumer();
    let producer = consumer.obtain_producer();
    
    // Init the frame buffer of the window
    let framebuffer = FrameBuffer::new(width - 2 * WINDOW_PADDING, height - 2 * WINDOW_PADDING, None)?;
    let inner = WindowInner{
        x:x,
        y:y,
        width:width,
        height:height,
        active:true,
        padding:WINDOW_PADDING,
        key_producer:producer,
    };

    // // Check if the window overlaps with others
    // let inner_ref = Arc::new(Mutex::new(inner));
    // let overlapped = self.check_overlap(&inner_ref, x, y, width, height);       
    // if overlapped  {
    //     return Err("Request area is already allocated");
    // }
    
    let inner_ref = Arc::new(Mutex::new(inner));

    WINDOWLIST.lock().add(&inner_ref);
    
    // active the void window content
    {
        let mut inner = inner_ref.lock();

        // TODO WENQIU: use render
        inner.clean()?;
        inner.active(true)?;
        //inner.draw_border(get_border_color(true))?;
    }

    // create the window
    let window: WindowObj = WindowObj{
        inner:inner_ref,
        //text_buffer:FrameTextBuffer::new(),
        consumer:consumer,
        components:BTreeMap::new(),
        framebuffer:framebuffer,
    }; 

    Ok(window)
}

/// Puts an input event into the active window (i.e. a keypress event, resize event, etc.)
/// If the caller wants to put an event into a specific window, use put_event_into_app()
pub fn send_event_to_active(event: Event) -> Result<(), &'static str>{    
    let window_list = WINDOWLIST.lock();
    let active_ref = window_list.active.upgrade(); // grabs a pointer to the active WindowInner
    if let Some(window) = active_ref {
        let window = window.lock();
        window.key_producer.enqueue(event);
    }
    Ok(())
}

// Gets the border color according to the active state
fn get_border_color(active:bool) -> u32 {
    if active {
        WINDOW_ACTIVE_COLOR
    } else {
        WINDOW_INACTIVE_COLOR
    }
}

impl WindowList{
    // add a new window to the list
    fn add(&mut self, inner_ref:&Arc<Mutex<WindowInner>>) {
        // // inactive all other windows and active the new one
        // for item in self.list.iter_mut(){
        //     let ref_opt = item.upgrade();
        //     if let Some(reference) = ref_opt {
        //         reference.lock().active(false)?;
        //     }
        // }

        // Add a reference of the new window to the window list
        let weak_ref = Arc::downgrade(&inner_ref);
        // window_list.active = weak_ref.clone();
        self.list.push_back(weak_ref);
    }

    // delete a window
    fn delete(&mut self, inner:&Arc<Mutex<WindowInner>>) -> Result<(), &'static str> {
        let mut i = 0;
        let len = self.list.len();
        for item in self.list.iter(){
            let reference = item.upgrade();
            if let Some(inner_ptr) = reference {
                if Arc::ptr_eq(&(inner_ptr), inner) {
                    break;
                }
            }
            i += 1;
        }
        if i < len {
            {
                let window_ref = &self.list[i];
                let window = window_ref.upgrade();
                if let Some(window) = window {
                    window.lock().key_producer.enqueue(Event::ExitEvent);
                }
            }
            self.list.remove(i);
        }

        inner.lock().clean()?;

        Ok(())
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
    fn next(&mut self) -> Option<Arc<Mutex<WindowInner>>> {
        let mut current_active = false;
        for item in self.list.iter_mut(){
            let reference = item.upgrade();
            if let Some(window) = reference {
                if window.lock().active {
                    current_active = true;
                } else if current_active {
                    return Some(window)
                }
            }
        }

        if current_active {
            for item in self.list.iter_mut(){
                let reference = item.upgrade();
                if let Some(window) = reference {
                    return Some(window)
                }
            }
        }

        None
    }


}

/// A window contains a reference to its inner reference owned by the window manager,
/// a consumer of inputs, a list of displayables and a framebuffer
pub struct WindowObj {
    inner:Arc<Mutex<WindowInner>>,
    //text_buffer:FrameTextBuffer,
    consumer:DFQueueConsumer<Event>,
    components:BTreeMap<String, Component>,
    /// the framebuffer owned by the window
    framebuffer:FrameBuffer,
}

impl WindowObj{
    // clean the content of a window
    fn clean(&mut self) -> Result<(), &'static str>{
        let (width, height) = self.inner.lock().get_content_size();
        fill_rectangle(&mut self.framebuffer, 0, 0, width, height, SCREEN_BACKGROUND_COLOR);
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
    pub fn add_displayable(&mut self, key: &str, x:usize, y:usize, displayable:TextDisplay) -> Result<(), &'static str>{        
        let key = key.to_string();
        let (width, height) = displayable.get_size();
        let inner = self.inner.lock();

        if !inner.check_in_content(x, y) || !inner.check_in_content(x + width, y)
            ||!inner.check_in_content(x, y + height) || !inner.check_in_content(x + width, y + height) {
            return Err("The displayable does not fit the window size.");
        } 

        let component = Component {
            x:x,
            y:y,
            displayable:displayable,
        };

        self.components.insert(key, component);
        Ok(())
    }

    /// Remove a displayable according to its name
    pub fn remove_displayable(&mut self, name:&str){
        self.components.remove(name);
    }

    /// Get a displayable of the name
    pub fn get_displayable(&self, name:&str) -> Option<&TextDisplay> {
        let opt = self.components.get(name);
        match opt {
            None => { return None },
            Some(component) => { return Some(component.get_displayable()); },
        };
    }

    /// Get the position of a displayable in the window
    pub fn get_displayable_position(&self, key:&str) -> Result<(usize, usize), &'static str> {
        let opt = self.components.get(key);
        match opt {
            None => { return Err("No such displayable"); },
            Some(component) => { return Ok(component.get_position()); },
        };
    }

    ///Get the content position of the window excluding border and padding
    pub fn get_content_position(&self) -> (usize, usize) {
        self.inner.lock().get_content_position()
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
    pub fn render(&mut self) -> Result<(), &'static str>{
        let (window_x, window_y) = { self.inner.lock().get_content_position() };
        FrameCompositor::compose(
            vec![(&mut self.framebuffer, window_x, window_y)]
        )
    }

    /// print a string in the window with a text displayable.
    pub fn display_string(&mut self, display_name:&str, slice:&str, font_color:u32, bg_color:u32) -> Result<(), &'static str> {
        let (display_x, display_y) = self.get_displayable_position(display_name)?;
        let (width, height) = self.get_displayable(display_name).ok_or("The displayable does not exist")?.get_size();        
        print_by_bytes(&mut self.framebuffer, display_x, display_y, width, height, slice, font_color, bg_color)?;
        self.render()
    }

    /// display a cursor in the window with a text displayable. position is the absolute position of the cursor
    pub fn display_cursor(&mut self, cursor:&Cursor, display_name:&str, position:usize, leftshift:usize, font_color:u32, bg_color:u32) -> Result<(), &'static str> {
        let (text_buffer_width, text_buffer_height) = match self.get_displayable(display_name) {
            Some(text_display) => { text_display.get_dimensions() },
            None => { return Err("The displayable does not exist") }
        };
        
        let mut column = position % text_buffer_width;
        let mut line = position / text_buffer_width;
        // adjusts to the correct position relative to the max rightmost absolute cursor position
        if column >= leftshift  {
            column -= leftshift;
        } else {
            column = text_buffer_width + column - leftshift;
            line -=1;
        }
        if line < text_buffer_height {
            let color = if cursor.show() { font_color } else { bg_color };
            fill_rectangle(&mut self.framebuffer, column * CHARACTER_WIDTH, line * CHARACTER_HEIGHT, CHARACTER_WIDTH, CHARACTER_HEIGHT, color);
            self.render()?;
        }
        Ok(())
    }

    // @Andrew
    /// resize a window as (width, height) at (x, y)
    pub fn resize(&mut self, x:usize, y:usize, width:usize, height:usize) -> Result<(), &'static str>{
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
                    item.resize(x*percent.0/100, y*percent.1/100, width*percent.0/100, height*percent.1/100);
                }
                inner.key_producer.enqueue(Event::new_resize_event(x, y, width, height));
                Ok(())
            },
            Err(err) => { Err(err) }
        }
    }

    ///Get a key event of the window
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

/// delete the reference of a window in the manager when the window is dropped
impl Drop for WindowObj {
    fn drop(&mut self) {
        let mut window_list = WINDOWLIST.lock();

        // Switches to a new active window and sets 
        // the active pointer field of the window allocator to the new active window
        match schedule() {
            Ok(_) => {}
            Err(err) => { error!("Fail to schedule to the next window: {}", err) }
        }; 
        
        match window_list.delete(&(self.inner)) {
            Ok(()) => {}
            Err(err) => { error!("Fail to delete the window from the window manager: {}", err) }
        }
    }
}

// The structure is owned by the window manager. It contains the information of a window but under the control of the manager
struct WindowInner {
    // the upper left x-coordinate of the window
    x: usize,
    // the upper left y-coordinate of the window
    y: usize,
    // the width of the window
    width: usize,
    // the height of the window
    height: usize,
    // whether the window is active
    active: bool,
    // a consumer of key input events to the window
    padding: usize,
    // the producer accepting a key event
    key_producer: DFQueueProducer<Event>,
}

impl WindowInner {
    //clean the window on the screen including the border and padding
    fn clean(&self) -> Result<(), &'static str> {
        let buffer_ref = match SCREEN_FRAME_BUFFER.try(){
            Some(buffer) => { buffer },
            None => { return Err("Fail to get the virtual frame buffer") }
        };
        let mut buffer_lock = buffer_ref.lock();
        let buffer = buffer_lock.deref_mut();
        draw_rectangle(buffer, self.x, self.y, self.width, self.height, SCREEN_BACKGROUND_COLOR);
        FrameCompositor::compose(
            vec![(buffer, 0, 0)]
        )
    }

    //check if the window is overlapped with any existing window
    fn is_overlapped(&self, x:usize, y:usize, width:usize, height:usize) -> bool {
        return self.check_in_area(x, y) && self.check_in_area(x, y + height) 
            && self.check_in_area(x + width, y) && self.check_in_area(x + width, y + height)
    } 

    // check if the pixel is within the window
    fn check_in_area(&self, x:usize, y:usize) -> bool {        
        return x >= self.x && x <= self.x + self.width 
                && y >= self.y && y <= self.y + self.height;
    }

    // check if the pixel is within the window exluding the border and padding
    fn check_in_content(&self, x:usize, y:usize) -> bool {        
        return x <= self.width - 2 * self.padding && y <= self.height - 2 * self.padding;
    }

    // active or inactive a window
    fn active(&mut self, active:bool) -> Result<(), &'static str> {
        self.active = active;
        self.draw_border(WINDOW_ACTIVE_COLOR)?;
        Ok(())
    }

    // draw the border of the window
    fn draw_border(&self, color:u32) -> Result<(), &'static str> {
        let buffer_ref = match SCREEN_FRAME_BUFFER.try(){
            Some(buffer) => { buffer },
            None => { return Err("Fail to get the virtual frame buffer") }
        };
        let mut buffer_lock = buffer_ref.lock();
        let buffer = buffer_lock.deref_mut();
        draw_rectangle(buffer, self.x, self.y, self.width, self.height, color);
        FrameCompositor::compose(
            vec![(buffer, 0, 0)]
        )
    }

    // adjust the size of a window
    fn resize(&mut self, x:usize, y:usize, width:usize, height:usize) -> Result<(usize, usize), &'static str> {
        self.draw_border(SCREEN_BACKGROUND_COLOR)?;
        let percent = ((width-self.padding)*100/(self.width-self.padding), (height-self.padding)*100/(self.height-self.padding));
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height; 
        self.draw_border(get_border_color(self.active))?;
        Ok(percent)
    }

    // get the size of content without padding
    fn get_content_size(&self) -> (usize, usize) {
        (self.width - 2 * self.padding, self.height - 2 * self.padding)
    }

    // get the position of content without padding
    fn get_content_position(&self) -> (usize, usize) {
        (self.x + self.padding, self.y + self.padding)
    }

}

// a component contains a displayable and its position
struct Component {
    x:usize,
    y:usize,
    displayable:TextDisplay,
}

impl Component {
    // get the displayable
    fn get_displayable(&self) -> &TextDisplay {
        return &(self.displayable)
    }

    // get the position of the displayable
    fn get_position(&self) -> (usize, usize) {
        (self.x, self.y)
    }

    // resize the displayable
    fn resize(&mut self, x:usize, y:usize, width:usize, height:usize) {
        self.x = x;
        self.y = y;
        self.displayable.resize(width, height);
    }
}

/// select the next window in the list and set it as active. set current active window and inactive
pub fn schedule() -> Result<(), &'static str>{
    let mut window_list = WINDOWLIST.lock();
    let next_window = match window_list.next() {
        Some(window) => { window }
        None => { return Ok(()) } //do nothing if current active window is the only window
    };

    // set current window as inactive    
    if let Some(window) = window_list.active.upgrade() {
        let mut current = window.lock();
        (*current).active(false)?;
    }

    // set next window as active
    let mut next = next_window.lock();
    (*next).active(true)?;
    window_list.active = Arc::downgrade(&next_window);

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


//Test functions for performance evaluation
/*pub fn set_time_start() {
    let hpet_lock = get_hpet();
    unsafe { STARTING_TIME = hpet_lock.as_ref().unwrap().get_counter(); }   
}

pub fn calculate_time_statistic() {
    let statistic = STATISTIC.call_once(|| {
        Mutex::new(Vec::new())
    });

  unsafe{

    let hpet_lock = get_hpet();
    let end_time = hpet_lock.as_ref().unwrap().get_counter();  

   
    let mut queue = statistic.lock();
    queue.push(end_time - STARTING_TIME);

    STARTING_TIME = 0;

    COUNTER  = COUNTER+1;

    if COUNTER == 1000 {
        for i in 0..queue.len(){
            trace!("Time\t{}", queue.pop().unwrap());
        }
        COUNTER = 0;
    }
}*/
