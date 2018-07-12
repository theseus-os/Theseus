//! This crate is an application to draw graphs on the screen

#![no_std]
#![feature(alloc)]

extern crate alloc;
extern crate spin;
extern crate irq_safety;
extern crate dfqueue;
extern crate keycodes_ascii;
#[macro_use] extern crate frame_buffer;
#[macro_use] extern crate window_manager;
#[macro_use] extern crate log;
#[macro_use] extern crate print;


use alloc::{Vec, String, LinkedList};
use spin::{Once, Mutex};
use irq_safety::MutexIrqSafe;
use core::ops::{DerefMut, Deref};
use dfqueue::{DFQueue,DFQueueConsumer,DFQueueProducer};
use keycodes_ascii::Keycode;
use window_manager::{get_window_obj, delete_window};
//use alloc::arc::{Arc, Weak};

//pub mod test_drawer;


static GraphDrawer: Once<MutexIrqSafe<GraphDrawer>> = Once::new();

static KEY_CODE_CONSUMER: Once<DFQueueConsumer<Keycode>> = Once::new();
static KEY_CODE_PRODUCER: Once<DFQueueProducer<Keycode>> = Once::new();



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
        match keycode {
            &Keycode::Left => {
                if self.x == 0 {
                    return
                }
                self.x = self.x - 1;
            },
            &Keycode::Right => {
                if self.x == frame_buffer::FRAME_BUFFER_WIDTH - 1 {
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
                if self.y == frame_buffer::FRAME_BUFFER_HEIGHT - 1 {
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
        match keycode {
            &Keycode::Left => {
                if self.start_x == 0 || self.end_x == 0 {
                    return
                }
                self.start_x = self.start_x - 1;
                self.end_x = self.end_x - 1;
            },
            &Keycode::Right => {
                if self.start_x == frame_buffer::FRAME_BUFFER_WIDTH - 1 
                    || self.end_x == frame_buffer::FRAME_BUFFER_WIDTH - 1 {
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
                if self.start_y == frame_buffer::FRAME_BUFFER_HEIGHT - 1 
                    || self.end_y == frame_buffer::FRAME_BUFFER_HEIGHT - 1 {
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
        match keycode {
            &Keycode::Left => {
                if self.x == 0 {
                    return
                }
                self.x = self.x - 1;
            },
            &Keycode::Right => {
                if self.x + self.width == frame_buffer::FRAME_BUFFER_WIDTH {
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
                if self.y + self.height == frame_buffer::FRAME_BUFFER_HEIGHT {
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
            let graph = try_opt_err!(list.deref().get(self.active as usize), "Couldn't switch to current window");
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

    let consumer = try_opt_err!(KEY_CODE_CONSUMER.try(),"No active consumer");

    let producer = KEY_CODE_PRODUCER.call_once(|| {
        consumer.obtain_producer()
    });

    let producer = KEY_CODE_PRODUCER.try();
    
    try_opt_err!(producer, "Couldn't init key code producer").enqueue(keycode);
    
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

    trace!("Wenqiu : drawer 1");
    let mut times = -1;

    if _args.len() > 0 {
        times = _args[0].parse().unwrap();
    }

   
    {
        let rs= get_window_obj(100, 10, 300, 380);
        trace!("Wenqiu : drawer 2");
        if rs.is_err() {
            return -1;
        }

        //let window_mutex_opt= rs.ok().expect("Fail to get window object");
        //let window_arc = window_mutex_opt.upgrade().unwrap();
        let window_arc = rs.ok().expect("Fail to get window object");
        let window = window_arc.lock();


        let window = &(*window);
        window.draw_line(30, 60, 50, 90, 0x000000);
    }
    //let consumer = KEY_CODE_CONSUMER.call_once(||DFQueue::new().into_consumer());
    
    window_manager::check_reference();

    let mut a = 0;
    loop {
        if times > 0 {
            a += 1;
        }
 
        /*let point = Graph::Point(Point{x:300, y:200});
        draw_graph(point, 10, 0x00FFFF);

        let point = Graph::Point(Point{x:300, y:200});
        draw_graph(point, 20, 0xFFFFFF);

        let line = Graph::Line(Line{start_x:20, start_y:30, end_x:200, end_y:300});
        draw_graph(line, 17, 0xFF0000);


        let rectangle = Graph::Rectangle(Rectangle{x:30, y:50, width:40, height:30, fill:false});
        draw_graph(rectangle, 19, 0x3768FF);

        let line = Graph::Line(Line{start_x:20, start_y:50, end_x:500, end_y:50});
        draw_graph(line, 30, 0x00FF00);

        let line = Graph::Line(Line{start_x:20, start_y:300, end_x:300, end_y:50});
        draw_graph(line, 19, 0x0000FF);


        let rectangle = Graph::Rectangle(Rectangle{x:40, y:60, width:100, height:30, fill:true});
        draw_graph(rectangle, 20, 0xFF3769);
        */
        if a == times {
            break;
        }
    }


    println!("Hello World");

    0
//    super::init();

}
    