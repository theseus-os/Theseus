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
extern crate frame_buffer_3d;
extern crate input_event_types;
extern crate spawn;
extern crate pit_clock;


#[macro_use] extern crate log;
#[macro_use] extern crate util;

extern crate acpi;


use spin::{Once, Mutex};
use alloc::{VecDeque, String};
use alloc::btree_map::BTreeMap;
use core::ops::{DerefMut, Deref};
use dfqueue::{DFQueue,DFQueueConsumer,DFQueueProducer};
use alloc::arc::{Arc, Weak};
use frame_buffer::text_buffer::{FrameTextBuffer};
use input_event_types::Event;

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
    active: Weak<Mutex<WindowInner>>, // a weak pointers directly to the active WindowInner so we don't have to search for the active window when we need it quickly
}

/// switch the active window. Active the next window in the list based on the order they are created
pub fn switch() -> Result<(), &'static str> {
    let mut allocator = try!(WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")).lock();
    allocator.switch();
    Ok(())
}

/// new a window object and return a strong reference to it
pub fn new_window<'a>(x:usize, y:usize, width:usize, height:usize) -> Result<WindowObj, &'static str>{

    let allocator: &Mutex<WindowAllocator> = WINDOW_ALLOCATOR.call_once(|| {
        Mutex::new(WindowAllocator{
            allocated:VecDeque::new(),
            active:Weak::new(),
        })
    });

    allocator.lock().allocate(x,y,width,height)
}

/// delete a window object
pub fn delete<'a>(window:WindowObj) -> Result<(), &'static str> {
    let mut allocator = try!(WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")).lock();
    allocator.delete(&(window.inner));
    Ok(())
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
            text_buffer:FrameTextBuffer::new(),
            consumer:consumer,
            components:BTreeMap::new(),
        };    
        
        Ok(window)    
    }

    fn switch(&mut self) -> Option<&'static str>{
        let mut flag = false;
        for item in self.allocated.iter_mut(){
            let reference = item.upgrade();
            if let Some(window) = reference {
                let mut window = window.lock();
                if flag {
                    (*window).active(true);
                    self.active = item.clone();
                    flag = false;
                    self.active = item.clone();
                } else if window.active {
                    (*window).active(false);
                    flag = true;
                }
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

        Some("End")
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
                let reference = self.allocated.get(i).unwrap().upgrade();
                if let Some(allocated_ref) = reference {
                    if !Arc::ptr_eq(&allocated_ref, inner) {
                        if allocated_ref.lock().is_overlapped(x, y, width, height) {
                            return true;
                        }
                    }
                    i += 1;
                } else {
                    self.allocated.remove(i);
                    len -= 1;
                }
            }
        }
        false
    }
    
    /// enqueues the key event into the active window
    fn put_key_code(&mut self, event:Event) -> Result<(), &'static str> {
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
    text_buffer:FrameTextBuffer,
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
    pub fn add_displayable(&mut self, key:String, displayable:TextDisplay) -> Result<(), &'static str>{
        //TODO check fit
        let (x, y, width, height) = displayable.get_size();
        let inner = self.inner.lock();
        if !inner.check_in_content(inner.x + inner.margin + x, inner.y + inner.margin + y) ||
            !inner.check_in_content(inner.x + inner.margin + x + width, inner.y + inner.margin + y) ||
            !inner.check_in_content(inner.x + inner.margin + x, inner.y + inner.margin + y + height) ||
            !inner.check_in_content(inner.x + inner.margin + x + width, inner.y + inner.margin + y + height) {
            return Err("The displayable does not fit the window size.");
        } 

        self.components.insert(key.clone(), displayable);
        Ok(())
    }

    ///Remove a displayable
    pub fn remove_displayable(&mut self, key:&String){
        self.components.remove(key);
    }

    ///Get a displayable by key
    pub fn get_displayable(&self, key:&String) -> Option<&TextDisplay> {
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

    ///resize a window and resize its displayable components proportionally
    pub fn resize(&mut self, x:usize, y:usize, width:usize, height:usize) -> Result<(), &'static str>{
        let mut inner = self.inner.lock();
        let rs = inner.resize(x,y,width,height);
        if rs.is_err(){
            Err(rs.unwrap_err())
        } else {
            let percent = rs.unwrap();
            for (key, item) in self.components.iter_mut() {
                let (x, y, width, height) = item.get_size();
                item.resize(x*percent.0/100, y*percent.1/100, width*percent.0/100, height*percent.1/100);
            }
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
    /// the upper left x-coordinate of the window
    x: usize,
    /// the upper left y-coordinate of the window
    y:usize,
    /// the width of the window
    width:usize,
    /// the height of the window
    height:usize,
    /// whether the window is active
    active:bool,
    /// a consumer of key input events
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
        //Check overlap
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

/// put key input events in the producer of window manager
pub fn put_key_code(event: Event) -> Result<(), &'static str>{    
    let mut allocator = try!(WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")).lock();
    allocator.put_key_code(event)
}

/// Finds and deletes the active window from the window manager, and sets the new active window
pub fn delete_active_window() -> Option<()> {
    // finds and deletes the window from the window manager
    let mut active_ref: Arc<Mutex<WindowInner>>;
    {   
        let mut i = 0; // tracks the index of the to-be-deleted window
        let mut locked_window_ptrs = try_opt!(WINDOW_ALLOCATOR.try()).lock();
        active_ref = locked_window_ptrs.deref_mut().allocated.get(0).unwrap().upgrade().unwrap(); // for variable initialization
        if locked_window_ptrs.deref_mut().allocated.len() == 1 {
            return Some(()); // does not proceed with delete if there is only one window currently running
        }
        // finding the active window and it's position in the WindowAllocator vector so that we can switch the active window to an adjacent one
        let num_windows = locked_window_ptrs.deref_mut().allocated.len();
        for window_ptr in locked_window_ptrs.deref_mut().allocated.iter() {
            let strong_window_ptr = window_ptr.upgrade();
            if let Some(window_inner_ref) = strong_window_ptr {
                {   
                    if window_inner_ref.lock().active {
                        active_ref = window_inner_ref; // finds the active WindowInner
                        break;                
                    }
                }
            }
            i += 1;
        }    
        locked_window_ptrs.delete(&active_ref); //deletes the active window
        if i == num_windows -1 {
            i -= 1; // it the user deletes the last window, the new active window will be the one right above it
        } else { // otherwise, the new active window is the one below it
        }
        
        let new_active_ref;
        {  // sets the selected window to be the new active window
            let active_ref = &locked_window_ptrs.deref_mut().allocated[i];
            new_active_ref = active_ref.clone();
            let strong_ref = new_active_ref.upgrade();
            if let Some(strong_ref) = strong_ref {
                strong_ref.lock().active(true);
            }
        }
        locked_window_ptrs.deref_mut().active = new_active_ref.clone(); // updates the weak pointer to the new active window
    }

    // Readjusts the remaining windows to fill the space
    let _result = adjust_window_after_deletion();
    return Some(())
}

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
            let _result = locked_window_ptr.resize(GAP_SIZE, height_index, window_width, window_height);
            locked_window_ptr.key_producer.enqueue(Event::DisplayEvent); // refreshes display after resize
            height_index += window_height + GAP_SIZE; // advance to the height index of the next window
        }
    }
    Ok(())
}

