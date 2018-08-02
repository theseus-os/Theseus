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
#![feature(alloc)]
#![feature(const_fn)]
#![feature(unique)]
#![feature(unique)]
#![feature(asm)]

extern crate spin;
extern crate irq_safety;
extern crate alloc;
extern crate dfqueue;
extern crate keycodes_ascii;
extern crate frame_buffer;
extern crate event_types;
extern crate spawn;
extern crate pit_clock;

//#[macro_use] extern crate log;
//#[macro_use] extern crate util;

extern crate acpi;


use spin::{Once, Mutex};
use alloc::VecDeque;
use alloc::btree_map::BTreeMap;
use core::ops::Deref;
use dfqueue::{DFQueue,DFQueueConsumer,DFQueueProducer};
use alloc::arc::{Arc, Weak};
use frame_buffer::text_buffer::{FrameTextBuffer};
use event_types::Event;
use alloc::string::{String, ToString};

pub mod displayable;

use displayable::text_display::TextDisplay;


static WINDOW_ALLOCATOR: Once<Mutex<WindowAllocator>> = Once::new();
const WINDOW_ACTIVE_COLOR:u32 = 0xFFFFFF;
const WINDOW_INACTIVE_COLOR:u32 = 0x343C37;
const SCREEN_BACKGROUND_COLOR:u32 = 0x000000;

/// 10 pixel gap between windows 
pub const GAP_SIZE: usize = 10; 

struct WindowAllocator {
    allocated: VecDeque<Weak<Mutex<WindowInner>>>, 
    // a weak pointer directly to the active WindowInner so we don't have to search for the active window when we need it quickly
    // this weak pointer is set in the WindowAllocator's switch(), delete(), and allocate() functions
    active: Weak<Mutex<WindowInner>>, 
}

/// Applications call this function to request a new window object with a default size (mostly fills screen with GAP_SIZE around all borders)
/// If the caller a specific window size, it should call new_window()
pub fn new_default_window() -> Result<WindowObj, &'static str> {
    let (window_width, window_height) = get_screen_size();
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

    allocator.lock().allocate(x,y,width,height)
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
    allocator.switch();
    Ok(())
}

/// delete a window object
pub fn delete(window:WindowObj) -> Result<(), &'static str> {
    let mut allocator = try!(WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")).lock();
    // Switches to a new active window and sets 
    // the active pointer field of the window allocator to the new active window
    allocator.switch(); 
    allocator.delete(&(window.inner));
    Ok(())
}

/// Returns the width and height (in pixels) of the screen 
pub fn get_screen_size() -> (usize, usize) {
    return (frame_buffer::FRAME_BUFFER_WIDTH, frame_buffer::FRAME_BUFFER_HEIGHT);
}

impl WindowAllocator{
    fn allocate(&mut self, x:usize, y:usize, width:usize, height:usize) -> Result<WindowObj, &'static str>{
        if width < 4 || height < 4 {
            return Err("Window size must be greater than 4");
        }
        if x + width >= frame_buffer::FRAME_BUFFER_WIDTH
       || y + height >= frame_buffer::FRAME_BUFFER_HEIGHT {
            return Err("Requested area extends the screen size");
        }

        let consumer = DFQueue::new().into_consumer();
        let producer = consumer.obtain_producer();

        //new_window = ||{WindowObj{x:x,y:y,width:width,height:height,active:true}};

        let inner = WindowInner{
            x:x,
            y:y,
            width:width,
            height:height,
            active:true,
            margin:2,
            key_producer:producer,
        };

        let inner_ref = Arc::new(Mutex::new(inner));

        let overlapped = self.check_overlap(&inner_ref, x, y, width, height);       
        if overlapped  {
            return Err("Request area is already allocated");
        }

        {
            let inner_lock = inner_ref.lock();
            inner_lock.clean();
            inner_lock.draw_border(get_border_color(true));
        }
  
        for item in self.allocated.iter_mut(){
            let ref_opt = item.upgrade();
            if let Some(reference) = ref_opt {
                reference.lock().active(false);
            }
        }

        let weak_ref = Arc::downgrade(&inner_ref);
        self.active = weak_ref.clone();
        self.allocated.push_back(weak_ref);

        let window: WindowObj = WindowObj{
            inner:inner_ref,
            //text_buffer:FrameTextBuffer::new(),
            consumer:consumer,
            components:BTreeMap::new(),
        };    
        
