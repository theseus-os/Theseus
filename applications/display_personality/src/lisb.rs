//! This crate is an application to draw graphs on the screen

#![no_std]
#![feature(alloc)]

#[macro_use] extern crate alloc;
extern crate spin;
extern crate irq_safety;
extern crate dfqueue;
extern crate keycodes_ascii;
extern crate memory;
extern crate mod_mgmt;
#[macro_use] extern crate frame_buffer;
#[macro_use] extern crate window_manager;
#[macro_use] extern crate log;
#[macro_use] extern crate print;
extern crate acpi;
extern crate path;


use alloc::vec::Vec;
use alloc::string::String;
//{Vec, String, LinkedList};
use spin::{Once, Mutex};
use irq_safety::MutexIrqSafe;
use core::ops::{DerefMut, Deref};
use dfqueue::{DFQueue,DFQueueConsumer,DFQueueProducer};
use keycodes_ascii::Keycode;
use mod_mgmt::{CrateNamespace, get_default_namespace};
use memory::{get_kernel_mmi_ref, MappedPages};
use alloc::sync::{Arc, Weak};
use acpi::get_hpet;
use alloc::prelude::ToString;
use mod_mgmt::SwapRequest;
use path::Path;


static GraphDrawer: Once<MutexIrqSafe<GraphDrawer>> = Once::new();

static KEY_CODE_CONSUMER: Once<DFQueueConsumer<Keycode>> = Once::new();
static KEY_CODE_PRODUCER: Once<DFQueueProducer<Keycode>> = Once::new();

type ResolutionFun = fn() -> (usize, usize);
type DrawPixelFun = fn(usize, usize, u32);
type FillRectangleFun = fn(usize, usize, usize, usize, u32);
type InitFun = fn() -> Result<(), &'static str >;


struct GraphDrawer {
    graphlist: Mutex<Vec<GraphObj>>,
    active:i32,
}

/// A point for display on the screen
pub struct Point{
    /// x-coordinate of the point
    x:usize,
    /// y-coordinate of the point
    y:usize,
}

impl Point {
    fn mov(&mut self, keycode:&Keycode){
        let (width, height) = frame_buffer::get_resolution();
        match keycode {
            &Keycode::Left => {
                if self.x == 0 {
                    return
                }
                self.x = self.x - 1;
            },
            &Keycode::Right => {
                if self.x == width - 1 {
                    return
                }
                self.x = self.x + 1;
            },
            &Keycode::Up => {
                if self.y == 0 {
                    return
                }
                self.y = self.y - 1;
            },
            &Keycode::Down => {
                if self.y == height - 1 {
                    return
                }
                self.y = self.y + 1;
            },

            _ => {

            },
        }
        
    }
}

/// A line for display on the screen
pub struct Line{
    /// x-coordinate of the start point
    start_x:usize,
    /// y-coordinate of the start point
    start_y:usize,
    /// x-coordinate of the end point
    end_x:usize,
    /// y-coordinate of the end point
    end_y:usize,    
}

impl Line {
     fn mov(&mut self, keycode:&Keycode){
            let (width, height) = frame_buffer::get_resolution();
            match keycode {
            &Keycode::Left => {
                if self.start_x == 0 || self.end_x == 0 {
                    return
                }
                self.start_x = self.start_x - 1;
                self.end_x = self.end_x - 1;
            },
            &Keycode::Right => {
                if self.start_x == width - 1 
                    || self.end_x == width - 1 {
                    return
                }
                self.start_x = self.start_x + 1;
                self.end_x = self.end_x + 1;
            },
            &Keycode::Up => {
                if self.start_y == 0 || self.end_y == 0 {
                    return
                }
                self.start_y = self.start_y - 1;
                self.end_y = self.end_y - 1;
            },
            &Keycode::Down => {
                if self.start_y == height - 1 
                    || self.end_y == height - 1 {
                    return
                }
                self.start_y = self.start_y + 1;
                self.end_y = self.end_y + 1;
            },

            _ => {

            },
        }
        
    }
}

///A rectangle for display on the screen
pub struct Rectangle {
    ///x-coordinate of the upper left point
    x:usize,
    ///y-coordinate of the upper left point
    y:usize,
    ///the height of the rectangle
    height:usize,
    ///the width of the rectangle
    width:usize,
    ///whether the rectangle is filled with its color
    fill:bool,
}

