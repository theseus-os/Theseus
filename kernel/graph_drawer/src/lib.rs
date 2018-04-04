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


static WINDOW_ALLOCATOR: Once<MutexIrqSafe<GraphDrawer>> = Once::new();

static KEY_CODE_CONSUMER: Once<DFQueueConsumer<Keycode>> = Once::new();
static KEY_CODE_PRODUCER: Once<DFQueueProducer<Keycode>> = Once::new();


pub struct GraphDrawer {
    allocated: LinkedList<Arc<Mutex<Graph>>>, //The last one is active
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