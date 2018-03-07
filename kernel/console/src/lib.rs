#![no_std]
#![feature(alloc)]

extern crate keycodes_ascii;
extern crate vga_buffer;
extern crate alloc;
extern crate spin;
extern crate dfqueue;
#[macro_use] extern crate log;

use keycodes_ascii::{Keycode, KeyAction, KeyEvent};
use alloc::string::String;
use spin::Once;
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
// use core::sync::atomic::Ordering;
// use rtc;



/// calls print!() with an extra "\n" at the end. 
#[macro_export]
macro_rules! println {
    ($fmt:expr) => (print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"), $($arg)*));
}

/// The main printing macro, which simply pushes an output event to the console's event queue. 
/// This ensures that only one thread (the console) ever accesses the UI, which right now is just the VGA buffer.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ({
            use core::fmt::Write;
            use alloc::String;
            let mut s: String = String::new();
            match write!(&mut s, $($arg)*) {
                Ok(_) => { }
                Err(e) => error!("Writing to String in print!() macro failed, error: {}", e),
            }
            match $crate::print_to_console(s) {
                Ok(_) => { }
                Err(e) => error!("print_to_console() in print!() macro failed, error: {}", e),
            }
    });
}


static PRINT_PRODUCER: Once<DFQueueProducer<ConsoleEvent>> = Once::new();


pub fn print_to_console<S>(s: S) -> Result<(), &'static str> where S: Into<String> {
    let output_event = ConsoleEvent::OutputEvent(ConsoleOutputEvent::new(s.into()));
    try!(PRINT_PRODUCER.try().ok_or("Console print producer isn't yet initialized!")).enqueue(output_event);
    Ok(())
}

#[derive(Debug)]
pub enum ConsoleEvent {
    InputEvent(ConsoleInputEvent),
    OutputEvent(ConsoleOutputEvent),
    ExitEvent,
}

impl ConsoleEvent {
    pub fn new_input_event(kev: KeyEvent) -> ConsoleEvent {
        ConsoleEvent::InputEvent(ConsoleInputEvent::new(kev))
    }

    pub fn new_output_event<S>(s: S) -> ConsoleEvent where S: Into<String> {
        ConsoleEvent::OutputEvent(ConsoleOutputEvent::new(s.into()))
    }
}

/// use this to deliver input events (such as keyboard input) to the console.
#[derive(Debug)]
pub struct ConsoleInputEvent {
    key_event: KeyEvent,
}

impl ConsoleInputEvent {
    pub fn new(kev: KeyEvent) -> ConsoleInputEvent {
        ConsoleInputEvent {
            key_event: kev,
        }
    }
}



/// use this to queue up a formatted string that should be printed to the console. 
#[derive(Debug)]
pub struct ConsoleOutputEvent {
    text: String,
}

impl ConsoleOutputEvent {
    pub fn new(s: String) -> ConsoleOutputEvent {
        ConsoleOutputEvent {
            text: s,
        }
    }
}


// /// the console owns and creates the event queue, and returns a producer reference to the queue.
// pub fn init() -> Result<DFQueueProducer<ConsoleEvent>, &'static str> {
//     assert_has_not_been_called!("console_init was called more than once!");

//     let console_dfq: DFQueue<ConsoleEvent> = DFQueue::new();
//     let console_consumer = console_dfq.into_consumer();
//     let returned_producer = console_consumer.obtain_producer();
//     PRINT_PRODUCER.call_once(|| {
//         console_consumer.obtain_producer()
//     });

//     println!("Console says hello!\n");
    
//     try!(spawn_kthread(main_loop, console_consumer, "console_loop"));
//     Ok(returned_producer)
// }


/// the console owns and creates the event queue, and returns the queue consumer
pub fn init() -> Result<DFQueueConsumer<ConsoleEvent>, &'static str> {
    let console_dfq: DFQueue<ConsoleEvent> = DFQueue::new();
    let console_consumer = console_dfq.into_consumer();

    PRINT_PRODUCER.call_once(|| {
        console_consumer.obtain_producer()
    });

    println!("Console says hello!\n");

    Ok(console_consumer)
}


/// the main console event-handling loop, runs on its own thread. 
/// This is the only thread that is allowed to touch the vga buffer!
/// ## Returns
/// true if the thread was smoothly exited intentionally, false if forced to exit due to an error.
pub fn main_loop(consumer: DFQueueConsumer<ConsoleEvent>) -> Result<(), &'static str> { // Option<usize> just a placeholder because kthread functions must have one Argument right now... :(
    use core::ops::Deref;

    loop { 
        let event = consumer.peek();
        if event.is_none() {
            continue; 
        }

        let event = event.unwrap();
        let event_data = event.deref(); // event.deref() is the equivalent of   &*event

        match event_data {
            &ConsoleEvent::ExitEvent => {
                try!(vga_buffer::print_str("\nSmoothly exiting console main loop.\n").map_err(|_| "print_str failed"));
                return Ok(()); 
            }
            &ConsoleEvent::InputEvent(ref input_event) => {
                handle_key_event(input_event.key_event);
            }
            &ConsoleEvent::OutputEvent(ref output_event) => {
                try!(vga_buffer::print_string(&output_event.text).map_err(|_| "print_string failed"));
            }
        }
        event.mark_completed();

    }

}


fn handle_key_event(keyevent: KeyEvent) {

    // Ctrl+D or Ctrl+Alt+Del kills the OS
    if keyevent.modifiers.control && keyevent.keycode == Keycode::D || 
            keyevent.modifiers.control && keyevent.modifiers.alt && keyevent.keycode == Keycode::Delete {
        panic!("Ctrl+D or Ctrl+Alt+Del was pressed, abruptly (not cleanly) stopping the OS!"); //FIXME do this better, by signaling the main thread
    }

    // only print ascii values on a key press down
    if keyevent.action != KeyAction::Pressed {
        return; 
    }

    // if keyevent.modifiers.control && keyevent.keycode == Keycode::T {
    //     debug!("PIT_TICKS={}, RTC_TICKS={:?}, SPURIOUS={}, APIC={}", 
    //             ::interrupts::pit_clock::PIT_TICKS.load(Ordering::Relaxed), 
    //             rtc::get_rtc_ticks().ok(),
    //             unsafe{::interrupts::SPURIOUS_COUNT},
    //             ::interrupts::APIC_TIMER_TICKS.load(Ordering::Relaxed));
    //     return; 
    // }


    // PUT ADDITIONAL KEYBOARD-TRIGGERED BEHAVIORS HERE


    let ascii = keyevent.keycode.to_ascii(keyevent.modifiers);
    match ascii {
        Some(c) => { 
            // we echo key presses directly to the console without needing to queue an event
            // vga_buffer::print_args(format_args!("{}", c)); // probably a better way to do this...
            use alloc::string::ToString;
            vga_buffer::print_string(&c.to_string()).unwrap();
            // trace!("  {}  ", c);
        }
        // _ => { println!("Couldn't get ascii for keyevent {:?}", keyevent); } 
        _ => { } 
    }
}