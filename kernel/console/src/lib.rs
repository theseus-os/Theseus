#![no_std]
#![feature(alloc)]
extern crate keycodes_ascii;
extern crate vga_buffer;
extern crate spin;
extern crate dfqueue;
extern crate atomic_linked_list; 
extern crate mod_mgmt;
extern crate spawn;
extern crate task;
extern crate memory;

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
// extern crate window_manager;

// temporary, should remove this once we fix crate system
extern crate console_types; 
use console_types::{ConsoleEvent, ConsoleOutputEvent};

// temporary, use until we find some other way to register commands to the terminal
// extern crate coreutils;

use vga_buffer::{VgaBuffer, ColorCode, DisplayPosition};
use keycodes_ascii::{Keycode, KeyAction, KeyEvent};
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
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
        use core::fmt::Write;
        use alloc::String;
        let mut s: String = String::new();
        match write!(&mut s, $($arg)*) {
            Ok(_) => { 
                if let Err(e) = $crate::print_to_console(s) {
                    error!("print!(): print_to_console failed, error: {}", e);

                }
            }
            Err(err) => {
                error!("print!(): writing to String failed, error: {}", err);
            }
        }
    });
}


// type MainFuncSignature = fn(Vec<String>) -> Result<String, &'static str>; 
// This is the function signature that all new application modules must implement

lazy_static! {
    static ref CONSOLE_VGA_BUFFER: Mutex<VgaBuffer> = Mutex::new(VgaBuffer::new());
}

static PRINT_PRODUCER: Once<DFQueueProducer<ConsoleEvent>> = Once::new();


/// Queues up the given `String` to be printed out to the console.
pub fn print_to_console(s: String) -> Result<(), &'static str> {
    let output_event = ConsoleEvent::OutputEvent(ConsoleOutputEvent::new(s));
    try!(PRINT_PRODUCER.try().ok_or("Console print producer isn't yet initialized!")).enqueue(output_event);
    Ok(())
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

    try!(print_to_console(String::from(WELCOME_STRING)));
    try!(print_to_console(String::from("Console says hello!\n")));
    try!(print_to_console(String::from("Press Ctrl+C to quit a task\n")));
    
    Ok(returned_producer)
}



/// the main console event-handling loop, runs on its own thread. 
/// This is the only thread that is allowed to touch the vga buffer!
/// It's an infinite loop, but will return if forced to exit because of an error. 
fn main_loop(consumer: DFQueueConsumer<ConsoleEvent>) -> Result<(), &'static str> { // Option<usize> just a placeholder because kthread functions must have one Argument right now... :(
    use core::ops::Deref;
    let mut console_input_string = String::new();
    let mut current_task_id = 0;
    // Indicates the leftmost bound that the cursor can travel
    let mut max_left_pos: u16 = 12 * 80 + 13;
    //Indicates the rightmost bound that the cursor can travel, dictated by the rightmost character typed by the user
    let mut text_offset: u16 = max_left_pos;
    // Indicates the current position of the cursor
    let mut cursor_pos: u16 = max_left_pos;

    
    try!(print_to_console(String::from("\ntype command:")));
    // initializes the cursor to the starting position
    vga_buffer::init_cursor();
    vga_buffer::update_cursor(13,12);


    loop { 
        let event = match consumer.peek() {
            Some(ev) => ev,
            _ => { continue; }
        };

        // Constantly checks to see if command thread has finished or manually killed
        // task id is 0 if there are no command line tasks running
        if current_task_id != 0 {
            // gets the task from the current task id variable
            let result = task::get_task(current_task_id);
            if let Some(ref task_result)  = result {
                let end_task = task_result.read();
                let exit_result = end_task.get_exit_value();
                // match statement will see if the task has finished with an exit value yet
                match exit_result {
                    Some(exit_val) => {
                        let my_exit_val = exit_val.clone();
                        match my_exit_val {
                            // if the task finishes successfully
                            Ok(boxed_val) => {
                                if boxed_val.downcast_ref::<isize>().is_some() {
                                    print_to_console(format!("finished with exit value {:?}\n", boxed_val.downcast_ref::<isize>().unwrap()))?;
                                }
                            },
                            // if the user presses Ctrl+C
                            Err(task::KillReason::Requested) => {
                                try!(print_to_console(format!("\ncommand manually aborted\n")));
                            }
                            // catches any other errors
                            Err(_) => {
                                try!(print_to_console(String::from("\ntask could not be run\n")));
                            },
                        }
                        // Resets the current task id to be ready for the next command
                        current_task_id = 0;
                        try!(print_to_console(String::from("\ntype command:")));
                    },
                    // None value indicates task has not yet finished so does nothing
                    None => {
                    },
                }
            }   
        }
        

        match event.deref() {
            &ConsoleEvent::ExitEvent => {
                use core::fmt::Write;
                try!(CONSOLE_VGA_BUFFER.lock().write_str("\nSmoothly exiting console main loop.\n")
                    .map_err(|_| "fmt::Error in VgaBuffer's write_str()")
                );
                return Ok(()); 
            }
            
            &ConsoleEvent::InputEvent(ref input_event) => {
                try!(handle_key_event(input_event.key_event, &mut console_input_string, 
                    &mut current_task_id, &mut max_left_pos, &mut text_offset, &mut cursor_pos));
            }
            &ConsoleEvent::OutputEvent(ref output_event) => {
                try!(CONSOLE_VGA_BUFFER.lock().write_string_with_color(&output_event.text, ColorCode::default())
                    .map_err(|_| "fmt::Error in VgaBuffer's write_string_with_color()")
                );
            }
        }

        // Updates the cursor to a new position 
        let new_x = CONSOLE_VGA_BUFFER.lock().column as u16;
        let display_line = CONSOLE_VGA_BUFFER.lock().display_line;
        let new_y = if display_line < 24 {display_line as u16} else {24 as u16};
        vga_buffer::update_cursor(new_x, new_y);
        event.mark_completed();
    }
}


