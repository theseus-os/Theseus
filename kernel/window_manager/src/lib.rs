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
#![feature(const_fn)]
#![feature(asm)]

extern crate spin;
extern crate irq_safety;
#[macro_use] extern crate alloc;
extern crate dfqueue;
extern crate keycodes_ascii;
extern crate display;
extern crate display_text;
extern crate event_types;
extern crate spawn;
extern crate pit_clock;

#[macro_use] extern crate log;
//#[macro_use] extern crate util;

extern crate acpi;
extern crate frame_buffer;

use spin::{Once, Mutex};
use alloc::collections::{VecDeque, BTreeMap};
use core::ops::{Deref, DerefMut};
use dfqueue::{DFQueue,DFQueueConsumer,DFQueueProducer};
use alloc::sync::{Arc, Weak};
use display_text::{Print};
use display::Display;
use frame_buffer::{FrameCompositor, Compositor, FrameBuffer};
use event_types::Event;
use alloc::string::{String, ToString};


pub mod displayable;

use displayable::text_display::TextDisplay;


static WINDOW_ALLOCATOR: Once<Mutex<WindowAllocator>> = Once::new();
const WINDOW_ACTIVE_COLOR:u32 = 0xFFFFFF;
const WINDOW_INACTIVE_COLOR:u32 = 0x343C37;
const SCREEN_BACKGROUND_COLOR:u32 = 0x000000;
const BORDER_WIDTH:usize = 1;
static VFRAME_BUFFER:Once<Arc<Mutex<FrameBuffer>>> = Once::new();

/// 10 pixel gap between windows 
pub const GAP_SIZE: usize = 10;
pub const WINDOW_MARGIN: usize = 2;

struct WindowAllocator {
    allocated: VecDeque<Weak<Mutex<WindowInner>>>, 
    // a weak pointer directly to the active WindowInner so we don't have to search for the active window when we need it quickly
    // this weak pointer is set in the WindowAllocator's switch(), delete(), and allocate() functions
    active: Weak<Mutex<WindowInner>>, 
}

/// Applications call this function to request a new window object with a default size (mostly fills screen with GAP_SIZE around all borders)
/// If the caller a specific window size, it should call new_window()
pub fn new_default_window() -> Result<WindowObj, &'static str> {
    let (window_width, window_height) = frame_buffer::get_screen_size()?;
    match new_window(GAP_SIZE, GAP_SIZE, window_width - 2* GAP_SIZE, window_height - 2* GAP_SIZE) {
        Ok(new_window) => {return Ok(new_window)}
        Err(err) => {return Err(err)}
    }
}

/// Lets the caller specify the dimensions of the new window and returns a new window
/// Params x,y specify the (x,y) coordinates of the top left corner of the window
/// Params width and height specify dimenions of new window in pixels
pub fn new_window<'a>(x:usize, y:usize, width:usize, height:usize) -> Result<WindowObj, &'static str>{

    let allocator: &Mutex<WindowAllocator> = WINDOW_ALLOCATOR.call_once(|| {
        Mutex::new(WindowAllocator{
            allocated:VecDeque::new(),
            active:Weak::new(),
        })
    });
    trace!("Wenqiu {} {} {} {}", x, y, width, height);
    allocator.lock().allocate(x, y, width, height)
}

/// Puts an input event into the active window (i.e. a keypress event, resize event, etc.)
/// If the caller wants to put an event into a specific window, use put_event_into_app()
pub fn send_event_to_active(event: Event) -> Result<(), &'static str>{    
    let mut allocator = try!(WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")).lock();
    allocator.send_event(event)
}


// Gets the border color specified by the window manager
fn get_border_color(active:bool) -> u32 {
    if active {
        WINDOW_ACTIVE_COLOR
    } else {
        WINDOW_INACTIVE_COLOR
    }
}


/// switch the active window. Active the next window in the list based on the order they are created
pub fn switch() -> Result<(), &'static str> {
    let mut allocator = try!(WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")).lock();
    allocator.switch()?;
    Ok(())
}

/// delete a window object
pub fn delete(window:WindowObj) -> Result<(), &'static str> {
    let mut allocator = try!(WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")).lock();
    // Switches to a new active window and sets 
    // the active pointer field of the window allocator to the new active window
    allocator.switch()?; 
    allocator.delete(&(window.inner))?;
    Ok(())
}

/// Returns the width and height (in pixels) of the screen 
// pub fn get_screen_size() -> Result<(usize, usize), &'static str> {
//     frame_buffer::get_resolution()
// }

impl WindowAllocator{
    
