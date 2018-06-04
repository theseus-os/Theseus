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
#[macro_use] extern crate alloc;

// temporary, should remove this once we fix crate system
extern crate console_types; 
use console_types::{ConsoleEvent, ConsoleOutputEvent};
use vga_buffer::{VgaBuffer, ColorCode, DisplayPosition};
use keycodes_ascii::{Keycode, KeyAction, KeyEvent};
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use spin::Mutex;
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

        // let my_terminal_obj = KERNEL_TERMINAL.try().unwrap();
        // my_terminal_obj.print(s)

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


lazy_static! {
    static ref RUNNING_TERMINALS: Mutex<Vec<Terminal>> = Mutex::new(Vec::new());
}

pub struct Terminal {
    vga_buffer: VgaBuffer,
    term_print_producer: DFQueueProducer<ConsoleEvent>,
    term_ref: usize,
    console_input_string: String,
    current_task_id: usize,
    max_left_pos: u16,
    text_offset: u16, 
    cursor_pos: u16,
}

impl Terminal {
    fn new(dfqueue_consumer: &DFQueueConsumer<ConsoleEvent>, ref_num: usize) -> Terminal {
        // creates a new terminal object
        // this method creates a new producer for the console's dfqueue
        let new_vga_buffer: VgaBuffer = VgaBuffer::new();
        let new_term_print_producer: DFQueueProducer<ConsoleEvent> = dfqueue_consumer.obtain_producer();
        Terminal {
            // internal number used to track the terminal object 
            term_ref: ref_num,
            vga_buffer: new_vga_buffer,
            term_print_producer: new_term_print_producer,
            console_input_string: String::new(),
            current_task_id: 0,
            // track the cursor position and bounds 
            max_left_pos: 12 * 80 + 13,
            text_offset: 12 * 80 + 13, // this is rightmost position that the cursor can travel
            cursor_pos: 12 * 80 + 13,
        }
    }
    