fn handle_key_event(keyevent: KeyEvent, console_input_string: &mut String, current_task_id: &mut usize, 
    max_left_pos: &mut u16 ,text_offset: &mut u16 ,cursor_pos: &mut u16) -> Result<(), &'static str> {
    // Finds current coordinates of the VGA buffer
    let y = CONSOLE_VGA_BUFFER.lock().display_line as u16;
    let x = CONSOLE_VGA_BUFFER.lock().column as u16;

    // Ctrl+D or Ctrl+Alt+Del kills the OS
    if keyevent.modifiers.control && keyevent.keycode == Keycode::D
     || 
            keyevent.modifiers.control && keyevent.modifiers.alt && keyevent.keycode == Keycode::Delete {
    panic!("Ctrl+D or Ctrl+Alt+Del was pressed, abruptly (not cleanly) stopping the OS!"); //FIXME do this better, by signaling the main thread
    }

    // Ctrl+C signals the main loop to exit the task
    if keyevent.modifiers.control && keyevent.keycode == Keycode::C {
        
        if *current_task_id != 0 {
            let task_ref = task::get_task(*current_task_id);
            if task_ref.is_some() {
                let _result = task_ref.unwrap().write().kill(task::KillReason::Requested);
            }
        } 
        return Ok(());
    }

    // EVERYTHING BELOW HERE WILL ONLY OCCUR ON A KEY PRESS (not key release)
    if keyevent.action != KeyAction::Pressed {
        return Ok(()); 
    }

    // Prevents any keypress below here from registering if a command is running
    if *current_task_id != 0 {
        return Ok(());
    }

    // PUT ADDITIONAL KEYBOARD-TRIGGERED BEHAVIORS HERE

    // Controls cursor movement as the user types, excluding the backspace and enter key, which are special
    if keyevent.keycode != Keycode::Enter && keyevent.keycode.to_ascii(keyevent.modifiers).is_some()
        && keyevent.keycode != Keycode::Backspace && keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
            if *text_offset == *cursor_pos {
                if keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
                    console_input_string.push(keyevent.keycode.to_ascii(keyevent.modifiers).unwrap());    
                }
            } else {
                // controls cursor movement and associated variables if the cursor is not at the end of the current line
                let insert_idx: usize = *cursor_pos as usize - *max_left_pos as usize;
                console_input_string.remove(insert_idx); // Take this out once you can dynamically shift buffer 
                if keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
                    console_input_string.insert(insert_idx,keyevent.keycode.to_ascii(keyevent.modifiers).unwrap()); 
                }
            }
            // DON'T RETURN HERE
    }

    if keyevent.keycode == Keycode::Backspace  {
        // Prevents user from moving cursor to the left of the typing bounds
        if *cursor_pos == *max_left_pos {    
            return Ok(());
        } else {
            let remove_idx: usize =  *cursor_pos as usize - *max_left_pos as usize-1;
            console_input_string.remove(remove_idx);
            if *cursor_pos < *text_offset {console_input_string.insert(remove_idx, ' ')};
            // DON'T RETURN HERE
        }
    }

    if keyevent.keycode == Keycode::Enter && keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
        // Does nothing if the user presses enter without any command
        if console_input_string.len() == 0 {
            return Ok(());
        } else if *current_task_id != 0 { // prevents the user from trying to execute a new command while one is currently running
            try!(print_to_console(String::from("Wait until the current command is finished executing\n")));        
        } else {
            // Calls the parse_input function to see if the command exists in the command table
            let command_structure = parse_input(console_input_string);
            match run_command_new_thread(command_structure, current_task_id) {
                    Ok(()) => {
                        // try!(print_to_console(String::from("done\n")));
                    } Err("Error: no module with this name found!") => {
                        try!(print_to_console(String::from("command not found\n")));
                        try!(print_to_console(String::from("\ntype command: ")));
                    
                    } Err(&_) => {
                        try!(print_to_console(String::from("running command on new thread failed\n")));
                        try!(print_to_console(String::from("\ntype command: ")));
                    }
                }

        };
        // Clears the buffer for another command once current command is finished executing
        console_input_string.clear();
         
         // Updates the cursor tracking variables when the enter key is pressed 
        *text_offset  = y * 80 + x;
        *cursor_pos = y * 80 + x;
        *max_left_pos =  y * 80 + x;
        
    }

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

    // Moves the cursor to the left and right 
    if keyevent.keycode == Keycode::Left {
        if *cursor_pos > *max_left_pos {
            CONSOLE_VGA_BUFFER.lock().column -= 1;
            *cursor_pos -=1;
            return Ok(());
        }
    }
    if keyevent.keycode == Keycode::Right {
        if *cursor_pos < *text_offset {
            CONSOLE_VGA_BUFFER.lock().column += 1;
            *cursor_pos += 1;
            return Ok(());
        }
    }
    
    /*
        //Pass TAB event to window manager
        //Window manager consumes direction key input
        match keyevent.keycode {
            Keycode::Tab => {
                //window_manager::set_time_start();
                loop{
                    window_manager::window_switch();
                }
            }
            Keycode::LeCOMMAND EXITft|Keycode::Right|Keycode::Up|Keycode::Down => {
                window_manager::put_key_code(keyevent.keycode).unwrap();
            }
            _ => {}
        }

        //Pass Delete event and direction key event to 3d drawer application
        /*match keyevent.keycode {
            Keycode::Tab|Keycode::Delete|Keycode::Left|Keycode::Right|Keycode::Up|Keycode::Down => {
                graph_drawer::put_key_code(keyevent.keycode).unwrap();
            }c
            _ => {}
        } */
    */
    

    match keyevent.keycode.to_ascii(keyevent.modifiers) {
        Some(c) => { 
            // we echo key presses directly to the console without queuing an event
            // trace!("  {}  ", c);
            try!(CONSOLE_VGA_BUFFER.lock().write_string_with_color(&c.to_string(), ColorCode::default())
                .map_err(|_| "fmt::Error in VgaBuffer's write_string_with_color()")
            );
            
            // adjusts the cursor tracking variables when the enter or the backspace keys are pressed
            // cursor tracking variables for all other keys are handled above
            if keyevent.keycode == Keycode::Backspace {
                if *cursor_pos == *text_offset {*text_offset -= 1;}
                *cursor_pos -= 1;
            } else if keyevent.keycode != Keycode::Enter {
                if *cursor_pos == *text_offset {*text_offset += 1;}
                *cursor_pos += 1;
            }
        }
        _ => { } 
    }

    Ok(())
}