        Ok(window)    
    }

    /// Switches to the next active window in the window allocator list
    /// This function returns the index of the previously active window in the window_allocator.allocated vector
    fn switch(&mut self) -> usize {
        let mut flag = false;
        let mut i = 0;
        let mut prev_active = 0;
        for item in self.allocated.iter_mut(){
            let reference = item.upgrade();
            if let Some(window) = reference {
                let mut window = window.lock();
                if flag {
                    (*window).active(true);
                    self.active = item.clone(); // clones the weak pointer to put in the active field
                    flag = false;
                    self.active = item.clone();
                } else if window.active {
                    (*window).active(false);
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
                    (*window).active(true);
                    self.active = item.clone(); // clones the weak pointer to put into the active field
                    break;
                }
            }
        }

        return prev_active;
    }

    fn delete(&mut self, inner:&Arc<Mutex<WindowInner>>){
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

        let inner_lock = inner.lock();
        inner_lock.clean();
        inner_lock.draw_border(SCREEN_BACKGROUND_COLOR);
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
            let mut window = window.lock();
            window.key_producer.enqueue(event);
        }
        Ok(())
    }

}

/// a window object
pub struct WindowObj {
    inner:Arc<Mutex<WindowInner>>,
    //text_buffer:FrameTextBuffer,
    consumer:DFQueueConsumer<Event>,
    components:BTreeMap<String, TextDisplay>,
}


impl WindowObj{
    fn clean(&self) {
        let inner = self.inner.lock();
        inner.clean();
    }

    ///Add a new displayable structure to the window
    ///We check if the displayable is in the window. But we do not check if it is overlapped with others
    pub fn add_displayable(&mut self, key: &str, displayable:TextDisplay) -> Result<(), &'static str>{
        //TODO check fit
        let key = key.to_string();
        let (x, y, width, height) = displayable.get_size();
        let inner = self.inner.lock();
        if !inner.check_in_content(inner.x + inner.margin + x, inner.y + inner.margin + y) ||
            !inner.check_in_content(inner.x + inner.margin + x + width, inner.y + inner.margin + y) ||
            !inner.check_in_content(inner.x + inner.margin + x, inner.y + inner.margin + y + height) ||
            !inner.check_in_content(inner.x + inner.margin + x + width, inner.y + inner.margin + y + height) {
            return Err("The displayable does not fit the window size.");
        } 

