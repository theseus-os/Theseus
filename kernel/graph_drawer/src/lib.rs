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


pub struct GraphDrawer {
    graphlist: Mutex<Vec<GraphObj>>, //The last one is active
}

pub struct Point{
    x:usize,
    y:usize,
}

pub struct Line{
    start_x:usize,
    start_y:usize,
    end_x:usize,
    end_y:usize,    
}

pub struct Square{
    x:usize,
    y:usize,
    height:usize,
    width:usize,
    fill:bool,
}

pub struct Circle {
    center_x:usize,
    center_y:usize,
    radius:usize, 
}

pub enum Graph {
    Point(Point),
    Line(Line),
    Square(Square),
    Circle(Circle),
}

pub struct GraphObj{
    graph: Graph,
    depth: usize,
    color: usize,
}

impl GraphObj {
    fn draw(&self) {
        match self.graph {
            Graph::Point(ref point) => {
                frame_buffer::draw_pixel(point.x, point.y, self.depth, self.color);
            },

            _ => {

            },  
        }
    }
}

pub fn draw_graph (graph: Graph, depth:usize, color:usize) -> Result<(), &'static str>{
    let drawer: &MutexIrqSafe<GraphDrawer> = GraphDrawer.call_once(|| {
        MutexIrqSafe::new(GraphDrawer{graphlist:Mutex::new(Vec::new())})
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
            let item = list.get(i).unwrap();
            if item.depth >= graph_obj.depth{
                index = i;
                break;
            }
        }
        list.insert(index, graph_obj);
       
        Ok(())
    }

}