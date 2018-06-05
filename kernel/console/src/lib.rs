#![no_std]
#![feature(alloc)]

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
#[macro_use] extern crate lazy_static;
extern crate spin;
extern crate dfqueue;
extern crate keycodes_ascii;
#[macro_use] extern crate vga_buffer;
// extern crate window_manager;
extern crate spawn;

// temporary, should remove this once we fix crate system
extern crate console_types; 
use console_types::{ConsoleEvent, ConsoleOutputEvent};


use vga_buffer::{VgaBuffer, ColorCode, DisplayPosition};
use keycodes_ascii::{Keycode, KeyAction, KeyEvent};
use alloc::string::String;
use spin::{Once, Mutex};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};




/// Calls `print!()` with an extra newilne ('\n') appended to the end. 
#[macro_export]
macro_rules! println {
    ($fmt:expr) => (print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"), $($arg)*));
}

/// The main printing macro, which simply pushes an output event to the console's event queue. 
/// This ensures that only one thread (the console acting as a consumer) ever accesses the GUI.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ({
        $crate::print_to_console_args(format_args!($($arg)*));
    });
}



lazy_static! {
    static ref CONSOLE_VGA_BUFFER: Mutex<VgaBuffer> = Mutex::new(VgaBuffer::new());
}

static PRINT_PRODUCER: Once<DFQueueProducer<ConsoleEvent>> = Once::new();


/// Queues up the given string to be printed out to the console.
/// If the console hasn't yet been initialized, it prints to the VGA buffer directly using `print_raw!()`.
pub fn print_to_console<S: Into<String>>(s: S) {
    if let Some(print_producer) = PRINT_PRODUCER.try() {
        let output_event = ConsoleEvent::OutputEvent(ConsoleOutputEvent::new(s.into()));
        print_producer.enqueue(output_event);
    }
    else {
        print_raw!("[no console yet]: {}", s.into());
    }
}

use core::fmt;
/// Converts the given `core::fmt::Arguments` to a `String` and queues it up to be printed out to the console.
/// If the console hasn't yet been initialized, it prints to the VGA buffer directly using `print_raw!()`.
pub fn print_to_console_args(fmt_args: fmt::Arguments) {
    print_to_console(format!("{}", fmt_args));
}


/// Initializes the console by spawning a new thread to handle all console events, and creates a new event queue. 
/// This event queue's consumer is given to that console thread, and a producer reference to that queue is returned. 
/// This allows other modules to push console events onto the queue. 
pub fn init() -> Result<DFQueueProducer<ConsoleEvent>, &'static str> {
    let console_dfq: DFQueue<ConsoleEvent> = DFQueue::new();
    let console_consumer = console_dfq.into_consumer();
    let returned_producer = console_consumer.obtain_producer();
    PRINT_PRODUCER.call_once(|| {
        console_consumer.obtain_producer()
    });

    info!("console::init() trying to spawn_kthread...");
    try!(spawn::spawn_kthread(main_loop, console_consumer, String::from("console_loop"), None));
    // vga_buffer::print_str("console::init(): successfully spawned kthread!\n").unwrap();
    info!("console::init(): successfully spawned kthread!");

    print_to_console(String::from(WELCOME_STRING));
    print_to_console(String::from("Console says hello!\n"));
    Ok(returned_producer)
}



/// the main console event-handling loop, runs on its own thread. 
/// This is the only thread that is allowed to touch the vga buffer!
/// It's an infinite loop, but will return if forced to exit because of an error. 
fn main_loop(consumer: DFQueueConsumer<ConsoleEvent>) -> Result<(), &'static str> { // Option<usize> just a placeholder because kthread functions must have one Argument right now... :(
    use core::ops::Deref;

    loop { 
        let event = match consumer.peek() {
            Some(ev) => ev,
            _ => { continue; }
        };

        match event.deref() {
            &ConsoleEvent::ExitEvent => {
                use core::fmt::Write;
                try!(CONSOLE_VGA_BUFFER.lock().write_str("\nSmoothly exiting console main loop.\n")
                    .map_err(|_| "fmt::Error in VgaBuffer's write_str()")
                );
                return Ok(()); 
            }
            &ConsoleEvent::InputEvent(ref input_event) => {
                try!(handle_key_event(input_event.key_event));
            }
            &ConsoleEvent::OutputEvent(ref output_event) => {
                try!(CONSOLE_VGA_BUFFER.lock().write_string_with_color(&output_event.text, ColorCode::default())
                    .map_err(|_| "fmt::Error in VgaBuffer's write_string_with_color()")
                );
            }
        }

        event.mark_completed();
    }
}