#[derive(Debug)] // Need in order to use spawn::kthread()11
// Struct contains the command string and its arguments
struct CommandStruct {
    command_str: String,
    arguments: Vec<String>
}


fn parse_input(console_input_string: &mut String) -> CommandStruct {
    // This function parses the string that the user inputted when Enter is pressed and populates the CommandStruct
    let mut words: Vec<String> = console_input_string.split_whitespace().map(|s| s.to_string()).collect();
    let mut command_string = words.remove(0);
    // Formats the string into the application module syntax
    command_string.insert(0, '_');
    command_string.insert(0, 'a');
    command_string.insert(0, '_');
    command_string.insert(0, '_');
    // Forms command structure to pass to the function that runs command on the new thread
    let command_structure = CommandStruct {
        command_str: command_string.to_string(),
        arguments: words
    };
    return command_structure;

}


fn run_command_new_thread(command_structure: CommandStruct, current_task_id: &mut usize) -> Result<(),&'static str> {
    // Function will execute the command on a new thread 
    use memory; 
    let module = memory::get_module(&command_structure.command_str).ok_or("Error: no module with this name found!")?;
    let args = command_structure.arguments;

    #[cfg(feature = "loadable")]
    {
        let section = mod_mgmt::metadata::get_symbol("spawn::spawn_application").upgrade().ok_or("no symbol: spawn::spawn_application")?;
        let mut space = 0;
        let func: & fn(&ModuleArea, Vec<String>, Option<String>, Option<u8>) -> Result<TaskRef, &'static str> = 
            section.mapped_pages()
            .ok_or("Couldn't get section's mapped_pages for \"spawn::spawn_application\"")?
            .as_func(section.mapped_pages_offset(), &mut space)?; 
        func(module, args, None, None)?;
    }
    #[cfg(not(feature = "loadable"))]
    {
        let taskref = spawn::spawn_application(module, args, None, None)?;
        // Gets the task id so we can reference this task if we need to kill it with Ctrl+C
        *current_task_id = taskref.read().id;
        return Ok(());
    }
    
}

const WELCOME_STRING: &'static str = "\n\n
 _____ _                              
|_   _| |__   ___  ___  ___ _   _ ___ 
  | | | '_ \\ / _ \\/ __|/ _ \\ | | / __|
  | | | | | |  __/\\__ \\  __/ |_| \\__ \\
  |_| |_| |_|\\___||___/\\___|\\__,_|___/ \n\n ";