    fn print_to_terminal(&mut self, s: String) -> Result<(), &'static str> {
        // Called by the terminal object to print to its own vga buffer
        let output_event = ConsoleEvent::OutputEvent(ConsoleOutputEvent::new(s, Some(self.term_ref)));
        self.term_print_producer.enqueue(output_event);
        return Ok(());
    }

    fn task_handler(&mut self) -> Result<(), &'static str> {
        // Called by the main loop to handle the exit of tasks

        // task id is 0 if there are no command line tasks running
        if self.current_task_id != 0 {
            // gets the task from the current task id variable
            let result = task::get_task(self.current_task_id);
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
                                    self.print_to_terminal(format!("finished with exit value {:?}\n", boxed_val.downcast_ref::<isize>().unwrap()))?;
                                }
                            },
                            // if the user presses Ctrl+C
                            Err(task::KillReason::Requested) => {
                                try!(self.print_to_terminal(format!("\ncommand manually aborted\n")));
                            }
                            // catches any other errors
                            Err(_) => {
                                try!(self.print_to_terminal(String::from("\ntask could not be run\n")));
                            },
                        }
                        // Resets the current task id to be ready for the next command
                        self.current_task_id = 0;
                        try!(self.print_to_terminal(String::from("\ntype command:")));
                    },
                    // None value indicates task has not yet finished so does nothing
                    None => {
                    },
                }
            }   
        }
        return Ok(());
    }

    fn cursor_handler(&mut self) -> Result<(), &'static str> {    
        // Updates the cursor to a new position 
        let new_x = self.vga_buffer.column as u16;
        let display_line = self.vga_buffer.display_line;
        let new_y = if display_line < 24 {display_line as u16} else {24 as u16};
        vga_buffer::update_cursor(new_x, new_y);
        // Refreshes the display
        self.vga_buffer.display(DisplayPosition::Same);
        return Ok(());
    }

    pub fn handle_key_event(&mut self, keyevent: KeyEvent, current_terminal_num: &mut usize, num_running: usize) -> Result<(), &'static str> {
        // Finds current coordinates of the VGA buffer
        let y = self.vga_buffer.display_line as u16;
        let x = self.vga_buffer.column as u16;

        // Ctrl+D or Ctrl+Alt+Del kills the OS
        if keyevent.modifiers.control && keyevent.keycode == Keycode::D
        || 
                keyevent.modifiers.control && keyevent.modifiers.alt && keyevent.keycode == Keycode::Delete {
        panic!("Ctrl+D or Ctrl+Alt+Del was pressed, abruptly (not cleanly) stopping the OS!"); //FIXME do this better, by signaling the main thread
        }

        // Ctrl+C signals the main loop to exit the task
        if keyevent.modifiers.control && keyevent.keycode == Keycode::C {
            
            if self.current_task_id != 0 {
                let task_ref = task::get_task(self.current_task_id);
                if task_ref.is_some() {
                    let _result = task_ref.unwrap().write().kill(task::KillReason::Requested);
                }
            } 
            return Ok(());
        }

        // Allows the user to switch between terminal tabs 1-9: 1 is the default kernel terminal, and user can only create terminals using ctrl + T
        if keyevent.modifiers.control && (
            keyevent.keycode == Keycode::Num1 ||
            keyevent.keycode == Keycode::Num2 ||
            keyevent.keycode == Keycode::Num3 ||
            keyevent.keycode == Keycode::Num4 ||
            keyevent.keycode == Keycode::Num5 ||
            keyevent.keycode == Keycode::Num6 ||
            keyevent.keycode == Keycode::Num7 ||
            keyevent.keycode == Keycode::Num8 ||
            keyevent.keycode == Keycode::Num9 ) {
            let selected_num = keyevent.keycode.to_ascii(keyevent.modifiers).unwrap().to_digit(10).unwrap() as usize;
            // Prevents user from switching to terminal tab that doesn't yet exist
            if selected_num > num_running {
                return Ok(());
            } else {
                *current_terminal_num = selected_num;
                return Ok(());
            }
        }


        // EVERYTHING BELOW HERE WILL ONLY OCCUR ON A KEY PRESS (not key release)
        if keyevent.action != KeyAction::Pressed {
            return Ok(()); 
        }  

        // Allows user to create a new terminal tab
        if keyevent.modifiers.control && keyevent.keycode == Keycode::T {
            if num_running < 9 {
                *current_terminal_num = 0;
            }
            return Ok(());
        }

        // Prevents any keypress below here from registering if a command is running
        if self.current_task_id != 0 {
            return Ok(());
        }

        // Tracks what the user has typed so far, excluding any keypresses by the backspace and Enter key, which are special and are handled directly below
        if keyevent.keycode != Keycode::Enter && keyevent.keycode.to_ascii(keyevent.modifiers).is_some()
            && keyevent.keycode != Keycode::Backspace && keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
                let text_offset = self.text_offset;
                let cursor_pos = self.cursor_pos;
                if text_offset == cursor_pos {
                    if keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
                        self.console_input_string.push(keyevent.keycode.to_ascii(keyevent.modifiers).unwrap());    
                    }
                } else {
                    // controls cursor movement and associated variables if the cursor is not at the end of the current line
                    let max_left_pos = self.max_left_pos as usize;
                    let cursor_pos = self.cursor_pos as usize;
                    let insert_idx: usize = cursor_pos - max_left_pos;
                    self.console_input_string.remove(insert_idx); // Take this out once you can dynamically shift buffer 
                    if keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
                        self.console_input_string.insert(insert_idx,keyevent.keycode.to_ascii(keyevent.modifiers).unwrap()); 
                    }
                }
                // DON'T RETURN HERE
        }

        // Tracks what the user does whenever she presses the backspace button
        if keyevent.keycode == Keycode::Backspace  {
            // Prevents user from moving cursor to the left of the typing bounds
            let cursor_pos = self.cursor_pos as usize;
            let max_left_pos = self.max_left_pos as usize;
            let text_offset = self.text_offset as usize;
            if cursor_pos == max_left_pos {    
                return Ok(());
            } else {
                let remove_idx: usize =  cursor_pos  - max_left_pos -1;
                self.console_input_string.remove(remove_idx);
                if cursor_pos < text_offset {self.console_input_string.insert(remove_idx, ' ')};
                // DON'T RETURN HERE
            }
        }

        // Attempts to run the command whenever the user presses enter and updates the cursor tracking variables 
        if keyevent.keycode == Keycode::Enter && keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
            // Does nothing if the user presses enter without any command
            if self.console_input_string.len() == 0 {
                return Ok(());
            } else if self.current_task_id != 0 { // prevents the user from trying to execute a new command while one is currently running
                try!(self.print_to_terminal(String::from("Wait until the current command is finished executing\n")));        
            } else {
                // Calls the parse_input function to see if the command exists in the command table and obtains a command struct
                let command_structure = parse_input(&mut self.console_input_string);
                match run_command_new_thread(command_structure) {
                        Ok(new_task_id) => {
                            self.current_task_id = new_task_id;
                        } Err("Error: no module with this name found!") => {
                            try!(self.print_to_terminal(String::from("command not found\n")));
                            try!(self.print_to_terminal(String::from("\ntype command: ")));
                        
                        } Err(&_) => {
                            try!(self.print_to_terminal(String::from("running command on new thread failed\n")));
                            try!(self.print_to_terminal(String::from("\ntype command: ")));
                        }
                    }
            };
            // Clears the buffer for another command once current command is finished executing
            self.console_input_string.clear();
            // Updates the cursor tracking variables when the enter key is pressed 
            self.text_offset  = y * 80 + x;
            self.cursor_pos = y * 80 + x;
            self.max_left_pos =  y * 80 + x;
        }

        // home, end, page up, page down, up arrow, down arrow for the console
        if keyevent.keycode == Keycode::Home {
            self.vga_buffer.display(DisplayPosition::Start);
            return Ok(());
        }
        if keyevent.keycode == Keycode::End {
            self.vga_buffer.display(DisplayPosition::End);
            return Ok(());
        }
        if keyevent.keycode == Keycode::PageUp {
            self.vga_buffer.display(DisplayPosition::Up(20));
            return Ok(());
        }
        if keyevent.keycode == Keycode::PageDown {
            self.vga_buffer.display(DisplayPosition::Down(20));
            return Ok(());
        }
        if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Up {
            self.vga_buffer.display(DisplayPosition::Up(1));
            return Ok(());
        }
        if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Down {
            self.vga_buffer.display(DisplayPosition::Down(1));
            return Ok(());
        }

        // Adjusts the cursor tracking variables when the user presses the left and right arrow keys
        if keyevent.keycode == Keycode::Left {
            // USE THE OBJECT TRAITS INSTEAD OF DEFNINING NEW ONES
            let cursor_pos = self.cursor_pos as usize;
            let max_left_pos = self.max_left_pos as usize;
            if cursor_pos > max_left_pos {
                self.vga_buffer.column -= 1;
                self.cursor_pos -=1;
                return Ok(());
            }
        }
        if keyevent.keycode == Keycode::Right {
            // let cursor_pos = self.cursor_pos as usize;
            // let text_offset = self.text_offset as usize;
            if self.cursor_pos < self.text_offset {
                self.vga_buffer.column += 1;
                self.cursor_pos += 1;
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
                try!(self.vga_buffer.write_string_with_color(&c.to_string(), ColorCode::default())
                    .map_err(|_| "fmt::Error in VgaBuffer's write_string_with_color()")
                );
                
                // adjusts the cursor tracking variables whenever the backspace or ascii keys are pressed, excluding the enter key which is handled above
                let cursor_pos = self.cursor_pos as usize;
                    let text_offset = self.text_offset as usize;
                if keyevent.keycode == Keycode::Backspace {
                    if cursor_pos == text_offset {self.text_offset -= 1;}
                    self.cursor_pos -= 1;
                } else if keyevent.keycode != Keycode::Enter {
                    if cursor_pos == text_offset {self.text_offset += 1;}
                    self.cursor_pos += 1;
                }
            }
            _ => { } 
        }

        Ok(())
    }

}