    // Allocate a new window and return it
    fn allocate(&mut self, x:usize, y:usize, width:usize, height:usize) -> Result<WindowObj, &'static str>{
        
        {
            let (screen_width, screen_height) = frame_buffer::get_screen_size()?;
            let vframebuffer = FrameBuffer::new(screen_width, screen_height, None)?;
            VFRAME_BUFFER.call_once(|| {
                Arc::new(Mutex::new(vframebuffer))
            });
        }

       /*{
            let mut drawer = frame_buffer::FRAME_DRAWER.lock();
                let index = frame_buffer::get_index_fn(drawer.width);

                let buffer;
                match drawer.buffer() {
                    Ok(rs) => {buffer = rs;},
                    Err(err) => {return Err(err);},
                }
                buffer[index(0,0)] = 0x777777;
        }*/

//        let (screen_width, screen_height) = DRAWER.get_resolution();
        if width < 2 * WINDOW_MARGIN || height < 2 * WINDOW_MARGIN {
            return Err("Window size must be greater than the margin");
        }

    //Window size can exceeds the boundray of the screen
    //     if x + width >= buffer_width
    //    || y + height >= buffer_height {
    //         return Err("Requested area exceeds the screen size");
    //     }

        let consumer = DFQueue::new().into_consumer();
        let producer = consumer.obtain_producer();

        //new_window = ||{WindowObj{x:x,y:y,width:width,height:height,active:true}};
        
        let framebuffer = FrameBuffer::new(width, height, None)?;
        let inner = WindowInner{
            x:x,
            y:y,
            width:width,
            height:height,
            active:true,
            margin:WINDOW_MARGIN,
            key_producer:producer,
        };

        let inner_ref = Arc::new(Mutex::new(inner));

        let overlapped = self.check_overlap(&inner_ref, x, y, width, height);       
        if overlapped  {
            return Err("Request area is already allocated");
        }

        // draw the window border

        {
            let inner = inner_ref.lock();
            inner.draw_border(get_border_color(true))?;
            inner.clean()?
        }
        
       // inactive all other windows and active the new one
        for item in self.allocated.iter_mut(){
            let ref_opt = item.upgrade();
            if let Some(reference) = ref_opt {
                reference.lock().active(false)?;
            }
        }
        let weak_ref = Arc::downgrade(&inner_ref);
        self.active = weak_ref.clone();
        self.allocated.push_back(weak_ref);

        // new the window
        let window: WindowObj = WindowObj{
            inner:inner_ref,
            //text_buffer:FrameTextBuffer::new(),
            consumer:consumer,
            components:BTreeMap::new(),
            framebuffer:framebuffer,
        }; 

        Ok(window)  
    }

    /// Switches to the next active window in the window allocator list
    /// This function returns the index of the previously active window in the window_allocator.allocated vector
    fn switch(&mut self) -> Result<usize, &'static str> {
        let mut flag = false;
        let mut i = 0;
        let mut prev_active = 0;
        for item in self.allocated.iter_mut(){
            let reference = item.upgrade();
            if let Some(window) = reference {
                let mut window = window.lock();
                if flag {
                    (*window).active(true)?;
                    self.active = item.clone(); // clones the weak pointer to put in the active field
                    flag = false;
                    self.active = item.clone();
                } else if window.active {
                    (*window).active(false)?;
                    prev_active = i;
                    flag = true;
   
             }
                i += 1;
            }
        }
        if flag {
            for item in self.allocated.iter_mut(){
                let reference = item.upgrade();
                if let Some(window) = reference {
                    let mut window = window.lock();
                    (*window).active(true)?;
                    self.active = item.clone(); // clones the weak pointer to put into the active field
                    break;
                }
            }
        }

        return Ok(prev_active);
    }

    fn delete(&mut self, inner:&Arc<Mutex<WindowInner>>) -> Result<(), &'static str> {
        let mut i = 0;
        let len = self.allocated.len();
        for item in self.allocated.iter(){
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
                let window_ref = &self.allocated[i];
                let window = window_ref.upgrade();
                if let Some(window) = window {
                    window.lock().key_producer.enqueue(Event::ExitEvent);
                }
            }
            self.allocated.remove(i);
        }

        inner.lock().draw_border(SCREEN_BACKGROUND_COLOR)?;

        Ok(())
    }

    fn check_overlap(&mut self, inner:&Arc<Mutex<WindowInner>>, x:usize, y:usize, width:usize, height:usize) -> bool {
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
                    self.allocated.remove(i);
                    len -= 1;
                }
            }
        }
        false
    }
    
    /// enqueues the key event into the active window
    fn send_event(&mut self, event:Event) -> Result<(), &'static str> {
        let reference = self.active.upgrade(); // grabs a pointer to the active WindowInner
        if let Some(window) = reference {
            let window = window.lock();
            window.key_producer.enqueue(event);
        }
        Ok(())
    }

}

