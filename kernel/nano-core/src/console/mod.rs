use drivers::input::keyboard::KeyEvent;
use drivers::vga_buffer;
use collections::string::String;
use irq_safety::RwLockIrqSafeWriteGuard;
use task::TaskList;
use core::sync::atomic::Ordering;
use spin::Once;
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};



// original print macro
// #[macro_export]
// macro_rules! print {
//     ($($arg:tt)*) => ({
//             $crate::drivers::vga_buffer::print_args(format_args!($($arg)*));
//     });
// }





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
            let mut s: String = String::new();
            write!(&mut s, $($arg)*);
            $crate::console::print_to_console(s).unwrap();
    });
}


static PRINT_PRODUCER: Once<DFQueueProducer<ConsoleEvent>> = Once::new();


pub fn print_to_console<S>(s: S) -> Result<(), &'static str> where S: Into<String> {
    let output_event = ConsoleEvent::OutputEvent(ConsoleOutputEvent::new(s.into()));
    PRINT_PRODUCER.try().expect("Console print producer isn't yet initialized!").enqueue(output_event);
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


/// the console does not own the event queue, it merely receives a reference to it. 
/// BUT WHO SHOULD OWN THE EVENT QUEUE? TODO: answer this question
// pub fn console_init(event_queue: &VecDeque<ConsoleEvent>, tasklist_mut: mut RwLockIrqSafeWriteGuard<TaskList>) -> bool {
pub fn console_init(mut tasklist_mut: RwLockIrqSafeWriteGuard<TaskList>) -> DFQueueProducer<ConsoleEvent> {
    assert_has_not_been_called!("console_init was called more than once!");

    let console_dfq: DFQueue<ConsoleEvent> = DFQueue::new();
    let console_consumer = console_dfq.into_consumer();
    let returned_producer = console_consumer.obtain_producer();
    PRINT_PRODUCER.call_once(|| {
        console_consumer.obtain_producer()
    });

    print_to_console("Console says hello!");
    
    tasklist_mut.spawn_kthread(main_loop, console_consumer, "console_loop");
    returned_producer
}


/// the main console event-handling loop, runs on its own thread. 
/// This is the only thread that is allowed to touch the vga buffer!
/// ## Returns
/// true if the thread was smoothly exited intentionally, false if forced to exit due to an error.
fn main_loop(consumer: DFQueueConsumer<ConsoleEvent>) -> bool { // Option<usize> just a placeholder because kthread functions must have one Argument right now... :(
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
                vga_buffer::print_str("\nSmoothly exiting console main loop.\n");
                return true; 
            }
            &ConsoleEvent::InputEvent(ref input_event) => {
                handle_key_event(input_event.key_event);
            }
            &ConsoleEvent::OutputEvent(ref output_event) => {
                vga_buffer::print_string(&output_event.text);
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

    // only print ascii values on a key press down
    if keyevent.action != KeyAction::Pressed {
        return; 
    }

    if keyevent.modifiers.control && keyevent.keycode == Keycode::T {
        debug!("PIT_TICKS={}, RTC_TICKS={}", 
                ::interrupts::pit_clock::PIT_TICKS.load(Ordering::Relaxed), 
                ::interrupts::rtc::RTC_TICKS.load(Ordering::Relaxed));
        return; 
    }


    // PUT ADDITIONAL KEYBOARD-TRIGGERED BEHAVIORS HERE


    let ascii = keyevent.keycode.to_ascii(keyevent.modifiers);
    match ascii {
        Some(c) => { 
            // we echo key presses directly to the console without needing to queue an event
            vga_buffer::print_args(format_args!("{}", c)); // probably a better way to do this...
        }
        // _ => { println!("Couldn't get ascii for keyevent {:?}", keyevent); } 
        _ => { } 
    }
}