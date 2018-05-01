//! This crate is an application to draw graphs on the screen

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


use spin::{Once, Mutex};
use irq_safety::MutexIrqSafe;
use alloc::{Vec, LinkedList};
use core::ops::{DerefMut, Deref};
use dfqueue::{DFQueue,DFQueueConsumer,DFQueueProducer};
use keycodes_ascii::Keycode;
use alloc::arc::{Arc, Weak};

pub mod test_drawer;

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

///A square for display on the screen
pub struct Square{
    ///x-coordinate of the upper left point
    x:usize,
    ///y-coordinate of the upper left point
    y:usize,
    ///the height of the square
    height:usize,
    ///the width of the square
    width:usize,
    ///whether the square is filled with its color
    fill:bool,
}

impl Square {
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

/// A graph enum including point, line and square
pub enum Graph {
    Point(Point),
    Line(Line),
    Square(Square),
}

trait Move {
    fn mov(&mut self, keycode:&Keycode);
}

impl Move for Graph {
    fn mov(&mut self, keycode:&Keycode) {
        match self {
            &mut Graph::Point(ref mut point) => { point.mov(keycode); },
            &mut Graph::Line(ref mut line) => { line.mov(keycode); },
            &mut Graph::Square(ref mut square) => { square.mov(keycode); },
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
    color: usize,
}

impl GraphObj {
    /// draw a graph object
    pub fn display(&self, show:bool) {
        match self.graph {
            Graph::Point(ref point) => {
                frame_buffer::draw_pixel(point.x, point.y, self.depth, self.color, show);
            },

            Graph::Line(ref line) => {
                frame_buffer::draw_line(line.start_x, line.start_y, line.end_x, line.end_y, self.depth, self.color,show);
            },

            Graph::Square(ref square) => {
                if square.fill {
                    frame_buffer::draw_square(square.x, square.y, square.width, square.height, self.depth, self.color,show);
                } else {
                    frame_buffer::draw_line(square.x, square.y, square.x + square.width, square.y, self.depth, self.color,show);
                    frame_buffer::draw_line(square.x + square.width - 1, square.y, square.x + square.width - 1, square.y + square.height, self.depth, self.color,show);
                    frame_buffer::draw_line(square.x + square.width, square.y + square.height - 1, square.x, square.y + square.height - 1, self.depth, self.color,show);
                    frame_buffer::draw_line(square.x, square.y + square.height, square.x, square.y, self.depth, self.color, show);
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

        match self.graph {
            Graph::Square(ref mut square) => {
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

    }
}

/// draw a primitive graph with its depth and color
pub fn draw_graph (graph: Graph, depth:usize, color:usize) -> Result<(), &'static str>{
    let drawer: &MutexIrqSafe<GraphDrawer> = GraphDrawer.call_once(|| {
        MutexIrqSafe::new(GraphDrawer{graphlist:Mutex::new(Vec::new()), active:-1})
    });

    let graph_obj = GraphObj{graph:graph, depth:depth, color:color};
    drawer.lock().deref_mut().add(graph_obj)
}

impl GraphDrawer {
    fn add (&mut self, graph_obj: GraphObj) -> Result<(), &'static str> {
        let mut list = self.graphlist.lock();
        let mut list = list.deref_mut();
        let mut index = list.len();
        graph_obj.draw();
        for i in 0..list.len() {
            let item = try_opt_err!(list.get(i), "GraphDrawer couldn't get a graph");
            if item.depth >= graph_obj.depth{
                index = i;
                break;
            }
        }
        list.insert(index, graph_obj);
        self.active = index as i32;
       
        Ok(())
    }

    fn switch (&mut self){
        if self.active == -1 {
            return;
        }
        let length = self.graphlist.lock().len();
        self.active = (((self.active+1) as usize) %length) as i32;

        let list = self.graphlist.lock();
        let graph = try_opt_err!(list.deref().get(self.active as usize), "Couldn't switch to current window");
        graph.clear();
        graph.draw();
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
        if self.active < 0{
            return;
        }

        let mut list = self.graphlist.lock();
        for i in (0..list.len()).rev(){
            let graph = try_opt_err!(list.deref_mut().get(i), "GraphDrawer.redraw() couldn't get a graph");
            if graph.depth < depth {
                break;
            }
            graph.draw();
        }
    }
}

/// Add key event to the drawer application
pub fn put_key_code(keycode:Keycode) -> Result<(), &'static str>{

    let consumer = KEY_CODE_CONSUMER.try();

    let producer = KEY_CODE_PRODUCER.call_once(|| {
        try_opt_err!(consumer,"No active consumer").obtain_producer()
    });

    let producer = KEY_CODE_PRODUCER.try();
    
    try_opt_err!(producer, "Couldn't init key code producer").enqueue(keycode);
    
    Ok(())
}

/// init the drawer to deal with key inputs
pub fn init() -> Result<(), &'static str>{
    loop{
        let consumer = KEY_CODE_CONSUMER.call_once(||DFQueue::new().into_consumer());
        let event = consumer.peek();
        if event.is_none() {
            continue; 
        }   
        let event = try_opt_err!(event, "graph_drawer::init() fail to read a key input event");
        event.mark_completed();
        let keycode = event.deref(); // event.deref() is the equivalent of   &*event
        
        let drawer = GraphDrawer.try();
        let drawer = try_opt_err!(drawer, "graph_drawer::init() Couldn't get GraphDrawer").lock().deref_mut()

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
        }
    }   
}
    