/// a window object
//TODO Wenqiu: drop function
pub struct WindowObj {
    inner:Arc<Mutex<WindowInner>>,
    //text_buffer:FrameTextBuffer,
    consumer:DFQueueConsumer<Event>,
    components:BTreeMap<String, Component>,
    /// the framebuffer owned by the window
    pub framebuffer:FrameBuffer,
}


impl WindowObj{

    // clean the content of a window
    fn clean(&mut self) -> Result<(), &'static str>{
        trace!("Wenqiu {} {}", 3, 4);
        let ((x, y), (width, height), margin) = { 
            let inner = self.inner.lock();
            (inner.get_location(), inner.get_size(), inner.get_margin())};
        self.fill_rectangle(margin, margin, width - 2 * margin, height - 2 * margin, SCREEN_BACKGROUND_COLOR)?;
        FrameCompositor::compose(
            vec![(&mut self.framebuffer, x, y)]
        )
    }


    /// Returns the dimensions of this window,
    /// as a tuple of `(width, height)`.
    pub fn dimensions(&self) -> (usize, usize) {
        let inner_locked = self.inner.lock();
        (inner_locked.width, inner_locked.height)
    }

    ///Add a new displayable structure to the window
    ///We check if the displayable is in the window. But we do not check if it is overlapped with others
    pub fn add_displayable(&mut self, key: &str, x:usize, y:usize, displayable:TextDisplay) -> Result<(), &'static str>{        
        let key = key.to_string();
        let (width, height) = displayable.get_size();
        let inner = self.inner.lock();

        if !inner.check_in_content(inner.x + inner.margin + x, inner.y + inner.margin + y) ||
            !inner.check_in_content(inner.x + inner.margin + x + width, inner.y + inner.margin + y) ||
            !inner.check_in_content(inner.x + inner.margin + x, inner.y + inner.margin + y + height) ||
            !inner.check_in_content(inner.x + inner.margin + x + width, inner.y + inner.margin + y + height) {

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

    pub fn remove_displayable(&mut self, key:&str){
        self.components.remove(key);
    }

    pub fn get_displayable(&self, key:&str) -> Option<&TextDisplay> {
        let opt = self.components.get(key);
        match opt {
            None => {return None},
            Some(component) => {return Some(component.get_displayable());},
        };
    }

    pub fn get_displayable_position(&self, key:&str) -> Result<(usize, usize), &'static str> {
        let opt = self.components.get(key);
        match opt {
            None => {return Err("No such displayable");},
            Some(component) => {return Ok(component.get_position());},
        };
    }

    ///Get the content position of the window excluding border and padding
    pub fn get_content_position(&self) -> (usize, usize) {
        let inner = self.inner.lock();
        (inner.x + inner.margin, inner.y + inner.margin)
    }

    /// draw a pixel in a window
    pub fn draw_pixel(&mut self, x:usize, y:usize, color:u32) -> Result<(), &'static str> {
        let inner = self.inner.lock();
        if x >= inner.width - 2 || y >= inner.height - 2 {
            return Ok(());
        }
        self.framebuffer.draw_pixel(x + inner.margin, y + inner.margin, color);
        FrameCompositor::compose(
            vec![(&self.framebuffer, inner.x, inner.y)]
        )
    }

    /// draw a line in a window
    pub fn draw_line(&mut self, start_x:usize, start_y:usize, end_x:usize, end_y:usize, color:u32) -> Result<(), &'static str> {
        let inner = self.inner.lock();
        if start_x > inner.width - 2
            || start_y > inner.height - 2
            || end_x > inner.width - 2
            || end_y > inner.height - 2 {
            return Ok(());
        }
        self.framebuffer.draw_line((start_x + inner.x + 1) as i32, (start_y + inner.y + 1) as i32, 
            (end_x + inner.x + 1) as i32, (end_y + inner.y + 1) as i32, color);
        FrameCompositor::compose(
            vec![(&self.framebuffer, inner.x, inner.y)]
        )
    }

    /// draw a rectangle in a window
    pub fn draw_rectangle(&mut self, x:usize, y:usize, width:usize, height:usize, color:u32) 
        -> Result<(), &'static str> {
        let inner = self.inner.lock();
        if x + width > inner.width - 2
            || y + height > inner.height - 2 {
            return Ok(());
        }

        frame_buffer::display::graph::fill_rectangle(&mut self.framebuffer, inner.margin + x, inner.margin + y, width, height, 
                color);
        FrameCompositor::compose(
            vec![(&self.framebuffer, inner.x, inner.y)]
        )
    }

    /// fill a rectangle in a window
    pub fn fill_rectangle(&mut self, x:usize, y:usize, width:usize, height:usize, color:u32) 
        -> Result<(), &'static str> {
        let inner = self.inner.lock();
        // if x + width > inner.width - 2
        //     || y + height > inner.height - 2 {
        //     return Ok(());
        // }
        frame_buffer::display::graph::fill_rectangle(&mut self.framebuffer, inner.margin + x, inner.margin + y, width, height, 
                color);
        FrameCompositor::compose(
            vec![(&self.framebuffer, inner.x, inner.y)]
        )
    }

    pub fn display_string(&mut self, display_name:&str, slice:&str, font_color:u32, bg_color:u32) -> Result<(), &'static str> {
        let (x, y) = self.get_displayable_position(display_name)?;
        let (width, height) = self.get_displayable(display_name).ok_or("The displayable does not exist")?.get_size();
        self.framebuffer.print_by_bytes(x, y, width, height, slice, font_color, bg_color)?;
        Ok(())
    }

    /// Requires that a str slice that will exactly fit the frame buffer
    /// The calculation is done inside the console crate by the print_by_bytes function and associated methods
    /// Print every byte and fill the blank with background color
    /*pub fn display_&str(&mut self, slice: &str) -> Result<(), &'static str> {
        let inner = self.inner.lock();
        self.text_buffer.print_by_bytes(inner.x + inner.margin, inner.y + inner.margin, 
            inner.width - 2 * inner.margin, inner.height - 2 * inner.margin, 
            slice)
    }*/
    
    /*
    pub fn draw_border(&self) -> (usize, usize, usize){
        let inner = self.inner.lock();
        inner.draw_border(get_border_color(inner.active))
    }
    */

    // @Andrew
    //Wenqiu TODO: remap the frame buffer
    pub fn resize(&mut self, x:usize, y:usize, width:usize, height:usize) -> Result<(), &'static str>{
        { // checks for overlap
            let inner = self.inner.clone();
            let mut allocator = try!(WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")).lock();
            match allocator.check_overlap(&inner, x,y,width,height) {
                true => {return Err("cannot resize because requested resize will cause overlap")}
                false => { }
            }
        }
        
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

struct WindowInner {
    /// the upper left x-coordinate of the window
    x: usize,
    /// the upper left y-coordinate of the window
    y: usize,
    /// the width of the window
    width: usize,
    /// the height of the window
    height: usize,
    /// whether the window is active
    active: bool,
    /// a consumer of key input events to the window
    margin: usize,
    /// the producer accepting a key event
    key_producer: DFQueueProducer<Event>,
}

impl WindowInner {
    fn clean(&self) -> Result<(), &'static str> {
        trace!("Wenqiu {} {}", 3, 4);
        let buffer_ref = match VFRAME_BUFFER.try(){
            Some(buffer) => { buffer },
            None => { return Err("Fail to get the virtual frame buffer") }
        };
        let mut buffer_lock = buffer_ref.lock();
        let buffer = buffer_lock.deref_mut();
        frame_buffer::display::graph::draw_rectangle(buffer, self.x + BORDER_WIDTH, self.x + BORDER_WIDTH,
            self.width - 2 * BORDER_WIDTH, self.height - 2 * BORDER_WIDTH, SCREEN_BACKGROUND_COLOR);
        FrameCompositor::compose(
            vec![(buffer, 0, 0)]
        )
    }

    //check if the window is overlapped with any existing window
    fn is_overlapped(&self, x:usize, y:usize, width:usize, height:usize) -> bool {
        if self.check_in_area(x, y)
            {return true;}
        if self.check_in_area(x, y+height)
            {return true;}
        if self.check_in_area(x+width, y)
            {return true;}
        if self.check_in_area(x+width, y+height)
            {return true;}
        false        
    } 

    // check if the pixel is within the window
    fn check_in_area(&self, x:usize, y:usize) -> bool {        
        return x>= self.x && x <= self.x+self.width 
                && y>=self.y && y<=self.y+self.height;
    }

    // check if the pixel is within the window exluding the border and margin
    fn check_in_content(&self, x:usize, y:usize) -> bool {        
        return x>= self.x+self.margin && x <= self.x+self.width-self.margin 
                && y>=self.y+self.margin && y<=self.y+self.height-self.margin;
    }

    // active or inactive a window
    fn active(&mut self, active:bool) -> Result<(), &'static str> {
        self.active = active;
        self.draw_border(get_border_color(active))?;
        Ok(())
    }

    // draw the border of the window
    fn draw_border(&self, color:u32) -> Result<(), &'static str> {
        trace!("Wenqiu {} {}", 3, 4);
        let buffer_ref = match VFRAME_BUFFER.try(){
            Some(buffer) => { buffer },
            None => { return Err("Fail to get the virtual frame buffer") }
        };
        let mut buffer_lock = buffer_ref.lock();
        let buffer = buffer_lock.deref_mut();
        frame_buffer::display::graph::draw_rectangle(buffer, self.x, self.y, self.width, self.height, color);
        FrameCompositor::compose(
            vec![(buffer, 0, 0)]
        )
    }

    /// adjust the size of a window
    fn resize(&mut self, x:usize, y:usize, width:usize, height:usize) -> Result<(usize, usize), &'static str> {
        self.draw_border(SCREEN_BACKGROUND_COLOR)?;
        let percent = ((width-self.margin)*100/(self.width-self.margin), (height-self.margin)*100/(self.height-self.margin));
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height; 
        self.draw_border(get_border_color(self.active))?;
        Ok(percent)
    }

    fn get_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn get_location(&self) -> (usize, usize) {
        (self.x, self.y)
    }

    fn get_margin(&self) -> usize {
        self.margin
    }

}

