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

const BACKGROUD_COLOR:usize = 0x000000;

static GraphDrawer: Once<MutexIrqSafe<GraphDrawer>> = Once::new();

static KEY_CODE_CONSUMER: Once<DFQueueConsumer<Keycode>> = Once::new();
static KEY_CODE_PRODUCER: Once<DFQueueProducer<Keycode>> = Once::new();



pub struct GraphDrawer {
    graphlist: Mutex<Vec<GraphObj>>,
    active:i32,
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


pub enum Graph {
    Point(Point),
    Line(Line),
    Square(Square),
}

pub struct GraphObj{
    graph: Graph,
    depth: usize,
    color: usize,
}

impl GraphObj {
    fn display(&self, show:bool) {
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
                    frame_buffer::draw_line(square.x + square.width, square.y, square.x + square.width, square.y + square.height, self.depth, self.color,show);
                    frame_buffer::draw_line(square.x + square.width, square.y + square.height, square.x, square.y + square.height, self.depth, self.color,show);
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

    fn clear(&mut self){
        self.color = BACKGROUD_COLOR;
        self.display(false);
    }
}

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
            let item = list.get(i).unwrap();
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

    fn redraw(&mut self, depth:usize) {
        if self.active < 0{
            return;
        }

        let mut list = self.graphlist.lock();
        for i in (0..list.len()).rev(){
            let graph = list.deref_mut().get(i).unwrap();
            if graph.depth < depth {
                break;
            }
            graph.draw();
        }
    }
}

pub fn put_key_code(keycode:Keycode) -> Result<(), &'static str>{

    let consumer = KEY_CODE_CONSUMER.try();
    if consumer.is_none(){
        return Err("No active consumer");
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

//TODO: create a new thread to do this
pub fn init() {
    loop{
        let consumer = KEY_CODE_CONSUMER.call_once(||DFQueue::new().into_consumer());
        let event = consumer.peek();
        if event.is_none() {
            continue; 
        }   
        let event = event.unwrap();
        event.mark_completed();
        let keycode = event.deref(); // event.deref() is the equivalent of   &*event

        match keycode {
            &Keycode::Delete => {
                let drawer: &MutexIrqSafe<GraphDrawer> = GraphDrawer.call_once(|| {
                     MutexIrqSafe::new(GraphDrawer{graphlist:Mutex::new(Vec::new()), active:0})
                });
                drawer.lock().deref_mut().delete_active();

            },

            _ => {

            },
        }
    }   
}
    