impl Rectangle {
    fn mov(&mut self, keycode:&Keycode){
        let (width, height) = frame_buffer::get_resolution();
        match keycode {
            &Keycode::Left => {
                if self.x == 0 {
                    return
                }
                self.x = self.x - 1;
            },
            &Keycode::Right => {
                if self.x + self.width == width {
                    return
                }
                self.x = self.x + 1;
            },
            &Keycode::Up => {
                if self.y == 0 {
                    return
                }
                self.y = self.y - 1;
            },
            &Keycode::Down => {
                if self.y + self.height == height {
                    return
                }
                self.y = self.y + 1;
            },

            _ => {

            },
        }
        
    }
}

/// A graph enum including point, line and rectangle
pub enum Graph {
    Point(Point),
    Line(Line),
    Rectangle(Rectangle),
}

trait Move {
    fn mov(&mut self, keycode:&Keycode);
}

impl Move for Graph {
    fn mov(&mut self, keycode:&Keycode) {
        match self {
            &mut Graph::Point(ref mut point) => { point.mov(keycode); },
            &mut Graph::Line(ref mut line) => { line.mov(keycode); },
            &mut Graph::Rectangle(ref mut rectangle) => { rectangle.mov(keycode); },
            _ => {},
        }
    }
}

/// An graph object for display on the screen
pub struct GraphObj{
    /// the primitive graph
    graph: Graph,
    /// the z-coordinate of the graph
    depth: usize,
    /// the color of the graph
    color: u32,
}

impl GraphObj {
    /// draw a graph object
    pub fn display(&self, show:bool) {
        match self.graph {
            Graph::Point(ref point) => {
                frame_buffer::draw_pixel(point.x, point.y, self.color);
            },

            Graph::Line(ref line) => {
                frame_buffer::draw_line(line.start_x, line.start_y, line.end_x, line.end_y, self.color);
            },

            Graph::Rectangle(ref rectangle) => {
                if rectangle.fill {
                    frame_buffer::fill_rectangle(rectangle.x, rectangle.y, rectangle.width, rectangle.height, self.color);
                } else {
                    frame_buffer::draw_rectangle(rectangle.x, rectangle.y, rectangle.width, rectangle.height, self.color);
                }
            },

            _ => {

            },  
        }
        
    }

    fn draw(&self){
        self.display(true);
    }

    fn clear(&self){
        //self.color = BACKGROUD_COLOR;
        self.display(false);
    }

    fn mov(&mut self, keycode:&Keycode){

        /*match self.graph {
            Graph::Rectangle(ref mut square) => {
                match keycode {
                    &Keycode::Left|&Keycode::Right => {
                        frame_buffer::draw_line(square.x + square.width - 1, square.y, 
                            square.x + square.width - 1, square.y+square.height, self.depth, self.color, false);
                        frame_buffer::draw_line(square.x, square.y, 
                            square.x, square.y + square.height, self.depth, self.color, false);
                    },

                    &Keycode::Up|&Keycode::Down => {
                        frame_buffer::draw_line(square.x, square.y + square.height - 1, 
                            square.x + square.width, square.y + square.height - 1, self.depth, self.color, false);
                        frame_buffer::draw_line(square.x, square.y, 
                            square.x + square.width, square.y, self.depth, self.color, false);
                    },

                    _ => {

                    },
                }
            },

            _ => {
                self.clear();
            },
        }

        self.graph.mov(keycode);
*/
    }
}

/// draw a primitive graph with its depth and color
pub fn draw_graph (graph: Graph, depth:usize, color:u32) {
    let drawer: &MutexIrqSafe<GraphDrawer> = GraphDrawer.call_once(|| {
        MutexIrqSafe::new(GraphDrawer{graphlist:Mutex::new(Vec::new()), active:-1})
    });

    let graph_obj = GraphObj{graph:graph, depth:depth, color:color};
    drawer.lock().deref_mut().add(graph_obj)
}

impl GraphDrawer {
    fn add (&mut self, graph_obj: GraphObj) {
        let mut list = self.graphlist.lock();
        let mut list = list.deref_mut();
        let mut index = list.len();
        graph_obj.draw();
        for i in 0..list.len() {
            let item = list.get(i).unwrap();
            if item.depth >= graph_obj.depth{
                index = i;
                break;
            }
        }
        list.insert(index, graph_obj);
        self.active = index as i32;
    }

