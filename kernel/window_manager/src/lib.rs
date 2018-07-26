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
extern crate text_display;


use spin::{Once, Mutex};
use alloc::{VecDeque};
use core::ops::{DerefMut, Deref};
use dfqueue::{DFQueue,DFQueueConsumer,DFQueueProducer};
use alloc::arc::{Arc, Weak};
use frame_buffer::font::{CHARACTER_WIDTH, CHARACTER_HEIGHT};
use frame_buffer::text_buffer::{FONT_COLOR, BACKGROUND_COLOR, FrameTextBuffer};
use text_display::TextDisplay;
use input_event_types::Event;


//static mut COUNTER:usize = 0; //For performance evaluation

/// A test mod of window manager
pub mod test_window_manager;


static WINDOW_ALLOCATOR: Once<Mutex<WindowAllocator>> = Once::new();
const WINDOW_ACTIVE_COLOR:u32 = 0xFFFFFF;
const WINDOW_INACTIVE_COLOR:u32 = 0x343C37;
pub static GAP_SIZE: usize = 10; // 10 pixel gap between windows 


struct WindowAllocator {
    allocated: VecDeque<Weak<Mutex<WindowInner>>>, 
    active: Weak<Mutex<WindowInner>>, // a weak pointers directly to the active WindowInner so we don't have to search for the active window when we need it quickly
}

/// switch the active window
pub fn window_switch() -> Result<(), &'static str> {
    let mut allocator = try!(WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")).lock();
    allocator.switch();
    Ok(())
}

/// new a window object and return it
pub fn get_window_obj<'a>(x:usize, y:usize, width:usize, height:usize) -> Result<WindowObj, &'static str>{

    let allocator: &Mutex<WindowAllocator> = WINDOW_ALLOCATOR.call_once(|| {
        Mutex::new(WindowAllocator{
            allocated:VecDeque::new(),
            active:Weak::new(),
        })
    });

    allocator.lock().allocate(x,y,width,height)
}

/// delete a window object
pub fn delete_window<'a>(window:WindowObj) -> Result<(), &'static str> {
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
        //TODO clean contents in window
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
        inner_lock.draw_border(BACKGROUND_COLOR);
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
}


impl WindowObj{

    fn clean(&self) {
        let inner = self.inner.lock();
        inner.clean();
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
        self.text_buffer.cursor.disable();
    }

    pub fn set_cursor(&mut self, line:u16, column:u16, reset:bool) {
        let cursor = &mut (self.text_buffer.cursor);
        cursor.enable();
        cursor.update(line as usize, column as usize, reset);
        let inner = self.inner.lock();
        frame_buffer::fill_rectangle(inner.x + inner.margin + (column as usize) * CHARACTER_WIDTH, 
                        inner.y + inner.margin + (line as usize) * CHARACTER_HEIGHT, 
                        CHARACTER_WIDTH, CHARACTER_HEIGHT, FONT_COLOR);
    }

    pub fn cursor_blink(&mut self) {
        let cursor = &mut (self.text_buffer.cursor);
        if cursor.blink() {
            let (line, column, show) = cursor.get_info();
            let inner = self.inner.lock();
            let color = if show { FONT_COLOR } else { BACKGROUND_COLOR };
            frame_buffer::fill_rectangle(inner.x + inner.margin + column * CHARACTER_WIDTH, 
                        inner.y + inner.margin + line * CHARACTER_HEIGHT, 
                        CHARACTER_WIDTH, CHARACTER_HEIGHT, color);
        }
    }

    /// Returns a tuple containing (buffer height, buffer width)
    pub fn get_dimensions(&self) -> (usize, usize) {
        let inner = self.inner.lock();
        ((inner.width-2*inner.margin)/CHARACTER_WIDTH, (inner.height-2*inner.margin)/CHARACTER_HEIGHT)
    }

    /// Requires that a str slice that will exactly fit the frame buffer
    /// The calculation is done inside the console crate by the print_by_bytes function and associated methods
    /// Print every byte and fill the blank with background color
    pub fn display_string(&mut self, slice: &str) -> Result<(), &'static str> {
        let inner = self.inner.lock();
        self.text_buffer.print_by_bytes(inner.x + inner.margin, inner.y + inner.margin, 
            inner.width - 2 * inner.margin, inner.height - 2 * inner.margin, 
            slice)
    }
    
    /*
    pub fn draw_border(&self) -> (usize, usize, usize){
        let inner = self.inner.lock();
        inner.draw_border(get_border_color(inner.active))
    }
    */


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

    fn active(&mut self, active:bool){
        self.active = active;
        self.draw_border(get_border_color(active));
    }

    fn clean(&self) {
        frame_buffer::fill_rectangle(self.x + 1, self.y + 1, self.width - 2, self.height - 2, BACKGROUND_COLOR);
    }

    fn draw_border(&self, color:u32) -> (usize, usize, usize){
        
        frame_buffer::draw_line(self.x, self.y, self.x+self.width, self.y, color);
        frame_buffer::draw_line(self.x, self.y + self.height-1, self.x+self.width, self.y+self.height-1, color);
        frame_buffer::draw_line(self.x, self.y+1, self.x, self.y+self.height-1, color);
        frame_buffer::draw_line(self.x+self.width-1, self.y+1, self.x+self.width-1, self.y+self.height-1, color);        
        (self.x, self.y, self.margin)
    }

    /// adjust the size of a window
    fn resize(&mut self, x:usize, y:usize, width:usize, height:usize) -> Result<(), &'static str> {
        self.draw_border(BACKGROUND_COLOR);
        self.clean();
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height; 
        self.draw_border(get_border_color(self.active));
        Ok(())
    }


}

/// put key input events in the producer of window manager
pub fn put_key_code(event: Event) -> Result<(), &'static str>{    
    let mut allocator = try!(WINDOW_ALLOCATOR.try().ok_or("The window allocator is not initialized")).lock();
    allocator.put_key_code(event)
}

fn get_border_color(active:bool) -> u32 {
    if active {
        WINDOW_ACTIVE_COLOR
    } else {
        WINDOW_INACTIVE_COLOR
    }
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
            locked_window_ptr.key_producer.enqueue(Event::DisplayEvent); // refreshes window after resize 
            height_index += window_height + GAP_SIZE; // advance to the height index of the next window
        }
    }


    return Some((height_index, window_width, window_height)); // returns the index at which the new window should be drawn
}







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
  }
}*/


