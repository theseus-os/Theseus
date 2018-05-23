#![no_std]
#![feature(alloc)]

extern crate keycodes_ascii;
extern crate vga_buffer;
extern crate alloc;
extern crate spin;
extern crate dfqueue;
extern crate atomic_linked_list; 

extern crate spawn;
extern crate task;

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;

// extern crate window_manager;


// temporary, should remove this once we fix crate system
extern crate console_types; 
use console_types::{ConsoleEvent, ConsoleOutputEvent};

// temporary, use until we find some other way to register commands to the terminal
extern crate coreutils;

use vga_buffer::{VgaBuffer, ColorCode, DisplayPosition};
use keycodes_ascii::{Keycode, KeyAction, KeyEvent};
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use spin::{Once, Mutex};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use atomic_linked_list::atomic_map::AtomicMap;
use task::{RunState};


lazy_static! {
    static ref CONSOLE_VGA_BUFFER: Mutex<VgaBuffer> = Mutex::new(VgaBuffer::new());
    static ref COMMAND_TABLE: AtomicMap<String, fn() -> String> = AtomicMap::new();
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
    build_command_table();


    try!(print_to_console(String::from(WELCOME_STRING)));
    try!(print_to_console(String::from("Console says hello!\n")));
    
    Ok(returned_producer)
}



/// the main console event-handling loop, runs on its own thread. 
/// This is the only thread that is allowed to touch the vga buffer!
/// It's an infinite loop, but will return if forced to exit because of an error. 
fn main_loop(consumer: DFQueueConsumer<ConsoleEvent>) -> Result<(), &'static str> { // Option<usize> just a placeholder because kthread functions must have one Argument right now... :(
    use core::ops::Deref;
    let mut console_input_string = String::new();
    let mut current_task_id = 0;
    // let mut max_left_pos = 13;
    // let mut max_right_pos = 13;
    let mut max_left_pos: u16 = 12 * 80 + 13;
    let mut text_offset: u16 = max_left_pos;
    let mut cursor_pos: u16 = max_left_pos;

    
    try!(print_to_console(String::from("\ntype command:")));

    vga_buffer::init_cursor();
    vga_buffer::update_cursor(13,12);


    loop { 
        let event = match consumer.peek() {
            Some(ev) => ev,
            _ => { continue; }
        };


        if current_task_id != 0 {
            let task_ref = task::get_task(current_task_id);
            if task_ref.unwrap().read().runstate == RunState::EXITED {
                current_task_id = 0;
                try!(print_to_console(String::from("\ntype command:")));
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
        let new_x = CONSOLE_VGA_BUFFER.lock().column as u16;
        let display_line = CONSOLE_VGA_BUFFER.lock().display_line;
        let new_y = if display_line < 24 {display_line as u16} else {24 as u16};
        vga_buffer::update_cursor(new_x, new_y);

        event.mark_completed();
    }
}


fn handle_key_event(keyevent: KeyEvent, console_input_string: &mut String, current_task_id: &mut usize, 
    max_left_pos: &mut u16 ,text_offset: &mut u16 ,cursor_pos: &mut u16) -> Result<(), &'static str> {
    let y = CONSOLE_VGA_BUFFER.lock().display_line as u16;
    let x = CONSOLE_VGA_BUFFER.lock().column as u16;

    // Ctrl+D or Ctrl+Alt+Del kills the OS
    if keyevent.modifiers.control && keyevent.keycode == Keycode::D
     || 
            keyevent.modifiers.control && keyevent.modifiers.alt && keyevent.keycode == Keycode::Delete {
    panic!("Ctrl+D or Ctrl+Alt+Del was pressed, abruptly (not cleanly) stopping the OS!"); //FIXME do this better, by signaling the main thread
    }
    
    // Ctrl+C kills the current task
    if keyevent.modifiers.control && keyevent.keycode == Keycode::C {
        
        if *current_task_id != 0 {
            let task_ref = task::get_task(*current_task_id);
            task_ref.unwrap().write().set_runstate(RunState::EXITED);
            *current_task_id = 0;
            try!(print_to_console(String::from("COMMAND EXITED\n")));
            console_input_string.clear();
            try!(print_to_console(String::from("\ntype command:")));
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


    if keyevent.keycode == Keycode::Backspace  {
        if *cursor_pos == *max_left_pos {    
            return Ok(());
        } else {
            let remove_idx: usize =  *cursor_pos as usize - *max_left_pos as usize-1;
            console_input_string.remove(remove_idx);
            if *cursor_pos < *text_offset {console_input_string.insert(remove_idx, ' ')};
            // DON'T RETURN HERE
        }
    }

       if keyevent.keycode != Keycode::Enter && keyevent.keycode.to_ascii(keyevent.modifiers).is_some()
        && keyevent.keycode != Keycode::Backspace && keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
            if *text_offset == *cursor_pos {
                console_input_string.push(keyevent.keycode.to_ascii(keyevent.modifiers).unwrap());    
            } else {
                let insert_idx: usize = *cursor_pos as usize - *max_left_pos as usize;
                console_input_string.remove(insert_idx); // Take this out once you can dynamically shift buffer 
                console_input_string.insert(insert_idx,keyevent.keycode.to_ascii(keyevent.modifiers).unwrap()); 
            }
            // DON'T RETURN HERE
    }

    if keyevent.keycode == Keycode::Enter && keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
        // Does nothing if the user presses enter without any command
        if console_input_string.len() == 0 {
            return Ok(());
        } else if *current_task_id != 0 {
            try!(print_to_console(String::from("Wait until the current command is finished executing\n")));        
        } else {
            // Calls the match_command function to see if the command exists in the command table
            match match_command(console_input_string){
                Ok(command_string) => {
                    // Spawns new thread if good
                    match run_command_new_thread(command_string, current_task_id) {
                        Ok(()) => {
                            // try!(print_to_console(String::from("done\n")));
                        } Err(&_) => {
                            try!(print_to_console(String::from("running command on new thread failed\n")));
                        }
                    }
                } Err(&_) => {
                    try!(print_to_console(String::from("ERROR: NOT A VALID COMMAND \n")));
                    try!(print_to_console(String::from("\ntype command:")));
                }
        };
        // Clears the buffer for another command once current command is finished executing
        console_input_string.clear();
         
        *text_offset  = y * 80 + x;
        *cursor_pos = y * 80 + x;
        *max_left_pos =  y * 80 + x;
        }
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
            Keycode::Left|Keycode::Right|Keycode::Up|Keycode::Down => {
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
        }*/
    */

    match keyevent.keycode.to_ascii(keyevent.modifiers) {
        Some(c) => { 
            // we echo key presses directly to the console without queuing an event
            // trace!("  {}  ", c);
            try!(CONSOLE_VGA_BUFFER.lock().write_string_with_color(&c.to_string(), ColorCode::default())
                .map_err(|_| "fmt::Error in VgaBuffer's write_string_with_color()")
            );
            
            // debug!("update column {:?}", CONSOLE_VGA_BUFFER.lock().column);
            // debug!("update row {:?}", CONSOLE_VGA_BUFFER.lock().lines.len());
            if keyevent.keycode == Keycode::Backspace {
                if *cursor_pos == *text_offset {*text_offset -= 1;}
                *cursor_pos -= 1;
            } else if keyevent.keycode != Keycode::Enter {
                if *cursor_pos == *text_offset {*text_offset += 1;}
                *cursor_pos += 1;
            }
        }
        // _ => { println!("Couldn't get ascii for keyevent {:?}", keyevent); } 
        _ => { } 
        

    }

    // let new_x = CONSOLE_VGA_BUFFER.lock().column as u16;
    // let new_y = CONSOLE_VGA_BUFFER.lock().display_line as u16 ;
    // vga_buffer::update_cursor(new_x, new_y);

    Ok(())
}


fn match_command(console_input_string: &mut String) -> Result<String, &'static str> {
    
    let mut words: Vec<&str> = console_input_string.split_whitespace().collect();
    let command_string = words.remove(0);
    // Uncomment once we have functionality to pass arguments to command
    // Structure command functions so that they propogate errors back up call stack if the argument/parameter is invalid
    // let arguments = &words;
    // Find a way to implement a structure that holds all the valid commands here
    let valid_command = COMMAND_TABLE.get(&command_string.to_string()).clone();
    // Checks if the command string returns Some or None value
    if valid_command.is_some() {
        return Ok(command_string.to_string());
    }
    else {
        return Err("invalid command");
    }
}

