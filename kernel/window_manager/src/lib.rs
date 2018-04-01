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
#[macro_use] extern crate frame_buffer;
#[macro_use] extern crate log;


use spin::Once;
use irq_safety::MutexIrqSafe;
use alloc::{Vec, LinkedList};
use core::ops::{DerefMut, Deref};
use dfqueue::{DFQueue,DFQueueConsumer,DFQueueProducer};
use keycodes_ascii::Keycode;
use alloc::arc::Arc;




static WINDOW_ALLOCATOR: Once<MutexIrqSafe<WindowAllocator>> = Once::new();

static KEY_CODE_CONSUMER: Once<DFQueueConsumer<Keycode>> = Once::new();
static KEY_CODE_PRODUCER: Once<DFQueueProducer<Keycode>> = Once::new();


pub struct WindowAllocator {
    allocated: LinkedList<Window_Obj>, //The last one is active
}


pub fn window_switch(){
    let allocator = WINDOW_ALLOCATOR.try();
    if allocator.is_some(){    
        allocator.unwrap().lock().deref_mut().switch();
    }
    let consumer = KEY_CODE_CONSUMER.try();
    if(consumer.is_none()){
        return
    }

    //Clear all key events before switch
    let consumer = consumer.unwrap();
    let mut event = consumer.peek();
    while(event.is_some()){
        event.unwrap().mark_completed();
        event = consumer.peek();
    }
}

#[macro_export]
macro_rules! window_switch {
    () => ({
        $crate::window_switch();
    });
}

pub fn get_window_obj<'a>(x:usize, y:usize, width:usize, height:usize) -> Result<*const Window_Obj, &'static str>{

    let allocator: &MutexIrqSafe<WindowAllocator> = WINDOW_ALLOCATOR.call_once(|| {
        MutexIrqSafe::new(WindowAllocator{allocated:LinkedList::new()})
    });


    allocator.lock().deref_mut().allocate(x,y,width,height)
}

pub fn print_all() {

    let allocator: &MutexIrqSafe<WindowAllocator> = WINDOW_ALLOCATOR.call_once(|| {
        MutexIrqSafe::new(WindowAllocator{allocated:LinkedList::new()})
    });


    allocator.lock().deref_mut().print();
}


impl WindowAllocator{
    pub fn allocate(&mut self, x:usize, y:usize, width:usize, height:usize) -> Result< *const Window_Obj, &'static str>{
        if (width < 2 || height < 2){
            return Err("Window size must be greater than 2");
        }
        if(x + width >= frame_buffer::FRAME_BUFFER_WIDTH
       || y + height >= frame_buffer::FRAME_BUFFER_HEIGHT){
            return Err("Requested area extends the screen size");
        }

        //new_window = ||{Window_Obj{x:x,y:y,width:width,height:height,active:true}};
        let mut window:Window_Obj = Window_Obj{x:x,y:y,width:width,height:height,active:true, consumer:None};
        unsafe{ 
            //window.resize(x,y,width,height);
            let consumer = KEY_CODE_CONSUMER.call_once(||DFQueue::new().into_consumer());
            let mut overlapped = false;
            for allocated_window in self.allocated.iter(){
                if window.is_overlapped(allocated_window) {
                    overlapped = true;
                    break;
                }
            }
            if overlapped  {
                trace!("Request area is already allocated");
                return Err("Request area is already allocated");
            }
            for mut allocated_window in  self.allocated.iter_mut(){
                allocated_window.active(false);
            }
            //window.fill(0xffffff);

            window.consumer = Some(consumer);
            window.draw_border();
            self.allocated.push_back(window);

            let reference = self.allocated.back().unwrap() as *const Window_Obj; 

            
            Ok(reference)
        }
    }

    pub fn switch(&mut self){

        let mut flag = false;
        for window in self.allocated.iter_mut(){
            if flag {
                window.active(true);
                flag = false;
            } else if window.active {
                window.active(false);
                flag = true;
            }
        }
        if (flag) {
            let window = self.allocated.front_mut().unwrap();
            window.active(true);
        }
    }

    pub fn print(&self) {

        for allocated_window in self.allocated.iter(){
                allocated_window.y, allocated_window.width, allocated_window.height, 
                allocated_window.active, allocated_window.consumer.is_none());

        }
       
    }

}