    fn switch (&mut self) -> Result<(), &'static str>{
        if self.active != -1 {
            let length = self.graphlist.lock().len();
            self.active = (((self.active+1) as usize) %length) as i32;

            let list = self.graphlist.lock();
            let graph = match list.deref().get(self.active as usize) {
                Some(graph) => {
                    graph
                },
                None => {
                    return Err("Couldn't switch to current window");
                }
            };
            graph.clear();
            graph.draw();
        }

        Ok(())
    }

    fn delete_active (&mut self){
        if self.active < 0 {
            return;
        }
        let depth;
        {
            let mut list = self.graphlist.lock();
            let mut graph = list.deref_mut().remove(self.active as usize);
            depth = graph.depth;
            graph.clear();
            if list.len() == 0{
                self.active = -1;
                return;
            }
            if self.active == list.len() as i32{
                self.active = 0;
            }
        }
        self.redraw(depth);
    }

    fn move_active (&mut self, keycode:&Keycode){
        if self.active < 0 {
            return;
        }
        let depth;
        {
            let mut list = self.graphlist.lock();
            let mut list = list.deref_mut();
            let mut graph = list.remove(self.active as usize);
            depth = graph.depth;
            graph.mov(keycode);
            list.insert(self.active as usize, graph);
        }
        self.redraw(depth);
    }

    fn redraw(&mut self, depth:usize) {
        /*if self.active >= 0 {
            let mut list = self.graphlist.lock();
            for i in (0..list.len()).rev(){
                let graph = try_opt_err!(list.deref_mut().get(i), "GraphDrawer.redraw() couldn't get a graph");
                if graph.depth < depth {
                    break;
                }
                graph.draw();
            }
        }*/
    }
}

/// Add key event to the drawer application
pub fn put_key_code(keycode:Keycode) -> Result<(), &'static str>{
    let consumer = match KEY_CODE_CONSUMER.try() {
        Some(consumer) => {
            consumer
        },
        None => {
            return Err("No active consumer")
        }
    };

    let producer = KEY_CODE_PRODUCER.call_once(|| {
        consumer.obtain_producer()
    });

    match KEY_CODE_PRODUCER.try() {
        Some(producer) => {
            producer.enqueue(keycode);
        },
        None => {
            return Err("Couldn't init key code producer");
        }
    }

    Ok(())
}

/// init the drawer to deal with key inputs
pub fn init() -> Result<(), &'static str>{
    loop{
    /*    let consumer = KEY_CODE_CONSUMER.call_once(||DFQueue::new().into_consumer());
        let event = consumer.peek();
        if event.is_none() {
            continue; 
        }   
        let event = try_opt_err!(event, "graph_drawer::init() fail to read a key input event");
        event.mark_completed();
        let keycode = event.deref(); // event.deref() is the equivalent of   &*event
        
        let drawer = try_opt_err!(GraphDrawer.try(), "graph_drawer::init() Couldn't get GraphDrawer");
        let drawer = drawer.lock().deref_mut();

        match keycode {
            &Keycode::Delete => {
               drawer.delete_active();
            },

            &Keycode::Tab => {
                drawer.switch();
            },

            &Keycode::Left|&Keycode::Right|&Keycode::Up|&Keycode::Down => {
                drawer.move_active(keycode);
            },

            _ => {

            },
        }*/
    }   
}

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    let mut size = 100;

    if _args.len() > 0 {
        size = _args[0].parse().unwrap();
    }

    let (width, height) = frame_buffer::get_resolution();
    let hpet_lock = get_hpet();
    let hpet = match hpet_lock.as_ref(){
        Some(hpet) => {hpet}
        None => {
            println!("Fail to get hpet");
            return -1;
        }
    };

	let kernel_mmi_ref = match get_kernel_mmi_ref() {
        Some(mmi) => {mmi},
        None => { 
            println!("couldn't get kernel mmi");
            return -1
        }
    };

	let normal_namespace = match get_default_namespace(){
        Some(space) => {space},
        None => {
            println!("couldn't get default namespace");
            return -1
        }
    };


    let frame_buffer_3d_dir = "3d";
    let path = Path::new(String::from(frame_buffer_3d_dir));
	let mut frame_buffer_namespace = match CrateNamespace::with_base_dir_path(String::from(frame_buffer_3d_dir), path){
        Ok(space) => {space},
        Err(err) => {
            println!("couldn't create new namespace: {}", err);
            return -1
        }
    };
