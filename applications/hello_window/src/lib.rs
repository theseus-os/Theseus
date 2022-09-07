#![no_std]
#![feature(core_intrinsics)]
// #![feature(plugin)]
// #![plugin(application_main_fn)]

extern crate shapes;
extern crate alloc;
extern crate window;
extern crate color;
extern crate sleep;
extern crate event_types;

// #[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;

use core::sync::atomic::AtomicUsize;
use core::time::Duration;
use alloc::vec::Vec;
use alloc::string::String;
use window::Window;
use shapes::{Coord, Rectangle};
use color::{Color, BLUE, RED};
use event_types::Event;

pub fn main(_args: Vec<String>) -> isize {
    // info!("Hello, world! (from hello application)");
    let mut w = Window::new(Coord{x:0,y:0}, 100, 100, BLUE).unwrap();
    println!("Hello, world! Args: {:?}", "don't care");

    //let start_time : AtomicUsize = AtomicUsize::new(sleep::get_current_time_in_ticks());
    while let Ok(e) = w.handle_event(){
        _ = w.render(None);
        //sleep::sleep_periodic(&start_time, 100);
        /*match e {
            Some(Event::MousePositionEvent(me)) => println!("{:?}", me),
            a => println!("{:?}", a),
        }*/
        //sleep::sleep(10);{
        //if e.is_some(){
        let t =sleep::get_current_time_in_ticks() as f64;
        w.framebuffer_mut().lock().fill(Color::new(u32::from_ne_bytes([
            128,
            (((t/10.)+170.)%255.) as u8,
            (((t/10.)+85.)%255.) as u8,
            (((t/10.)+0.)%255.) as u8,
        ])).into());
        /*let mut inner = w.inner.lock();
            w.show_button(TopButton::Close, 1, &mut inner);
            w.show_button(TopButton::MinimizeMaximize, 1, &mut inner);
            w.show_button(TopButton::Hide, 1, &mut inner);

        match w.render(None){
            Err(a) => println!("{}", a),
            _ => (),
        }*/
        //_=w.render(Some(Rectangle{Coord{0, w.inner.lock().title_bar_height}, Coord{w.inner.lock().get_size()});
        _=w.render(Some(w.area()));
        //}
    }
    0
}
//core::intrinsics::cosf64 -> kernel/mod_mgmt/src/lib.rs:2039: Symbol "cos" not found. Try loading the specific crate manually first.
