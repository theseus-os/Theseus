#![no_std]
#![feature(alloc)]
#![feature(const_fn)]
#![feature(unique)]
#![feature(unique)]
#![feature(asm)]

// Temp: Notes to andrew
// Screen pixel dimensions are 640 x 400

//deref_mut
//cursor
//framebuffer::

extern crate spin;
extern crate irq_safety;
extern crate alloc;
extern crate dfqueue;
extern crate keycodes_ascii;
extern crate frame_buffer;
#[macro_use] extern crate frame_buffer_3d;
extern crate input_event_types;
extern crate spawn;
extern crate terminal;


#[macro_use] extern crate log;
#[macro_use] extern crate util;

extern crate acpi;
extern crate text_display;



use spin::{Once, Mutex};
use irq_safety::MutexIrqSafe;
use alloc::{VecDeque};
use alloc::string::ToString;
use alloc::btree_map::BTreeMap;
use core::ops::{DerefMut, Deref};
use dfqueue::{DFQueue,DFQueueConsumer,DFQueueProducer, PeekedData};
use keycodes_ascii::Keycode;
use alloc::arc::{Arc, Weak};
use frame_buffer::font::{CHARACTER_WIDTH, CHARACTER_HEIGHT};
use frame_buffer::text_buffer::{FONT_COLOR, BACKGROUND_COLOR, FrameTextBuffer};
use text_display::TextDisplay;
use input_event_types::Event;

//use acpi::get_hpet;


//static mut STARTING_TIME:u64 = 0;
//static mut COUNTER:usize = 0; //For performance evaluation

/// A test mod of window manager
pub mod test_window_manager;


static WINDOW_ALLOCATOR: Once<Mutex<WindowAllocator>> = Once::new();

const WINDOW_ACTIVE_COLOR:u32 = 0xFFFFFF;
const WINDOW_INACTIVE_COLOR:u32 = 0x343C37;


struct WindowAllocator {
    allocated: VecDeque<Weak<Mutex<WindowInner>>>, //The last one is active
}

/// switch the active window
pub fn window_switch() -> Option<&'static str>{
    let allocator = try_opt!(WINDOW_ALLOCATOR.try());
    allocator.lock().switch();
    Some("End")
}

/// new a window object and return it
pub fn get_window_obj<'a>(x:usize, y:usize, width:usize, height:usize) -> Result<WindowObj, &'static str>{

    let allocator: &Mutex<WindowAllocator> = WINDOW_ALLOCATOR.call_once(|| {
        Mutex::new(WindowAllocator{allocated:VecDeque::new()})
    });

    allocator.lock().allocate(x,y,width,height)
}

/// delete a window object
pub fn delete_window<'a>(window:WindowObj) -> Option<&'static str> {
    let allocator = try_opt!(WINDOW_ALLOCATOR.try());
    allocator.lock().delete(&(window.inner));
    
    Some("End")
}

impl WindowAllocator{
    fn allocate(&mut self, x:usize, y:usize, width:usize, height:usize) -> Result<WindowObj, &'static str>{
        if width < 2 || height < 2 {
            return Err("Window size must be greater than 2");
        }
        if x + width >= frame_buffer::FRAME_BUFFER_WIDTH
       || y + height >= frame_buffer::FRAME_BUFFER_HEIGHT {
            return Err("Requested area extends the screen size");
        }

        let consumer = DFQueue::new().into_consumer();
        let producer = consumer.obtain_producer();

        //new_window = ||{WindowObj{x:x,y:y,width:width,height:height,active:true}};
        let mut inner = WindowInner{
            x:x,
            y:y,
            width:width,
            height:height,
            active:true,
            margin:2,
            key_producer:producer,
        };

        let overlapped = self.check_overlap(&inner);       
        if overlapped  {
            return Err("Request area is already allocated");
        }

        for item in self.allocated.iter_mut(){
            let ref_opt = item.upgrade();
            if ref_opt.is_some() {
                let reference = ref_opt.unwrap();
                reference.lock().active(false);
            }
        }

        let inner_ref = Arc::new(Mutex::new(inner));
        self.allocated.push_back(Arc::downgrade(&inner_ref));

        let mut window:WindowObj = WindowObj{
            inner:inner_ref,
            text_buffer:FrameTextBuffer::new(),
            consumer:consumer,
        };

        window.clean();
        window.draw_border();       
        