pub fn print_to_console(s: String) -> Result<(), &'static str> {
    // temporary hack to print the text output to the default kernel terminal; replace once we abstract standard streams
    let output_event = ConsoleEvent::OutputEvent(ConsoleOutputEvent::new(s, Some(1)));
    RUNNING_TERMINALS.lock().get_mut(0).ok_or("could not acquire kernel terminal")?.term_print_producer.enqueue(output_event);
    return Ok(())
}


/// Initializes the console by spawning a new thread to handle all console events, and creates a new event queue. 
/// This event queue's consumer is given to that console thread, and a producer reference to that queue is returned. 
/// This allows other modules to push console events onto the queue. 
pub fn init() -> Result<DFQueueProducer<ConsoleEvent>, &'static str> {
    let console_dfq: DFQueue<ConsoleEvent> = DFQueue::new();
    let console_consumer = console_dfq.into_consumer();
    let returned_producer = console_consumer.obtain_producer();
    // Initializes the default kernel terminal
    let mut kernel_term = Terminal::new(&console_consumer, 1);
    try!(kernel_term.print_to_terminal(String::from(WELCOME_STRING)));
    try!(kernel_term.print_to_terminal(String::from("Console says hello!\nPress Ctrl+C to quit a task\nKernel Terminal\ntype command: ")));
    kernel_term.vga_buffer.init_cursor();
    kernel_term.vga_buffer.update_cursor(13,12);
    // Adds this default kernel terminal to the static list of running terminals
    // Note that the list owns all the terminals that are spawned
    RUNNING_TERMINALS.lock().push(kernel_term);
    // Spawns the infinite loop to run the terminals
    spawn::spawn_kthread(input_event_loop, console_consumer, "main input event handling loop".to_string(), None)?;
    Ok(returned_producer)
}


