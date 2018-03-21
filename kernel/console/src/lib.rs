#![no_std]
#![feature(alloc)]

extern crate keycodes_ascii;
extern crate vga_buffer;
#[macro_use] extern crate alloc;
extern crate spin;
extern crate dfqueue;
#[macro_use] extern crate log;
extern crate nano_core;

// temporary, should remove this once we fix crate system
extern crate console_types; 
use console_types::{ConsoleEvent, ConsoleOutputEvent};


use keycodes_ascii::{Keycode, KeyAction, KeyEvent};
use alloc::string::String;
use spin::Once;
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};

static PRINT_PRODUCER: Once<DFQueueProducer<ConsoleEvent>> = Once::new();


pub fn print_to_console(s: String) -> Result<(), &'static str> {
    let output_event = ConsoleEvent::OutputEvent(ConsoleOutputEvent::new(s));
    try!(PRINT_PRODUCER.try().ok_or("Console print producer isn't yet initialized!")).enqueue(output_event);
    Ok(())
}


/// the console owns and creates the event queue, and returns a producer reference to the queue.
pub fn init() -> Result<DFQueueProducer<ConsoleEvent>, &'static str> {
    let console_dfq: DFQueue<ConsoleEvent> = DFQueue::new();
    let console_consumer = console_dfq.into_consumer();
    let returned_producer = console_consumer.obtain_producer();
    PRINT_PRODUCER.call_once(|| {
        console_consumer.obtain_producer()
    });

    use nano_core::spawn_kthread;

    if true {
        // vga_buffer::print_str("console::init() trying to spawn_kthread...\n").unwrap();
        info!("console::init() trying to spawn_kthread...");
        try!(spawn_kthread(main_loop, console_consumer, String::from("console_loop")));
        // vga_buffer::print_str("console::init(): successfully spawned kthread!\n").unwrap();
        info!("console::init(): successfully spawned kthread!");
    }
    else {
        vga_buffer::print_str("console::init(): skipping spawn_kthread to test.\n").unwrap();
    }
    try!(print_to_console(String::from("Console says hello!\n")));
    Ok(returned_producer)
}


/// the main console event-handling loop, should be run on its own thread. 
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
        // info!("console_loop: got event {:?}", event);
        // vga_buffer::print_string(&format!("console_loop: got event {:?}\n", event)).unwrap();
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