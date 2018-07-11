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
#[macro_use] extern crate frame_buffer_3d;


#[macro_use] extern crate log;
#[macro_use] extern crate util;

extern crate acpi;



use spin::{Once, Mutex};
use irq_safety::MutexIrqSafe;
use alloc::{VecDeque};
use core::ops::{DerefMut, Deref};
use dfqueue::{DFQueue,DFQueueConsumer,DFQueueProducer};
use keycodes_ascii::Keycode;
use alloc::arc::{Arc, Weak};
//use acpi::get_hpet;


//static mut STARTING_TIME:u64 = 0;
//static mut COUNTER:usize = 0; //For performance evaluation

/// A test mod of window manager
pub mod test_window_manager;


static WINDOW_ALLOCATOR: Once<MutexIrqSafe<WindowAllocator>> = Once::new();

static KEY_CODE_CONSUMER: Once<DFQueueConsumer<Keycode>> = Once::new();
static KEY_CODE_PRODUCER: Once<DFQueueProducer<Keycode>> = Once::new();


struct WindowAllocator {
    allocated: VecDeque<Arc<Mutex<WindowObj>>>, //The last one is active
}

/// switch the active window
pub fn window_switch() -> Option<&'static str>{

    let allocator = try_opt!(WINDOW_ALLOCATOR.try());
//    unsafe { allocator.force_unlock(); }
    allocator.lock().deref_mut().switch();


    let consumer = try_opt!(KEY_CODE_CONSUMER.try());

    //Clear all key events before switch
    let mut event = consumer.peek();
    while event.is_some() {
        try_opt!(event).mark_completed();
        event = consumer.peek();
    }

    Some("End")
}

/// new a window object and return it
pub fn get_window_obj<'a>(x:usize, y:usize, width:usize, height:usize) -> Result<Arc<Mutex<WindowObj>>, &'static str>{

    let allocator: &MutexIrqSafe<WindowAllocator> = WINDOW_ALLOCATOR.call_once(|| {
        MutexIrqSafe::new(WindowAllocator{allocated:VecDeque::new()})
    });

    allocator.lock().deref_mut().allocate(x,y,width,height)
}

/// new a window object and return it
pub fn delete_window<'a>(window:&Arc<Mutex<WindowObj>>) {
    let allocator: &MutexIrqSafe<WindowAllocator> = WINDOW_ALLOCATOR.call_once(|| {
        MutexIrqSafe::new(WindowAllocator{allocated:VecDeque::new()})
    });
    allocator.lock().deref_mut().delete(window)
}

pub fn check_reference() {
    let allocator: &MutexIrqSafe<WindowAllocator> = WINDOW_ALLOCATOR.call_once(|| {
        MutexIrqSafe::new(WindowAllocator{allocated:VecDeque::new()})
    });
    allocator.lock().check_reference();
}


impl WindowAllocator{
    fn allocate(&mut self, x:usize, y:usize, width:usize, height:usize) -> Result<Arc<Mutex<WindowObj>>, &'static str>{
        if width < 2 || height < 2 {
            return Err("Window size must be greater than 2");
        }
        if x + width >= frame_buffer::FRAME_BUFFER_WIDTH
       || y + height >= frame_buffer::FRAME_BUFFER_HEIGHT {
            return Err("Requested area extends the screen size");
        }

        //new_window = ||{WindowObj{x:x,y:y,width:width,height:height,active:true}};
        let mut window:WindowObj = WindowObj{x:x,y:y,width:width,height:height,active:true, consumer:None};

        //window.resize(x,y,width,height);
        let consumer = KEY_CODE_CONSUMER.call_once(||DFQueue::new().into_consumer());
        //let overlapped = self.check_overlap(&window);
        let mut overlapped = false;
        let mut len = self.allocated.len();
        let mut i = 0;
        while i < len {
            let mut remove = false;
            {   


                let item = self.allocated.get(i).unwrap();
                trace!("Wenqiu: reference number {}",Arc::strong_count(item));

                let allocated_window = item.lock();
                if window.is_overlapped(&(*allocated_window)) {
                    if Arc::weak_count(item) > 1 {
                        overlapped = true;
                    } else {
                        remove = true;
                    }
                }
            }
            if remove {
                self.allocated.remove(i);
                len -= 1;
            } else {
                i += 1;
            }
        }

        if overlapped  {
            trace!("Request area is already allocated");
            return Err("Request area is already allocated");
        }
        for item in self.allocated.iter_mut(){
            let mut allocated_window = item.lock();
            (*allocated_window).active(false);
        }
        //window.fill(0xffffff);