fn input_event_loop(consumer: DFQueueConsumer<ConsoleEvent>) -> Result<(), &'static str> {
    // variable to track which terminal the user is currently focused on
    // terminal objects have a field term_ref that can be used for this purpose
    let mut current_terminal_num: usize = 1;

    loop {
        use core::ops::Deref;
        let mut num_running: usize = 0;
        for term in RUNNING_TERMINALS.lock().iter_mut() {
            num_running += 1;
            let _result = term.task_handler();
            if term.term_ref == current_terminal_num {
                let _result = term.cursor_handler();
            }
        }

        let event = match consumer.peek() {
            Some(ev) => ev,
            _ => { continue; }
        };

        match event.deref() {
            &ConsoleEvent::ExitEvent => {
                let _result = print_to_console("\nSmoothly exiting console main loop.\n".to_string());
                return Ok(());
            }

            &ConsoleEvent::InputEvent(ref input_event) => {
                for term in RUNNING_TERMINALS.lock().iter_mut() {
                    if term.term_ref == current_terminal_num {
                        let focus_terminal = term;
                        try!(focus_terminal.handle_key_event(input_event.key_event, &mut current_terminal_num, num_running));
                        break;
                    }
                }
            }

            &ConsoleEvent::OutputEvent(ref output_event) => {
                for term in RUNNING_TERMINALS.lock().iter_mut() {
                    if term.term_ref == output_event.term_num.unwrap_or(1 as usize) {
                        let focus_terminal = term;
                        try!(focus_terminal.vga_buffer.write_string_with_color(&output_event.text, ColorCode::default())
                        .map_err(|_| "fmt::Error in VgaBuffer's write_string_with_color()"));
                        break;
                    }
                }                
            }
        }

        if current_terminal_num  == 0 {
            // Creates a new terminal object whenever handle keyevent sets the current_terminal_num to 0
            current_terminal_num = num_running + 1;
            let mut new_term_obj = Terminal::new(&consumer, current_terminal_num.clone());
            new_term_obj.print_to_terminal(String::from(WELCOME_STRING))?;
            new_term_obj.print_to_terminal(String::from(format!("Console says hello!\nPress Ctrl+C to quit a task\nTerminal {} \ntype command: ", current_terminal_num)))?;
            new_term_obj.vga_buffer.init_cursor();
            new_term_obj.vga_buffer.update_cursor(13,12);
            // List now owns the terminal object
            RUNNING_TERMINALS.lock().push(new_term_obj);
        }
        event.mark_completed();
    }
}


#[derive(Debug)] 
// Struct contains the command string and its arguments
struct CommandStruct {
    command_str: String,
    arguments: Vec<String>
}


fn parse_input(console_input_string: &mut String) -> CommandStruct {
    // This function parses the string that the user inputted when Enter is pressed and populates the CommandStruct
    let mut words: Vec<String> = console_input_string.split_whitespace().map(|s| s.to_string()).collect();
    // This will never panic because pressing the enter key does not register if she has not entered anything
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


fn run_command_new_thread(command_structure: CommandStruct) -> Result<usize, &'static str> {
    // Function will execute the command on a new thread 
    use memory; 
    let module = memory::get_module(&command_structure.command_str).ok_or("Error: no module with this name found!")?;
    let args = command_structure.arguments; 
    let taskref = spawn::spawn_application(module, args, None, None)?;
    // Gets the task id so we can reference this task if we need to kill it with Ctrl+C
    let new_task_id = taskref.read().id;
    return Ok(new_task_id);
    
}

const WELCOME_STRING: &'static str = "\n\n
 _____ _                              
|_   _| |__   ___  ___  ___ _   _ ___ 
  | | | '_ \\ / _ \\/ __|/ _ \\ | | / __|
  | | | | | |  __/\\__ \\  __/ |_| \\__ \\
  |_| |_| |_|\\___||___/\\___|\\__,_|___/ \n\n ";
