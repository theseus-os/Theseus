use drivers::input::keyboard::KeyEvent;
use vga_buffer::{VgaBuffer, ColorCode, DisplayPosition};
use alloc::string::String;
use core::sync::atomic::Ordering;
use core::fmt;
use logger;
use spin::{Once, Mutex};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use rtc;
use task::spawn_kthread;


// original print macro
// #[macro_export]
// macro_rules! print {
//     ($($arg:tt)*) => ({
//             $crate::vga_buffer::print_args(format_args!($($arg)*));
//     });
// }



lazy_static! {
    static ref CONSOLE_VGA_BUFFER: Mutex<VgaBuffer> = Mutex::new(VgaBuffer::new());
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
        match $crate::console::print_to_console(s) {
            Ok(_) => { }
            Err(e) => error!("print_to_console() in print!() macro failed, error: {}", e),
        }
    });
}

/// calls print!() with an extra "\n" at the end. 
#[macro_export]
macro_rules! println {
    ($fmt:expr) => (print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"), $($arg)*));
}


/// the callback use in the logger crate for mirroring log functions to the console
fn mirror_to_vga_cb(_color: logger::LogColor, prefix: &'static str, args: fmt::Arguments) {
    println!("{} {}", prefix, args);
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


/// the console owns and creates the event queue, and returns a producer reference to the queue.
pub fn init() -> Result<DFQueueProducer<ConsoleEvent>, &'static str> {
    assert_has_not_been_called!("console_init was called more than once!");

    let console_dfq: DFQueue<ConsoleEvent> = DFQueue::new();
    let console_consumer = console_dfq.into_consumer();
    let returned_producer = console_consumer.obtain_producer();
    PRINT_PRODUCER.call_once(|| {
        console_consumer.obtain_producer()
    });

    if cfg!(feature = "mirror_serial") {
         // enables mirroring of serial port logging outputs to VGA buffer (for real hardware)
        logger::mirror_to_vga(mirror_to_vga_cb);
    }

    println!("{}", WELCOME_STRING);

    println!("Console says hello!\n");
    
    try!(spawn_kthread(main_loop, console_consumer, "console_loop"));
    Ok(returned_producer)
}


/// the main console event-handling loop, runs on its own thread. 
/// This is the only thread that is allowed to touch the vga buffer!
/// ## Returns
/// true if the thread was smoothly exited intentionally, false if forced to exit due to an error.
fn main_loop(consumer: DFQueueConsumer<ConsoleEvent>) -> Result<(), &'static str> { // Option<usize> just a placeholder because kthread functions must have one Argument right now... :(
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
                use core::fmt::Write;
                try!(CONSOLE_VGA_BUFFER.lock().write_str("\nSmoothly exiting console main loop.\n").map_err(|_| "error in VgaBuffer's write_str()"));
                return Ok(()); 
            }
            &ConsoleEvent::InputEvent(ref input_event) => {
                handle_key_event(input_event.key_event);
            }
            &ConsoleEvent::OutputEvent(ref output_event) => {
                CONSOLE_VGA_BUFFER.lock().write_string_with_color(&output_event.text, ColorCode::default());
            }
        }
        event.mark_completed();

    }

}


fn handle_key_event(keyevent: KeyEvent) {
    use drivers::input::keyboard::KeyAction;
    use keycodes_ascii::Keycode;

    // Ctrl+D or Ctrl+Alt+Del kills the OS
    if keyevent.modifiers.control && keyevent.keycode == Keycode::D || 
            keyevent.modifiers.control && keyevent.modifiers.alt && keyevent.keycode == Keycode::Delete {
        panic!("Ctrl+D or Ctrl+Alt+Del was pressed, abruptly (not cleanly) stopping the OS!"); //FIXME do this better, by signaling the main thread
    }


    // EVERYTHING BELOW HERE WILL ONLY OCCUR ON A KEY PRESS (not key release)
    if keyevent.action != KeyAction::Pressed {
        return; 
    }

    if keyevent.modifiers.control && keyevent.keycode == Keycode::T {
        // use core::fmt::Write;
        // use core::ops::DerefMut;
        let s = format!("PIT_TICKS={}, RTC_TICKS={:?}, SPURIOUS={}, APIC={}", 
            ::interrupts::pit_clock::PIT_TICKS.load(Ordering::Relaxed), 
            rtc::get_rtc_ticks().ok(),
            unsafe{::interrupts::SPURIOUS_COUNT},
            ::interrupts::APIC_TIMER_TICKS.load(Ordering::Relaxed)
        );

        CONSOLE_VGA_BUFFER.lock().write_string_with_color(&s, ColorCode::default());
        
        // debug!("PIT_TICKS={}, RTC_TICKS={:?}, SPURIOUS={}, APIC={}", 
        //         ::interrupts::pit_clock::PIT_TICKS.load(Ordering::Relaxed), 
        //         rtc::get_rtc_ticks().ok(),
        //         unsafe{::interrupts::SPURIOUS_COUNT},
        //         ::interrupts::APIC_TIMER_TICKS.load(Ordering::Relaxed));
        return; 
    }


    // PUT ADDITIONAL KEYBOARD-TRIGGERED BEHAVIORS HERE


    // home, end, page up, page down, up arrow, down arrow for the console
    if keyevent.keycode == Keycode::Home {
        CONSOLE_VGA_BUFFER.lock().display(DisplayPosition::Start);
        return;
    }
    if keyevent.keycode == Keycode::End {
        CONSOLE_VGA_BUFFER.lock().display(DisplayPosition::End);
        return;
    }
    if keyevent.keycode == Keycode::PageUp {
        CONSOLE_VGA_BUFFER.lock().display(DisplayPosition::Up(20));
        return;
    }
    if keyevent.keycode == Keycode::PageDown {
        CONSOLE_VGA_BUFFER.lock().display(DisplayPosition::Down(20));
        return;
    }
    if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Up {
        CONSOLE_VGA_BUFFER.lock().display(DisplayPosition::Up(1));
        return;
    }
    if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Down {
        CONSOLE_VGA_BUFFER.lock().display(DisplayPosition::Down(1));
        return;
    }


    match keyevent.keycode.to_ascii(keyevent.modifiers) {
        Some(c) => { 
            // we echo key presses directly to the console without queuing an event
            // trace!("  {}  ", c);
            use alloc::string::ToString;
            CONSOLE_VGA_BUFFER.lock().write_string_with_color(&c.to_string(), ColorCode::default());
        }
        // _ => { println!("Couldn't get ascii for keyevent {:?}", keyevent); } 
        _ => { } 
    }
}




// this doesn't line up as shown here because of the escaped backslashes,
// but it lines up properly when printed :)
const WELCOME_STRING: &'static str = "\n\n
 _____ _                              
|_   _| |__   ___  ___  ___ _   _ ___ 
  | | | '_ \\ / _ \\/ __|/ _ \\ | | / __|
  | | | | | |  __/\\__ \\  __/ |_| \\__ \\
  |_| |_| |_|\\___||___/\\___|\\__,_|___/ \n\n";