;
	let frame_buffer_test = frame_buffer_namespace.get_kernel_file_starting_with("frame_buffer-").unwrap();
    
    frame_buffer_namespace.enable_fuzzy_symbol_matching();
	match frame_buffer_namespace.load_kernel_crate(&frame_buffer_test, Some(normal_namespace), kernel_mmi_ref.lock().deref_mut(), false){
        Ok(_) => {

        },
        Err(err) => {
            println!("Fail to load modules to new namespace: {}", err);
            return -1;
        }
    }

	frame_buffer_namespace.disable_fuzzy_symbol_matching();

    

    let mut space = 0;
    let (init_pages, offset) = match get_function_pages(&frame_buffer_namespace, "frame_buffer::init::"){
        Ok(pages) => {pages},
        Err(err) => {
            println!("couldn't find func init()");
            return -1;
        }
    };
	let init: &InitFun = match init_pages.lock().as_func(offset, &mut space){
        Ok(func) => {func},
        Err(err) => {            
            println!("Fail to transmute func init()");
            return -1;
        }
    };

    match init(){
        Ok(()) =>{},
        Err(_) => {println!("Fail to init frame buffer");}
    };

    let mut space = 0;
    let (get_resolution_pages, offset) = match get_function_pages(&frame_buffer_namespace, "frame_buffer::get_resolution::"){
        Ok(pages) => {pages},
        Err(err) => {
            println!("couldn't find func get_resolution()");
            return -1;
        }
    };
	let get_resolution: &ResolutionFun = match get_resolution_pages.lock().as_func(offset, &mut space){
        Ok(func) => {func},
        Err(err) => {            
            println!("Fail to transmute func get_resolution()");
            return -1;
        }
    };

    let mut space = 0;
    let (draw_pixel_pages, offset) = match get_function_pages(&frame_buffer_namespace, "frame_buffer::draw_pixel::"){
        Ok(pages) => {pages},
        Err(err) => {
            println!("couldn't find func get_resolution()");
            return -1;
        }
    };
	let draw_pixel: &DrawPixelFun = match draw_pixel_pages.lock().as_func(offset, &mut space){
        Ok(func) => {func},
        Err(err) => {            
            println!("Fail to transmute func func draw_pixel()");
            return -1;
        }
    };

    let mut space = 0;
    let (fill_rectangle_pages, offset) = match get_function_pages(&frame_buffer_namespace, "frame_buffer::fill_rectangle::"){
        Ok(pages) => {pages},
        Err(err) => {
            println!("couldn't find func fill_rectangle()");
            return -1;
        }
    };
	let fill_rectangle: &FillRectangleFun = match draw_pixel_pages.lock().as_func(offset, &mut space){
        Ok(func) => {func},
        Err(err) => {            
            println!("Fail to transmute func fill_rectangle()");
            return -1;
        }
    };

    let mut color = 0x0000FF;
    let start_time = hpet.get_counter();  

    loop {
        color = color + 0x100 - 0x1;
        if color > 0xff00 {
            break;
        }
        let x = width/2 + 10;
        let y = height/2 + 10;
    	fill_rectangle(x, 10, size, size, color);
        frame_buffer::fill_rectangle(x, y, size, size, color);
        //frame_buffer::fill_rectangle(width/2+GAP, height/2+GAP, width/2 - 2*GAP, height/2-2*GAP, 0x00ff00);
    }

    let end_time = hpet.get_counter();  

    println!("Size: {}\n Time: {}", size, end_time - start_time);
    0
//    super::init();

}
    