        window.consumer = Some(consumer);
        window.draw_border();
        let reference = Arc::new(Mutex::new(window));
        trace!("wenqiu:reference number:{}", Arc::strong_count(&reference));
        self.allocated.push_back(reference.clone());
        trace!("wenqiu:reference number:{}", Arc::strong_count(&reference));
        self.check_reference();
        //let reference = self.allocated.back(), "WindowAllocator fails to get new window reference")); 
        Ok(reference)
    
    }

    fn switch(&mut self) -> Option<&'static str>{
        let mut flag = false;
        for item in self.allocated.iter_mut(){
//            unsafe{ item.force_unlock();}
            let mut window = item.lock();
            if flag {
                (*window).active(true);
                flag = false;
            } else if window.active {
                (*window).active(false);
                flag = true;
            }
        }
        if flag {
            let item = try_opt!(self.allocated.front_mut());
 //           unsafe{ item.force_unlock();}
            let mut window = item.lock();
            (*window).active(true);
        }

        Some("End")
    }

    fn delete(&mut self, window:&Arc<Mutex<WindowObj>>){
        let mut i = 0;
        let len = self.allocated.len();
        for item in self.allocated.iter(){
            if Arc::ptr_eq(item, window) {
                break;
            }
            i += 1;
        }
        if i < len {
            self.allocated.remove(i);
        }
    }

    fn check_overlap(&mut self, window:&WindowObj) -> bool {
        let mut len = self.allocated.len();
        let mut i = 0;
        while i < len {
            let mut remove = false;
            {   
                let item = self.allocated.get(i).unwrap();
                let allocated_window = item.lock();
                if window.is_overlapped(&(*allocated_window)) {
                    if Arc::weak_count(item) > 1 {
                        return true;
                    } else {
                        remove = true;
                    }
                }
            }
            if remove {
                self.allocated.remove(i);
                len -= 1;
            } else {
                i += 1;
            }
        }
        false
    }

    fn check_reference(&mut self) -> bool {
        if self.allocated.len() == 0{
            return false;
        }
        let item = self.allocated.get(0).unwrap();
        trace!("Wenqiu: reference number {}",Arc::strong_count(item));
        return false;
    }

}

/// a window object
pub struct WindowObj {
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
    consumer: Option<&'static DFQueueConsumer<Keycode>>,
}


impl WindowObj{
    fn is_overlapped(&self, window:&WindowObj) -> bool {
        if self.check_in_area(window.x, window.y)
            {return true;}
        if self.check_in_area(window.x, window.y+window.height)
            {return true;}
        if self.check_in_area(window.x+window.width, window.y)
            {return true;}
        if self.check_in_area(window.x+window.width, window.y+window.height)
            {return true;}
        false        
    } 

    fn check_in_area(&self, x:usize, y:usize) -> bool {
        return x>= self.x && x <= self.x+self.width && y>=self.y && y<=self.y+self.height;
    }

    /// adjust the size of a window
    pub fn resize(&mut self, x:usize, y:usize, width:usize, height:usize){
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height;
    }

    fn active(&mut self, active:bool){
        self.active = active;
        if active && self.consumer.is_none() {
            let consumer = KEY_CODE_CONSUMER.try();
            self.consumer = consumer;
        }
        if !active && self.consumer.is_some(){
            
            self.consumer = None;
        }
        self.draw_border();
        //print_all();
    }

    fn draw_border(&self){
        let mut color = 0x343c37;
        if self.active { color = 0xffffff; }
        frame_buffer::draw_line(self.x, self.y, self.x+self.width, self.y, color);
        frame_buffer::draw_line(self.x, self.y + self.height-1, self.x+self.width, self.y+self.height-1, color);
        frame_buffer::draw_line(self.x, self.y+1, self.x, self.y+self.height-1, color);
        frame_buffer::draw_line(self.x+self.width-1, self.y+1, self.x+self.width-1, self.y+self.height-1, color);        
    }

    fn get_key_code(&self) -> Option<Keycode> {
        if self.consumer.is_some() {

            let event_opt = try_opt!(self.consumer).peek();
            let event = try_opt!(event_opt);
            let event_data = event.deref().clone(); // event.deref() is the equivalent of   &*event
            event.mark_completed();
            return Some(event_data);
        }
        None
    }

    /// draw a pixel in a window
    pub fn draw_pixel(&self, x:usize, y:usize, color:u32){
        if x >= self.width - 2 || y >= self.height - 2 {
            return;
        }
        //frame_buffer::draw_pixel(x + self.x + 1, y + self.y + 1, 0, color, true);
        frame_buffer::draw_pixel(x + self.x + 1, y + self.y + 1, color);
        }

    /// draw a line in a window
    pub fn draw_line(&self, start_x:usize, start_y:usize, end_x:usize, end_y:usize, color:u32){
        if start_x > self.width - 2
            || start_y > self.height - 2
            || end_x > self.width - 2
            || end_y > self.height - 2 {
            return;
        }
//        frame_buffer::draw_line(start_x + self.x + 1, start_y + self.y + 1, 
 //           end_x + self.x + 1, end_y + self.y + 1, 0, color, true);
          frame_buffer::draw_line(start_x + self.x + 1, start_y + self.y + 1, 
            end_x + self.x + 1, end_y + self.y + 1, color);
    }

    /// draw a square in a window
    pub fn draw_square(&self, x:usize, y:usize, width:usize, height:usize, color:u32){
        if x + width > self.width - 2
            || y + height > self.height - 2 {
            return;
        }
        //frame_buffer::draw_square(x + self.x + 1, y + self.y + 1, width, height, 0, color, true);
        frame_buffer::draw_rectangle(x + self.x + 1, y + self.y + 1, width, height, 
            color);
    }
}

/// put key input events in the producer of window manager
pub fn put_key_code(keycode:Keycode) -> Result<(), &'static str>{

    let consumer = try_opt_err!(KEY_CODE_CONSUMER.try(), "No active window");

    let producer = KEY_CODE_PRODUCER.call_once(|| {
        consumer.obtain_producer()
    });
    
    producer.enqueue(keycode);
    Ok(())
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
    if STARTING_TIME == 0 {
        debug!("test_window_managers::calculate: No starting time!");
        return;
    }

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