fn handle_key_event(keyevent: KeyEvent) -> Result<(), &'static str> {

    // Ctrl+D or Ctrl+Alt+Del kills the OS
    if keyevent.modifiers.control && keyevent.keycode == Keycode::D || 
            keyevent.modifiers.control && keyevent.modifiers.alt && keyevent.keycode == Keycode::Delete {
        panic!("Ctrl+D or Ctrl+Alt+Del was pressed, abruptly (not cleanly) stopping the OS!"); //FIXME do this better, by signaling the main thread
    }


    // EVERYTHING BELOW HERE WILL ONLY OCCUR ON A KEY PRESS (not key release)
    if keyevent.action != KeyAction::Pressed {
        return Ok(()); 
    }

    // if keyevent.modifiers.control && keyevent.keycode == Keycode::T {
    //     // use core::fmt::Write;
    //     // use core::ops::DerefMut;
    //     let s = format!("PIT_TICKS={}, RTC_TICKS={:?}, SPURIOUS={}, APIC={}", 
    //         ::interrupts::pit_clock::PIT_TICKS.load(Ordering::Relaxed), 
    //         rtc::get_rtc_ticks().ok(),
    //         unsafe{::interrupts::SPURIOUS_COUNT},
    //         ::interrupts::APIC_TIMER_TICKS.load(Ordering::Relaxed)
    //     );

    //     CONSOLE_VGA_BUFFER.lock().write_string_with_color(&s, ColorCode::default());
        
    //     // debug!("PIT_TICKS={}, RTC_TICKS={:?}, SPURIOUS={}, APIC={}", 
    //     //         ::interrupts::pit_clock::PIT_TICKS.load(Ordering::Relaxed), 
    //     //         rtc::get_rtc_ticks().ok(),
    //     //         unsafe{::interrupts::SPURIOUS_COUNT},
    //     //         ::interrupts::APIC_TIMER_TICKS.load(Ordering::Relaxed));
    //     return; 
    // }


    // PUT ADDITIONAL KEYBOARD-TRIGGERED BEHAVIORS HERE


    // home, end, page up, page down, up arrow, down arrow for the console
    if keyevent.keycode == Keycode::Home {
        CONSOLE_VGA_BUFFER.lock().display(DisplayPosition::Start);
        return Ok(());
    }
    if keyevent.keycode == Keycode::End {
        CONSOLE_VGA_BUFFER.lock().display(DisplayPosition::End);
        return Ok(());
    }
    if keyevent.keycode == Keycode::PageUp {
        CONSOLE_VGA_BUFFER.lock().display(DisplayPosition::Up(20));
        return Ok(());
    }
    if keyevent.keycode == Keycode::PageDown {
        CONSOLE_VGA_BUFFER.lock().display(DisplayPosition::Down(20));
        return Ok(());
    }
    if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Up {
        CONSOLE_VGA_BUFFER.lock().display(DisplayPosition::Up(1));
        return Ok(());
    }
    if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Down {
        CONSOLE_VGA_BUFFER.lock().display(DisplayPosition::Down(1));
        return Ok(());
    }
    
    /*
        //Pass TAB event to window manager
        //Window manager consumes dir();ection key input
        match keyevent.keycode {
            Keycode::Tab => {
                //window_manager::set_time_start();
                loop{
                    window_manager::window_switch();
                }
            }
            Keycode::Left|Keycode::Right|Keycode::Up|Keycode::Down => {
                window_manager::put_key_code(keyevent.keycode).unwrap();
            }
            _ => {}
        }

        //Pass Delete event and direction key event to 3d drawer application
        /*match keyevent.keycode {
            Keycode::Tab|Keycode::Delete|Keycode::Left|Keycode::Right|Keycode::Up|Keycode::Down => {
                graph_drawer::put_key_code(keyevent.keycode).unwrap();
            }
            _ => {}
        }*/
    */

    match keyevent.keycode.to_ascii(keyevent.modifiers) {
        Some(c) => { 
            // we echo key presses directly to the console without queuing an event
            // trace!("  {}  ", c);
            use alloc::string::ToString;
            try!(CONSOLE_VGA_BUFFER.lock().write_string_with_color(&c.to_string(), ColorCode::default())
                .map_err(|_| "fmt::Error in VgaBuffer's write_string_with_color()")
            );
        }
        _ => { } 
    }

    Ok(())
}




// this doesn't line up as shown here because of the escaped backslashes,
// but it lines up properly when printed :)
const WELCOME_STRING: &'static str = "\n\n
 _____ _                              
|_   _| |__   ___  ___  ___ _   _ ___ 
  | | | '_ \\ / _ \\/ __|/ _ \\ | | / __|
  | | | | | |  __/\\__ \\  __/ |_| \\__ \\
  |_| |_| |_|\\___||___/\\___|\\__,_|___/ \n\n";
