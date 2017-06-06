use collections::VecDeque;
use drivers::input::keyboard::KeyEvent;
use drivers::vga_buffer;
use collections::string::String;
use irq_safety::RwLockIrqSafeWriteGuard;
use task::TaskList;
use core::sync::atomic::Ordering;


// TODO: transform this into a generic producer and consumer template, 
//       and this file will be the single consumer of data that pushes it to the vga buffer



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
            let output_event = $crate::console::ConsoleEvent::OutputEvent(
                $crate::console::ConsoleOutputEvent::new(s));
            $crate::console::queue_event(output_event);
    });
}


static mut CONSOLE_QUEUE: Option<VecDeque<ConsoleEvent>> = None;


pub fn queue_event(e: ConsoleEvent) -> Result<(), &'static str> {
    /// SAFE: just appending an event to the back of the queue
    let mut cq = unsafe { CONSOLE_QUEUE.as_mut().expect("CONSOLE_QUEUE uninintialized") }; 
    cq.push_back(e);
    Ok(())
}

#[derive(Debug)]
pub enum ConsoleEvent {
    InputEvent(ConsoleInputEvent),
    OutputEvent(ConsoleOutputEvent),
    ExitEvent,
}

/// use this to deliver input events (such as keyboard input) to the console.
#[derive(Debug)]
pub struct ConsoleInputEvent {
    key_event: KeyEvent,
}

impl ConsoleInputEvent {
    pub fn new(ke: KeyEvent) -> ConsoleInputEvent {
        ConsoleInputEvent {
            key_event: ke,
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
pub fn console_init(mut tasklist_mut: RwLockIrqSafeWriteGuard<TaskList>) -> bool {
    assert_has_not_been_called!("console_init was called more than once!");

    unsafe { 
        CONSOLE_QUEUE = Some(VecDeque::with_capacity(256));
    }
    tasklist_mut.spawn(main_loop, unsafe { CONSOLE_QUEUE.as_mut().expect("CONSOLE_QUEUE failed to initialize") }, "console_loop");
    true
}


/// the main console event-handling loop, runs on its own thread. 
/// This is the only thread that is allowed to touch the vga buffer!
/// ## Returns
/// true if the thread was smoothly exited intentionally, false if forced to exit due to an error.
fn main_loop(event_queue: &mut VecDeque<ConsoleEvent>) -> bool {

    loop { 
        // pop_front() is a non-blocking call right now that returns None if the queue is empty
        // we should probably make it a blocking (non-spinlock) call once we have wait events working
        let event: Option<ConsoleEvent> = event_queue.pop_front();
        if event.is_none() {
            continue; 
        }

        match event.unwrap() {
            ConsoleEvent::ExitEvent => {
                vga_buffer::print_str("\nSmoothly exiting console main loop.\n");
                return true; 
            }
            ConsoleEvent::InputEvent(input_event) => {
                handle_key_event(input_event.key_event);
            }
            ConsoleEvent::OutputEvent(output_event) => {
                vga_buffer::print_string(output_event.text);
            }
        }
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