/// Adjusts the windows preemptively so that we can add a new window directly below the old ones to maximize screen usage without overlap
pub fn adjust_windows_before_addition() -> Option<(usize, usize, usize)> {
    let mut allocator = try_opt!(WINDOW_ALLOCATOR.try()).lock();
    let num_windows = allocator.deref_mut().allocated.len();
    // one gap between each window and one gap between the edge windows and the frame buffer boundary
    let window_height = (frame_buffer::FRAME_BUFFER_HEIGHT - GAP_SIZE * (num_windows + 2))/(num_windows + 1); 
    let window_width = frame_buffer::FRAME_BUFFER_WIDTH - 2 * GAP_SIZE; // refreshes display after resize
    let mut height_index = GAP_SIZE; // start resizing the windows after the first gap 

    // Resizes the windows vertically
    for window_inner_ref in allocator.deref_mut().allocated.iter_mut() {
        let strong_ptr = window_inner_ref.upgrade();
        if let Some(window_inner_ptr) = strong_ptr {
            let mut locked_window_ptr = window_inner_ptr.lock();
            let _result = locked_window_ptr.resize(GAP_SIZE, height_index, window_width, window_height);
            locked_window_ptr.key_producer.enqueue(Event::DisplayEvent); // refreshes window after  
            height_index += window_height + GAP_SIZE; // advance to the height index of the next window
        }
    }

    return Some((height_index, window_width, window_height)); // returns the index at which the new window should be drawn
}

fn get_border_color(active:bool) -> u32 {
    if active {
        WINDOW_ACTIVE_COLOR
    } else {
        WINDOW_INACTIVE_COLOR
    }
}