struct Component {
    x:usize,
    y:usize,
    displayable:TextDisplay,
}

impl Component {
    pub fn get_displayable(&self) -> &TextDisplay {
        return &(self.displayable)
    }

    pub fn get_position(&self) -> (usize, usize) {
        (self.x, self.y)
    }

    fn resize(&mut self, x:usize, y:usize, width:usize, height:usize) {
        self.x = x;
        self.y = y;
        self.displayable.resize(width, height);
    }
}

/*  Following two functions can be used to systematically resize windows forcibly
/// Readjusts remaining windows after a window is deleted to maximize screen usage
pub fn adjust_window_after_deletion() -> Result<(), &'static str> {
    let mut allocator = try!(WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")).lock();
    let num_windows = allocator.deref_mut().allocated.len();
    // one gap between each window and one gap between the edge windows and the frame buffer boundary
    let window_height = (frame_buffer::FRAME_BUFFER_HEIGHT - GAP_SIZE * (num_windows + 1))/(num_windows); 
    let window_width = frame_buffer::FRAME_BUFFER_WIDTH - 2 * GAP_SIZE; // fill the width of the screen with a slight gap at the boundaries
    let mut height_index = GAP_SIZE; // start resizing the windows after the first gap 

    // Resizes the windows vertically
    for window_inner_ref in allocator.deref_mut().allocated.iter_mut() {
        let strong_window_ptr = window_inner_ref.upgrade();
        if let Some(window_inner_ptr) = strong_window_ptr {
            let mut locked_window_ptr = window_inner_ptr.lock();
            locked_window_ptr.resize(GAP_SIZE, height_index, window_width, window_height)?;
            locked_window_ptr.key_producer.enqueue(Event::DisplayEvent); // refreshes display after resize
            height_index += window_height + GAP_SIZE; // advance to the height index of the next window
        }
    }
    Ok(())
/// Adjusts the windows preemptively so that we can add a new window directly below the old ones to maximize screen usage without overlap
pub fn adjust_windows_before_addition() -> Result<(usize, usize, usize), &'static str> {
    let mut allocator = try!(WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")).lock();
    let num_windows = allocator.deref_mut().allocated.len();
    // one gap between each window and one gap between the edge windows and the frame buffer boundary
    let window_height = (display::FRAME_BUFFER_HEIGHT - GAP_SIZE * (num_windows + 2))/(num_windows + 1); 
    let window_width = frame_buffer::FRAME_BUFFER_WIDTH - 2 * GAP_SIZE; // refreshes display after resize
    let mut height_index = GAP_SIZE; // start resizing the windows after the first gap 

    if num_windows >=1  {
        // Resizes the windows vertically
        for window_inner_ref in allocator.deref_mut().allocated.iter_mut() {
            let strong_ptr = window_inner_ref.upgrade();
            if let Some(window_inner_ptr) = strong_ptr {
                let mut locked_window_ptr = window_inner_ptr.lock();
                locked_window_ptr.resize(GAP_SIZE, height_index, window_width, window_height)?;
                locked_window_ptr.key_producer.enqueue(Event::DisplayEvent); // refreshes window after  
                height_index += window_height + GAP_SIZE; // advance to the height index of the next window
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