// struct CommandStruct<'lifetime> {
//     // function_pointer: 'lifetime fn(Vec<&str>) -> String,
//     command_str = String,
//     arguments: Vec<&'lifetime str>
// }

fn run_command_new_thread(command_string: String, current_task_id: &mut usize) -> Result<(),&'static str> {
    // let command_function_pointer: fn = COMMAND_TABLE.get(&command_string.to_string()).clone();
    let thread_execution = try!(spawn::spawn_kthread(run_command, command_string, 
    String::from("executing command on new thread"), None)
    );
    *current_task_id = thread_execution.read().id;
    Ok(())
}

fn run_command(command_string: String) {

    let fn_pointer: fn() -> String = *COMMAND_TABLE.get(&command_string.to_string()).clone().unwrap();
    print_to_console(String::from("executing command...\n")).unwrap();
    // Calls the function
    let result = fn_pointer();
    print_to_console(String::from(result)).unwrap();
}


fn build_command_table() {
    // Builds command table by mapping command string to its function pointer
    // Registers date command
    let f: fn() -> String = coreutils::get_date;
    COMMAND_TABLE.insert(String::from("date"), f);

    // Registers test command
    let f: fn() -> String = coreutils::test;
    COMMAND_TABLE.insert(String::from("test"), f);
}

pub fn add_command(command_string: String, func: fn() -> String) {
    COMMAND_TABLE.insert(String::from(command_string),func);
}

// this doesn't line up as shown here because of the escaped backslashes,
// but it lines up properly when printed :)
const WELCOME_STRING: &'static str = "\n\n
 _____ _                              
|_   _| |__   ___  ___  ___ _   _ ___ 
  | | | '_ \\ / _ \\/ __|/ _ \\ | | / __|
  | | | | | |  __/\\__ \\  __/ |_| \\__ \\
  |_| |_| |_|\\___||___/\\___|\\__,_|___/ \n\n ";
