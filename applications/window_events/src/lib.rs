#![no_std]
#[macro_use] extern crate alloc;
extern crate spin;
extern crate mpmc;
extern crate event_types;
extern crate keycodes_ascii;
extern crate mouse_data;
extern crate framebuffer;
extern crate shapes;

use mpmc::Queue;
use alloc::collections::VecDeque;
use alloc::collections::BTreeSet;
use alloc::string::ToString;
use alloc::sync::{Arc, Weak};
//use alloc::result::Result;
use spin::{Mutex, Once, Lazy};
use keycodes_ascii::{KeyEvent};
use mouse_data::MouseEvent;
use framebuffer::{Framebuffer, AlphaPixel};
use shapes::{Coord, Rectangle};
use alloc::boxed::Box;

#[derive(Debug, Clone)]
pub enum WmToWindowEvent {
    TellSize((usize, usize)),
    KeyboardEvent(KeyEvent),
    MouseEvent(MouseEvent),
}

#[derive(Clone)]
pub enum WindowToWmEvent {
    Render(Arc<Mutex<Framebuffer<AlphaPixel>>>, Option<Rectangle>), //&'static Framebuffer<AlphaPixel>
    AskSize
}

use core::fmt::Debug;
use core::fmt::Formatter;
use core::fmt::Error;
impl Debug for WindowToWmEvent{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        f.write_fmt(format_args!("WindowToWmEvent: {:#?}", self))
    }
}
pub static WILI: Lazy<Mutex<Queue<(Queue<WindowToWmEvent>, Queue<WmToWindowEvent>)>>> = Lazy::new(|| Mutex::new(Queue::with_capacity(100)));

pub fn register_window(wtowm: Queue<WindowToWmEvent>, wmtow: Queue<WmToWindowEvent>) -> Result<(), &'static str> {
    WILI.lock().push((wtowm, wmtow)).map_err(|_| "Can't push to WILI") //.get().ok_or("no WILI ?")?
}