fn get_function_pages(namespace:&CrateNamespace, fname_pre:&'static str) -> Result<(Arc<Mutex<MappedPages>>, usize), &'static str> {
    let section_ref = 
        match namespace.get_symbol_starting_with(fname_pre).upgrade() {
            Some(symbol) => {symbol},
            None => { 
                println!("no single symbol matching \"{}\"", fname_pre);
                return Err("Fail to get the symbol");
            }
        };

	let section = section_ref.lock();
	Ok((section.mapped_pages.clone(), section.mapped_pages_offset)) 
}

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    let mut size = 100;

    if _args.len() > 0 {
        size = _args[0].parse().unwrap();
    }

    let (width, height) = frame_buffer::get_resolution();
    let hpet_lock = get_hpet();
    let hpet = match hpet_lock.as_ref(){
        Some(hpet) => {hpet}
        None => {
            println!("Fail to get hpet");
            return -1;
        }
    };
    let start_time = hpet.get_counter();  

    let mut color = 0x0000FF;
    loop {
        color = color + 0x100 - 0x1;
        if color > 0xff00 {
            break;
        }
        let x = width/2 + 10;
        let y = height/2 + 10;
    	frame_buffer::fill_rectangle(x, 10, size, size, color);
        frame_buffer::fill_rectangle(x, y, size, size, color);
        //frame_buffer::fill_rectangle(width/2+GAP, height/2+GAP, width/2 - 2*GAP, height/2-2*GAP, 0x00ff00);
    }

    let end_time = hpet.get_counter();  

    println!("Size: {}\n Time: {}", size, end_time - start_time);
    0
//    super::init();

}

// #[no_mangle]
// pub fn main_swap(_args: Vec<String>) -> isize {
//     let mut size = 100;

//     if _args.len() > 0 {
//         size = _args[0].parse().unwrap();
//     }


//     let mut v: Vec<(&str, &str, bool)> = Vec::new();

//     let (width, height) = frame_buffer::get_resolution();
//     let hpet_lock = get_hpet();
//     let hpet = match hpet_lock.as_ref(){
//         Some(hpet) => {hpet}
//         None => {
//             println!("Fail to get hpet");
//             return -1;
//         }
//     };
//     let start_time = hpet.get_counter();  

//     let mut color = 0x0000FF;
//     loop {
//         color = color + 0x100 - 0x1;
//         if color > 0xff00 {
//             break;
//         }
//         let x = width/2 + 10;
//         let y = height/2 + 10;
//     	frame_buffer::fill_rectangle(x, 10, size, size, color);



        
//         frame_buffer::fill_rectangle(x, y, size, size, color);
//         //frame_buffer::fill_rectangle(width/2+GAP, height/2+GAP, width/2 - 2*GAP, height/2-2*GAP, 0x00ff00);
//     }

//     let end_time = hpet.get_counter();  

//     println!("Size: {}\n Time: {}", size, end_time - start_time);
//     0
// //    super::init();

// }


// /// Performs the actual swapping of modules.
// fn swap_modules(tuples: Vec<(&str, &str, bool)>, verbose_log: bool) -> Result<(), String> {
//     let namespace = mod_mgmt::get_default_namespace();

//     let swap_requests = {
//         let mut mods: Vec<SwapRequest> = Vec::with_capacity(tuples.len());
//         for (o, n, r) in tuples {
//             // 1) check that the old crate exists
//             let old_crate = namespace.get_crate_starting_with(o)
//                 .ok_or_else(|| format!("Couldn't find old crate that matched {:?}", o))?;

//             // 2) check that the new module file exists
//             let new_crate_module = get_module(n)
//                 .ok_or_else(|| format!("Couldn't find new module file {:?}.", n))
//                 .or_else(|_| get_module_starting_with(n)
//                     .ok_or_else(|| format!("Couldn't find single fuzzy match for new module file {:?}.", n))
//                 )?;
//             mods.push(SwapRequest::new(old_crate.lock_as_ref().crate_name.clone(), new_crate_module, r));
//         }
//         mods
//     };

//     let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or_else(|| "couldn't get kernel_mmi_ref".to_string())?;
//     let mut kernel_mmi = kernel_mmi_ref.lock();
    
//     let start = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();

//     let swap_result = namespace.swap_crates(
//         swap_requests, 
//         kernel_mmi.deref_mut(), 
//         verbose_log
//     );
    
//     let end = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
//     let hpet_period = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.counter_period_femtoseconds();

//     let elapsed_ticks = end - start;
//     println!("Swap operation complete. Elapsed HPET ticks: {}, (HPET Period: {} femtoseconds)", 
//         elapsed_ticks, hpet_period);


//     swap_result.map_err(|e| e.to_string())
// }