        self.components.insert(key, displayable);
        Ok(())
    }

    pub fn remove_displayable(&mut self, key:&str){
        self.components.remove(key);
    }

    pub fn get_displayable(&self, key:&str) -> Option<&TextDisplay> {
        return self.components.get(key);
    }

    ///Get the content position of the displayable excluding border and padding
    pub fn get_content_position(&self) -> (usize, usize) {
        let inner = self.inner.lock();
        (inner.x + inner.margin, inner.y + inner.margin)
    }

    /// draw a pixel in a window
    pub fn draw_pixel(&self, x:usize, y:usize, color:u32){
        let inner = self.inner.lock();
        if x >= inner.width - 2 || y >= inner.height - 2 {
            return;
        }
        frame_buffer::draw_pixel(x + inner.x + 1, y + inner.y + 1, color);
    }

    /// draw a line in a window
    pub fn draw_line(&self, start_x:usize, start_y:usize, end_x:usize, end_y:usize, color:u32){
        let inner = self.inner.lock();
        if start_x > inner.width - 2
            || start_y > inner.height - 2
            || end_x > inner.width - 2
            || end_y > inner.height - 2 {
            return;
        }
        frame_buffer::draw_line(start_x + inner.x + 1, start_y + inner.y + 1, 
            end_x + inner.x + 1, end_y + inner.y + 1, color);
    }

    /// draw a square in a window
    pub fn draw_rectangle(&self, x:usize, y:usize, width:usize, height:usize, color:u32){
        let inner = self.inner.lock();
        if x + width > inner.width - 2
            || y + height > inner.height - 2 {
            return;
        }
        frame_buffer::draw_rectangle(x + inner.x + 1, y + inner.y + 1, width, height, 
            color);
    }

    pub fn disable_cursor(&mut self) {
       // self.text_buffer.cursor.disable();
    }

    pub fn set_cursor(&mut self, line:u16, column:u16, reset:bool) {
        /*let cursor = &mut (self.text_buffer.cursor);
        cursor.enable();
        cursor.update(line as usize, column as usize, reset);
        let inner = self.inner.lock();
        frame_buffer::fill_rectangle(inner.x + inner.margin + (column as usize) * CHARACTER_WIDTH, 
                        inner.y + inner.margin + (line as usize) * CHARACTER_HEIGHT, 
                        CHARACTER_WIDTH, CHARACTER_HEIGHT, FONT_COLOR);*/
    }

    pub fn cursor_blink(&mut self) {
        /*let cursor = &mut (self.text_buffer.cursor);
        if cursor.blink() {
            let (line, column, show) = cursor.get_info();
            let inner = self.inner.lock();
            let color = if show { 0xFFFFFF } else { 0 };
            frame_buffer::fill_rectangle(inner.x + inner.margin + column * CHARACTER_WIDTH, 
                        inner.y + inner.margin + line * CHARACTER_HEIGHT, 
                        CHARACTER_WIDTH, CHARACTER_HEIGHT, color);
        }*/
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

    pub fn resize(&mut self, x:usize, y:usize, width:usize, height:usize) -> Result<(), &'static str>{
        { // checks for overlap
            let inner = self.inner.clone();
            let mut allocator = try!(WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")).lock();
            match allocator.check_overlap(&inner, x,y,width,height) {
                true => {return Err("cannot resize because requested resize will cause overlap")}
                false => { }
            }

        }
        
        let mut inner = self.inner.lock();
        let rs = inner.resize(x,y,width,height);
        if rs.is_err(){
            Err(rs.unwrap_err())
        } else {
            let percent = rs.unwrap();
            for (_key, item) in self.components.iter_mut() {
                let (x, y, width, height) = item.get_size();
                item.resize(x*percent.0/100, y*percent.1/100, width*percent.0/100, height*percent.1/100);
            }
            inner.key_producer.enqueue(Event::new_resize_event(x, y, width, height));
            Ok(())
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
    /// the upper left x-coordinate of inner:&Arc<Mutex<WindowInner>>
    x: usize,
    /// the upper left y-coordinate of inner:&Arc<Mutex<WindowInner>>
    y:usize,
    /// the width of the window
    width:usize,
    /// the height of the window
    height:usize,
    /// whether the window is active
    active:bool,
    /// a consumer of key input events inner:&Arc<Mutex<WindowInner>>
    margin:usize,
    /// the producer accepting a key event
    key_producer:DFQueueProducer<Event>,
}

impl WindowInner {
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

    fn check_in_area(&self, x:usize, y:usize) -> bool {        
        return x>= self.x && x <= self.x+self.width 
                && y>=self.y && y<=self.y+self.height;
    }

    fn check_in_content(&self, x:usize, y:usize) -> bool {        
        return x>= self.x+self.margin && x <= self.x+self.width-self.margin 
                && y>=self.y+self.margin && y<=self.y+self.height-self.margin;
    }

    fn active(&mut self, active:bool){
        self.active = active;
        self.draw_border(get_border_color(active));
    }

    fn clean(&self) {
        frame_buffer::fill_rectangle(self.x + 1, self.y + 1, self.width - 2, self.height - 2, SCREEN_BACKGROUND_COLOR);
    }

    fn draw_border(&self, color:u32) -> (usize, usize, usize){        
        frame_buffer::draw_line(self.x, self.y, self.x+self.width, self.y, color);
        frame_buffer::draw_line(self.x, self.y + self.height-1, self.x+self.width, self.y+self.height-1, color);
        frame_buffer::draw_line(self.x, self.y+1, self.x, self.y+self.height-1, color);
        frame_buffer::draw_line(self.x+self.width-1, self.y+1, self.x+self.width-1, self.y+self.height-1, color);        
        (self.x, self.y, self.margin)
    }

    /// adjust the size of a window
    fn resize(&mut self, x:usize, y:usize, width:usize, height:usize) -> Result<(usize, usize), &'static str> {
        self.draw_border(SCREEN_BACKGROUND_COLOR);
        self.clean();
        let percent = ((width-self.margin)*100/(self.width-self.margin), (height-self.margin)*100/(self.height-self.margin));
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height; 
        self.draw_border(get_border_color(self.active));
        Ok(percent)
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
    let window_height = (frame_buffer::FRAME_BUFFER_HEIGHT - GAP_SIZE * (num_windows + 2))/(num_windows + 1); 
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