#[derive(Copy, Clone)]
pub struct Window_Obj {
    x: usize,
    y:usize,
    width:usize,
    height:usize,
    active:bool,

    consumer: Option<&'static DFQueueConsumer<Keycode>>,
}


impl Window_Obj{
    pub fn is_overlapped(&self, window:&Window_Obj) -> bool {
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

    pub fn resize(&mut self, x:usize, y:usize, width:usize, height:usize){
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height;
    }

    fn active(&mut self, active:bool){
        self.active = active;
        if (active && self.consumer.is_none()) {
            let consumer = KEY_CODE_CONSUMER.try();
            self.consumer = consumer;
        }
        if(!active && self.consumer.is_some()){
            
            self.consumer = None;
        }
        self.draw_border();
        //print_all();
    }

    fn draw_border(&self){
        let mut color = 0x343c37;
        if self.active { color = 0xffffff; }
        draw_line!(self.x, self.y, self.x+self.width, self.y, color);
        draw_line!(self.x, self.y + self.height-1, self.x+self.width, self.y+self.height-1, color);
        draw_line!(self.x, self.y+1, self.x, self.y+self.height-1, color);
        draw_line!(self.x+self.width-1, self.y+1, self.x+self.width-1, self.y+self.height-1, color);        
    }

    fn fill(&self, color:usize){
        draw_square!(self.x+1, self.y+1, self.width-2, self.height-2, color);
    }

    pub fn print (&self){
        trace!("x: {}, y:{}, w:{}, h:{}, active: {}, consumer: {}", self.x, self.y, self.width, self.height, self.active, self.consumer.is_none());    
    }

    pub fn get_key_code(&self) -> Option<Keycode> {
        if (self.consumer.is_some()) {

            let event = self.consumer.unwrap().peek();
            if event.is_none() {
                return None; 
            }   

            let event = event.unwrap();
            let event_data = event.deref().clone(); // event.deref() is the equivalent of   &*event
            event.mark_completed();
            return Some(event_data);
        }
        None
    }

    pub fn draw_pixel(&self, x:usize, y:usize, color:usize){
        self.print();
        if x >= self.width - 2 || y >= self.height - 2 {
            return;
        }
        frame_buffer::draw_pixel(x + self.x + 1, y + self.y + 1, color);
    }

    pub fn draw_line(&self, start_x:usize, start_y:usize, end_x:usize, end_y:usize, color:usize){
        if start_x > self.width - 2
            || start_y > self.height - 2
            || end_x > self.width - 2
            || end_y > self.height - 2 {
            return;
        }
        frame_buffer::draw_line(start_x + self.x + 1, start_y + self.y + 1, end_x + self.x + 1, end_y + self.y + 1, color);
    }

    pub fn draw_square(&self, x:usize, y:usize, width:usize, height:usize, color:usize){
        self.print();
        if x + width > self.width - 2
            || y + height > self.height - 2 {
            return;
        }
        frame_buffer::draw_square(x + self.x + 1, y + self.y + 1, width, height, color);
    }
}

pub fn put_key_code(keycode:Keycode) -> Result<(), &'static str>{

    let consumer = KEY_CODE_CONSUMER.try();
    if consumer.is_none(){
        return Err("No active window");
    }
    let producer = KEY_CODE_PRODUCER.call_once(|| {
        consumer.unwrap().obtain_producer()
    });

    let producer = KEY_CODE_PRODUCER.try();
    if(producer.is_none()){
        return Err("Couldn't init key code producer");
    }
    
    producer.unwrap().enqueue(keycode);
    Ok(())
}