        Ok(window)    
    }

    fn switch(&mut self) -> Option<&'static str>{
        let mut flag = false;
        for item in self.allocated.iter_mut(){
            let reference = item.upgrade();
            if reference.is_some() {
                let mut window = reference.unwrap();
                //unsafe{ window.force_unlock();}
                let mut window = window.lock();
                if flag {
                    (*window).active(true);
                    flag = false;
                } else if window.active {
                    (*window).active(false);
                    flag = true;
                }
            }
        }
        if flag {
            for item in self.allocated.iter_mut(){
                let reference = item.upgrade();
                if reference.is_some() {
                    let mut window = reference.unwrap();
                    let mut window = window.lock();
                    (*window).active(true);
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
            if reference.is_some() {
                if Arc::ptr_eq(&(reference.unwrap()), inner) {
                    break;
                }
            }
            i += 1;
        }
        if i < len {
            self.allocated.remove(i);
        }

        let inner_lock = inner.lock();
        inner_lock.clean();
        inner_lock.draw_border(BACKGROUND_COLOR);

    }

    fn check_overlap(&mut self, inner:&WindowInner) -> bool {

        let mut len = self.allocated.len();
        let mut i = 0;
        while i < len {
            let mut remove = false;
            {   
                let reference = self.allocated.get(i).unwrap().upgrade();
                if reference.is_some() {
                    let inner_ref = reference.unwrap();
                    let inner_lock = inner_ref.lock();
                    if inner.is_overlapped(&(*inner_lock)) {
                        return true;
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

    fn put_key_code(&mut self, event:Event) -> Result<(), &'static str> {
        for item in self.allocated.iter_mut(){
            let reference = item.upgrade();
            if reference.is_some() {
                let mut window = reference.unwrap();
                //unsafe{ window.force_unlock(); }
                let mut window = window.lock();

                if (*window).active {
                    window.key_producer.enqueue(event);
                    break;
                }
            }
        }

        Ok(())
    }

}

/// a window object
pub struct WindowObj {
    inner:Arc<Mutex<WindowInner>>,
    //consumer: DFQueueConsumer<Event>,
    text_buffer:FrameTextBuffer,

    consumer:DFQueueConsumer<Event>,
}


impl WindowObj{

    /// adjust the size of a window
    pub fn resize(&mut self, x:usize, y:usize, width:usize, height:usize){
        let mut inner = self.inner.lock();  
        inner.resize(x, y, width, height);
    }

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

}

/// Implements TextDisplay trait for vga buffer.
/// set_cursor() should accept coordinates within those specified by get_dimensions() and display to window
impl TextDisplay for WindowObj {

    fn disable_cursor(&mut self) {
        self.text_buffer.cursor.disable();
    }

    fn set_cursor(&mut self, line:u16, column:u16, reset:bool) {
        let cursor = &mut (self.text_buffer.cursor);
        cursor.enable();
        cursor.update(line as usize, column as usize, reset);
        let inner = self.inner.lock();
        frame_buffer::fill_rectangle(inner.x + inner.margin + (column as usize) * CHARACTER_WIDTH, 
                        inner.y + inner.margin + (line as usize) * CHARACTER_HEIGHT, 
                        CHARACTER_WIDTH, CHARACTER_HEIGHT, FONT_COLOR);
    }

    fn cursor_blink(&mut self) {
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
    fn get_dimensions(&self) -> (usize, usize) {
        let inner = self.inner.lock();
        ((inner.width-2*inner.margin)/CHARACTER_WIDTH, (inner.height-2*inner.margin)/CHARACTER_HEIGHT)
    }

    /// Requires that a str slice that will exactly fit the frame buffer
    /// The calculation is done inside the console crate by the print_by_bytes function and associated methods
    /// Print every byte and fill the blank with background color
    fn display_string(&mut self, slice: &str) -> Result<(), &'static str> {
        let inner = self.inner.lock();
        self.text_buffer.print_by_bytes(inner.x + inner.margin, inner.y + inner.margin, 
            inner.width - 2 * inner.margin, inner.height - 2 * inner.margin, 
            slice)
    }
    
    fn draw_border(&self) -> (usize, usize, usize){
        let inner = self.inner.lock();
        inner.draw_border(get_border_color(inner.active))
    }


    fn get_key_event(&self) -> Option<Event> {
        let event_opt = self.consumer.peek();
        if event_opt.is_some() {
            let event = event_opt.unwrap();
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
    fn is_overlapped(&self, inner:&WindowInner) -> bool {
        if self.check_in_area(inner.x, inner.y)
            {return true;}
        if self.check_in_area(inner.x, inner.y+inner.height)
            {return true;}
        if self.check_in_area(inner.x+inner.width, inner.y)
            {return true;}
        if self.check_in_area(inner.x+inner.width, inner.y+inner.height)
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
        /*if active && self.consumer.is_none() {
            let consumer = KEY_CODE_CONSUMER.try();
            self.consumer = consumer;
        }
        if !active && self.consumer.is_some(){       
            self.consumer = None;
        }*/
    }

    fn resize(&mut self, x:usize, y:usize, width:usize, height:usize) {
        self.draw_border(BACKGROUND_COLOR);
        self.clean();
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height;
        self.draw_border(get_border_color(self.active));
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
}

/// put key input events in the producer of window manager
pub fn put_key_code(event: Event) -> Result<(), &'static str>{    
    let allocator: &Mutex<WindowAllocator> = WINDOW_ALLOCATOR.call_once(|| {
        Mutex::new(WindowAllocator{allocated:VecDeque::new()})
    });

    allocator.lock().put_key_code(event)
}

fn get_border_color(active:bool) -> u32 {
    if active {
        WINDOW_ACTIVE_COLOR
    } else {
        WINDOW_INACTIVE_COLOR
    }
}

pub fn init() -> Result<DFQueueProducer<Event>, &'static str> {
    let keyboard_event_handling_queue: DFQueue<Event> = DFQueue::new();
    let keyboard_event_handling_consumer = keyboard_event_handling_queue.into_consumer();
    let returned_keyboard_producer = keyboard_event_handling_consumer.obtain_producer();
    // Initializes the default kernel terminal
    let mut window_object = match get_window_obj(20, 20, 600, 150) {
        Ok(obj) => obj,
        Err(_) => return Err("Window object couldn't be initalized")
    };

    terminal::Terminal::init(window_object, 0)?;
    // Initalizes a second terminal; will fix in next version
    let window_object = match get_window_obj(20, 200, 600, 150) {
        Ok(obj) => obj,
        Err(_) => return Err("Window object couldn't be initalized")
    };
    terminal::Terminal::init(window_object, 1)?;

    // populates a struct with the args needed for input_event_loop

    // Adds this default kernel terminal to the static list of running terminals
    // Note that the list owns all the terminals that are spawned
    spawn::spawn_kthread(input_event_loop, keyboard_event_handling_consumer, "main input event handling loop".to_string(), None)?;
    Ok(returned_keyboard_producer)
}


fn input_event_loop(mut consumer:DFQueueConsumer<Event>) -> Result<(), &'static str> {
    let mut terminal_id_counter: usize = 1;
    // Bool prevents keypresses like ctrl+t from actually being pushed to the terminal scrollback buffer
    let mut meta_keypress = false;
    loop {
        meta_keypress = false;
        use core::ops::Deref;

        // Pops events off the keyboard queue and redirects to the appropriate terminal input queue producer
        let event = match consumer.peek() {
            Some(ev) => ev,
            _ => { continue; }
        };
        match event.deref() {
            &Event::ExitEvent => {
                return Ok(()); 
            }

            &Event::InputEvent(ref input_event) => {
                let key_input = input_event.key_event;
                // Creates new terminal window
                // if key_input.modifiers.control && key_input.keycode == Keycode::T {
                //     // hardcoding for now, will fix once I figure out a good system for auto-resizing of windows
                //     if terminal_id_counter < 2 {                   
                //         let window_object = match get_window_obj(20, 200, 600, 150) {
                //             Ok(obj) => obj,
                //             Err(_) => {
                //                 debug!("error initializing window");
                //                 return Err("Window object couldn't be initalized")}
                //         };
                //         debug!("in here");
                //         terminal::Terminal::init(window_object, 1)?;
                //         terminal_id_counter += 1;
                //         meta_keypress = true;
                //         event.mark_completed();
                //     }
                // }
                if key_input.modifiers.alt && key_input.keycode == Keycode::Tab {
                    window_switch();
                    meta_keypress = true;
                    event.mark_completed();

                }
            }
            _ => { }
        }

        // If the keyevent was not for control of the terminal windows
        if !meta_keypress {
                put_key_code(event.deref().clone())?;
                event.mark_completed();

        }
    }    
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
    i    let allocator = try_opt!(WINDOW_ALLOCATOR.try());[T] at top of panic_wrapper: index out of bounds: the len is 400 but the index is 400 frame_buffer/src/text_buffer.rs:127:13
[E] PANIC was unhandled in task "terminal loop" on core Some(0) at frame_buffer/src/text_buffer.rs:127:13 -- index out of bounds: the len is 400 but the index is 400
[E] STACK TRACE: FFFFFE0000032890
[E]  FFFFFE0000032890: INVALID_ADDRESS
[E] Killing panicked task "terminal loop"

//    unsafe { allocator.force_unlock(); }
    allocator.lock().deref_mut().switch();
         let allocator = try_opt!(WINDOW_ALLOCATOR.try());
//    unsafe { allocator.force_unlock(); }
    allocator.lock().deref_mut().switch();
         let allocator = try_opt!(WINDOW_ALLOCATOR.try());
//    unsafe { allocator.force_unlock(); }
    allocator.lock().deref_mut().switch();
    }    let allocator = try_opt!(WINDOW_ALLOCATOR.try());
//    unsafe { allocator.force_unlock(); }
    allocator.lock().deref_mut().switch();

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
