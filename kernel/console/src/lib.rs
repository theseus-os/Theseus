#![no_std]
#![feature(alloc)]
extern crate keycodes_ascii;
#[macro_use] extern crate frame_buffer_text;
extern crate spin;
extern crate dfqueue;
extern crate atomic_linked_list; 
extern crate mod_mgmt;
extern crate spawn;
extern crate task;
extern crate memory;


#[macro_use] extern crate lazy_static;
#[macro_use] extern crate alloc;
#[macro_use] extern crate log;

// temporary, should remove this once we fix crate system
extern crate console_types; 
use console_types::{ConsoleEvent, ConsoleOutputEvent};
use frame_buffer_text::{FrameTextBuffer, DisplayPosition};
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
        $crate::print_to_console_args(format_args!($($arg)*));
    });
}


lazy_static! {
    static ref RUNNING_TERMINALS: Mutex<Vec<Terminal>> = Mutex::new(Vec::new());
}

// Variables for the vga buffer cursor
const DEFAULT_X_POS: u16 = 13;
const DEFAULT_Y_POS: u16 = 12;
const FRAME_BUFFER_WIDTH: u16 = 80;
const FRAME_BUFFER_HEIGHT: u16= 24;

// Defines the max number of terminals that can be running
const MAX_TERMS: usize = 9;

#[derive(Debug)] 
// Struct contains the command string and its arguments
struct CommandStruct {
    /// String that contains the command keyword
    command_str: String,
    /// Vector of strings that contain any arguments to the command, though support for this is not fully developed yet
    arguments: Vec<String>
}

pub struct Terminal {
    /// The terminal's own vga buffer that it displays to
    frame_buffer: FrameTextBuffer,
    /// The terminal's print producer queue that it pushes output events to that will later be handled by the main loop
    term_print_producer: DFQueueProducer<ConsoleEvent>,
    /// The reference number that can be used to switch between/correctly identify the terminal object
    term_ref: usize,
    /// The string that stores the users keypresses after the prompt
    console_input_string: String,
    /// The string that stores the user's keypresses if a command is currently running
    console_buffer_string: String,
    /// Variable that stores the task id of any application manually spawned from the terminal
    current_task_id: usize,
    /// The leftmost position in the vga buffer that the cursor may travel (calculated via row * vga buffer width + column)
    max_left_pos: u16,
    /// The rightmost position in the vga buffer that the cursor may travel (calculated via row * vga buffer width + column)
    text_offset: u16, 
    /// The current position in the vga buffer (calculated via row * vga buffer width + column)
    cursor_pos: u16,
    /// The string that is prompted to the user (ex. kernel_term~$)
    prompt_string: String,
}
 
/// Terminal Structure that allows multiple terminals to be individually run
impl Terminal {
    /// Creates a new terminal object
    fn new(dfqueue_consumer: &DFQueueConsumer<ConsoleEvent>, ref_num: usize) -> Terminal {
        let prompt_string: String;
        if ref_num == 1 {
            prompt_string = "kernel:~$ ".to_string();
        } else {
            prompt_string = format!("terminal_{}:~$ ", ref_num);
        }
        // creates a new terminal object
        Terminal {
            // internal number used to track the terminal object 
            term_ref: ref_num,
            term_print_producer: dfqueue_consumer.obtain_producer(),
            frame_buffer: FrameTextBuffer::new(),
            console_input_string: String::new(),
            console_buffer_string: String::new(),
            current_task_id: 0,
            // track the cursor position and bounds 
            max_left_pos: DEFAULT_Y_POS * FRAME_BUFFER_WIDTH + DEFAULT_X_POS,
            text_offset: DEFAULT_Y_POS * FRAME_BUFFER_WIDTH + DEFAULT_X_POS, // this is rightmost position that the cursor can travel                // debug!("start here");

            cursor_pos: DEFAULT_Y_POS * FRAME_BUFFER_WIDTH + DEFAULT_X_POS,
            prompt_string: prompt_string,
        }
    }
    

    /// Print function that will put a ConsoleOutputEvent into the queue if we ever need it
    fn print_to_terminal(&mut self, s: String) -> Result<(), &'static str> {
        trace!("Wenqiu:print to terminal");
        let output_event = ConsoleEvent::OutputEvent(ConsoleOutputEvent::new(s, Some(self.term_ref)));
        self.term_print_producer.enqueue(output_event);
        trace!("Wenqiu:print to terminal");

        return Ok(());
    }

    /// Called by the main loop to handle the exiting of tasks initiated in the terminal
    fn task_handler(&mut self) -> Result<(), &'static str> {
        // Called by the main loop to handle the exit of tasks

        // task id is 0 if there are no command line tasks running
        if self.current_task_id != 0 {
            // gets the task from the current task id variable
            let result = task::get_task(self.current_task_id);
            if let Some(ref task_result)  = result {
                let mut end_task = task_result.write();
                    let exit_result = end_task.take_exit_value();
                    // match statement will see if the task has finished with an exit value yet
                    match exit_result {
                        Some(exit_val) => {
                            match exit_val {
                                Ok(exit_status) => {
                                    // here: the task ran to completion successfully, so it has an exit value.
                                    // we know the return type of this task is `isize`,
                                    // so we need to downcast it from Any to isize.
                                    let val: Option<&isize> = exit_status.downcast_ref::<isize>();
                                    warn!("task returned exit value: {:?}", val);
                                    if let Some(unwrapped_val) = val {
                                        self.print_to_terminal(format!("task returned with exit value {:?}\n", unwrapped_val))?;
                                    }
                                }
                                // If the user manually aborts the task
                                Err(task::KillReason::Requested) => {
                                    warn!("task was manually aborted");
                                    self.print_to_terminal("^C\n".to_string())?;
                                }
                                Err(kill_reason) => {
                                    // here: the task exited prematurely, e.g., it was killed for some reason.
                                    warn!("task was killed, reason: {:?}", kill_reason);
                                    self.print_to_terminal(format!("task was killed, reason: {:?}\n", kill_reason))?;
                                }
                            }
                            // Resets the current task id to be ready for the next command
                            self.current_task_id = 0;
                            let prompt_string = self.prompt_string.clone();
                            self.print_to_terminal(prompt_string)?;
 
                            if self.console_buffer_string.len() > 0 {
                                let temp = self.console_buffer_string.clone();
                                self.print_to_terminal(temp.clone())?;
                                
                                self.console_input_string = temp;
                                self.text_offset += self.console_buffer_string.len() as u16;
                                self.cursor_pos += self.console_buffer_string.len() as u16;
                                self.console_buffer_string.clear();
                                }
                        },
                        // None value indicates task has not yet finished so does nothing
                    None => {
                        },
                    }
            }   
        }
        return Ok(());
    }
    

    /// Updates the cursor to a new position 
    fn cursor_handler(&mut self) -> Result<(), &'static str> {    
        let new_x = self.frame_buffer.column as u16;
        let display_line = self.frame_buffer.display_line;
        let new_y;
        if display_line < FRAME_BUFFER_HEIGHT as usize {
            new_y = display_line as u16;
        } else {
            new_y = FRAME_BUFFER_HEIGHT;
        };
        frame_buffer_text::update_cursor(new_x, new_y);
        // Refreshes the display
        self.frame_buffer.display(DisplayPosition::Same);
        return Ok(());
    }

    /// Called whenever the main loop consumes an input event off the DFQueue to handle a key event
    pub fn handle_key_event(&mut self, keyevent: KeyEvent, current_terminal_num: &mut usize, num_running: usize) -> Result<(), &'static str> {
        // Finds current coordinates of the VGA buffer
        let y = self.frame_buffer.display_line as u16;
        let x = self.frame_buffer.column as u16;

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
                if let Some(curr_task) = task_ref {
                    let _result = curr_task.write().kill(task::KillReason::Requested);
                }
            } else {
                self.console_input_string.clear();
                self.console_buffer_string.clear();
                let _result = self.frame_buffer.write_string_with_color(&"^C\n".to_string(), frame_buffer_text::FONT_COLOR);
                let prompt_string = self.prompt_string.clone();
                self.print_to_terminal(prompt_string)?;
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
            let selected_num;
            match keyevent.keycode.to_ascii(keyevent.modifiers) {
                Some(key) => {
                    match key.to_digit(10) {
                        Some(digit) => {
                            selected_num = digit;
                        },
                        None => {
                            return Ok(());
                        }
                    }
                },
                None => {
                    return Ok(());
                },
            }
            // Prevents user from switching to terminal tab that doesn't yet exist
            if selected_num > num_running as u32 {
                return Ok(());
            } else {
                *current_terminal_num = selected_num as usize;
                return Ok(());
            }
        }


        // EVERYTHING BELOW HERE WILL ONLY OCCUR ON A KEY PRESS (not key release)
        if keyevent.action != KeyAction::Pressed {
            return Ok(()); 
        }  

        // Allows user to create a new terminal tab
        if keyevent.modifiers.control && keyevent.keycode == Keycode::T {
            if num_running < MAX_TERMS {
                *current_terminal_num = 0;
            }
            return Ok(());
        }

        // Tracks what the user has typed so far, excluding any keypresses by the backspace and Enter key, which are special and are handled directly below
        if keyevent.keycode != Keycode::Enter && keyevent.keycode.to_ascii(keyevent.modifiers).is_some()
            && keyevent.keycode != Keycode::Backspace && keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
                if self.text_offset == self.cursor_pos {
                    if keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
                        match keyevent.keycode.to_ascii(keyevent.modifiers) {
                            Some(string) => {
                                if self.current_task_id != 0 {
                                    self.console_buffer_string.push(string);
                                    return Ok(());
                                } else {
                                    self.console_input_string.push(string);
                                }
                            },
                            None => {
                                return Err("Couldn't get key event");
                            }
                        }

                    }
                } else {
                    // controls cursor movement and associated variables if the cursor is not at the end of the current line
                    let insert_idx: usize = self.cursor_pos as usize - self.max_left_pos as usize;
                    self.console_input_string.remove(insert_idx); // Take this out once you can dynamically shift buffer 
                    match keyevent.keycode.to_ascii(keyevent.modifiers) {
                        Some(string) => {
                            self.console_input_string.push(string);
                        },
                        None => {
                            return Err("Couldn't get key event");
                        }
                    }
                }
        }

        // Tracks what the user does whenever she presses the backspace button
        if keyevent.keycode == Keycode::Backspace  {
            // Prevents user from moving cursor to the left of the typing bounds
            if self.console_input_string.len() == 0 { 
                return Ok(());
            } else {
                // Subtraction by accounts for 0-indexing
                let remove_idx: usize =  self.cursor_pos as usize  - self.max_left_pos as usize -1;
                self.console_input_string.remove(remove_idx);
                if self.cursor_pos < self.text_offset {self.console_input_string.insert(remove_idx, ' ')};
            }
        }

        // Attempts to run the command whenever the user presses enter and updates the cursor tracking variables 
        if keyevent.keycode == Keycode::Enter && keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
            // Does nothing if the user presses enter without any command
            if self.console_input_string.len() == 0 {
                return Ok(());
            } else if self.current_task_id != 0 { // prevents the user from trying to execute a new command while one is currently running
                self.print_to_terminal("Wait until the current command is finished executing\n".to_string())?;
            } else {
                // Calls the parse_input function to see if the command exists in the command table and obtains a command struct
                let console_input_string = self.console_input_string.clone();
                let command_structure = self.parse_input(&console_input_string);
                let prompt_string = self.prompt_string.clone();
                match self.run_command_new_thread(command_structure) {
                        Ok(new_task_id) => { 
                            self.current_task_id = new_task_id;
                        } Err("Error: no module with this name found!") => {
                            self.print_to_terminal(format!("{}: command not found\n\n{}",console_input_string, prompt_string))?;
                        } Err(&_) => {
                            self.print_to_terminal(format!("running command on new thread failed\n\n{}", prompt_string))?;
                        }
                }
            };
            // Clears the buffer for another command once current command is finished executing
            self.console_input_string.clear();
            // Updates the cursor tracking variables when the enter key is pressed 
            self.text_offset  = y * FRAME_BUFFER_WIDTH + x;
            self.cursor_pos = y * FRAME_BUFFER_WIDTH + x;
            self.max_left_pos =  y * FRAME_BUFFER_WIDTH + x;

        }

        // home, end, page up, page down, up arrow, down arrow for the console
        if keyevent.keycode == Keycode::Home {
            // Home command only registers if the vga buffer has the ability to scroll
            if self.frame_buffer.can_scroll(){
                self.frame_buffer.display(DisplayPosition::Start);
                self.frame_buffer.disable_cursor();
            }
            return Ok(());
        }
        if keyevent.keycode == Keycode::End {
            self.frame_buffer.display(DisplayPosition::End);
            self.frame_buffer.enable_cursor();
            return Ok(());
        }
        if keyevent.keycode == Keycode::PageUp {
            // only registers the page up command if the vga buffer can already scroll
            if self.frame_buffer.can_scroll(){
               self.frame_buffer.display(DisplayPosition::Up(20));
               self.frame_buffer.disable_cursor();
            }
            return Ok(());
        }
        if keyevent.keycode == Keycode::PageDown {
            self.frame_buffer.display(DisplayPosition::Down(20));
            self.frame_buffer.enable_cursor();
            return Ok(());
        }
        if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Up {
            self.frame_buffer.display(DisplayPosition::Up(1));
            return Ok(());
        }
        if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Down {
            self.frame_buffer.display(DisplayPosition::Down(1));
            return Ok(());
        }

        // Adjusts the cursor tracking variables when the user presses the left and right arrow keys
        if keyevent.keycode == Keycode::Left {
            if self.cursor_pos > self.max_left_pos {
                self.frame_buffer.column -= 1;
                self.cursor_pos -=1;
                return Ok(());
            }
        }
        if keyevent.keycode == Keycode::Right {
            if self.cursor_pos < self.text_offset {
                self.frame_buffer.column += 1;
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
                try!(self.frame_buffer.write_string_with_color(&c.to_string(), frame_buffer_text::FONT_COLOR)
                    .map_err(|_| "fmt::Error in FrameBuffer's write_string_with_color()")
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
    
    /// This function parses the string that the user inputted when Enter is pressed and populates the CommandStruct
    fn parse_input(&self, console_input_string: &String) -> CommandStruct {
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


    /// Function will execute the command on a new thread 
    fn run_command_new_thread(&mut self, command_structure: CommandStruct) -> Result<usize, &'static str> {
        use memory; 
        let module = memory::get_module(&command_structure.command_str).ok_or("Error: no module with this name found!")?;
        let args = command_structure.arguments; 
        let taskref = spawn::spawn_application(module, args, None, None)?;
        // Gets the task id so we can reference this task if we need to kill it with Ctrl+C
        let new_task_id = taskref.read().id;
        return Ok(new_task_id);
        
    }
}


/// Queues up the given string to be printed out to the default kernel terminal.
/// If no terminals have been initialized yet, it prints to the VGA buffer directly using `print_raw!()`.
pub fn print_to_console<S: Into<String>>(s: S) -> Result<(), &'static str> {
    if let Some(kernel_term) = RUNNING_TERMINALS.lock().get_mut(0) {
        // temporary hack to print the text output to the default kernel terminal; replace once we abstract standard streams
        let output_event = ConsoleEvent::OutputEvent(ConsoleOutputEvent::new(s.into(), Some(1)));
        kernel_term.term_print_producer.enqueue(output_event);
        Ok(())
    }
    else {
        //print_raw!("[RAW] {}", s.into());
        Ok(())
    }
}

use core::fmt;
/// Converts the given `core::fmt::Arguments` to a `String` and queues it up to be printed out to the console.
/// If the console hasn't yet been initialized, it prints to the VGA buffer directly using `print_raw!()`.
pub fn print_to_console_args(fmt_args: fmt::Arguments) {
    let _result = print_to_console(format!("{}", fmt_args));
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
    kernel_term.print_to_terminal(WELCOME_STRING.to_string())?; 
    let prompt_string = kernel_term.prompt_string.clone();
    kernel_term.print_to_terminal(format!("Console says hello!\nPress Ctrl+C to quit a task\nKernel Terminal\n{}", prompt_string))?;
    kernel_term.frame_buffer.enable_cursor();
    kernel_term.frame_buffer.update_cursor(DEFAULT_X_POS,DEFAULT_Y_POS);
    // Adds this default kernel terminal to the static list of running terminals
    // Note that the list owns all the terminals that are spawned
    RUNNING_TERMINALS.lock().push(kernel_term);
    // Spawns the infinite loop to run the terminals
    spawn::spawn_kthread(input_event_loop, console_consumer, "main input event handling loop".to_string(), None)?;
    Ok(returned_producer)
}

/// Main infinite loop that handles DFQueue input and output events
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

        trace!("Wenqiu:consumer.peek");
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
                        try!(focus_terminal.frame_buffer.write_string_with_color(&output_event.text, frame_buffer_text::FONT_COLOR)
                        .map_err(|_| "fmt::Error in FrameTextBuffer's write_string_with_color()"));
                        break;
                    }
                }                
            }
        }

        if current_terminal_num  == 0 {
            // Creates a new terminal object whenever handle keyevent sets the current_terminal_num to 0
            current_terminal_num = num_running + 1;
            let mut new_term_obj = Terminal::new(&consumer, current_terminal_num.clone());
            let prompt_string = new_term_obj.prompt_string.clone();
            let ref_num = new_term_obj.term_ref;
            new_term_obj.print_to_terminal(WELCOME_STRING.to_string())?;
            new_term_obj.print_to_terminal(format!("Console says hello!\nPress Ctrl+C to quit a task\nTerminal_{}\n{}", ref_num, prompt_string))?;  
            new_term_obj.frame_buffer.enable_cursor();
            new_term_obj.frame_buffer.update_cursor(DEFAULT_X_POS,DEFAULT_Y_POS);
            // List now owns the terminal object
            RUNNING_TERMINALS.lock().push(new_term_obj);
        }
        event.mark_completed();
    }
}


const WELCOME_STRING: &'static str = "\n\n
 _____ _                              
|_   _| |__   ___  ___  ___ _   _ ___ 
  | | | '_ \\ / _ \\/ __|/ _ \\ | | / __|
  | | | | | |  __/\\__ \\  __/ |_| \\__ \\
  |_| |_| |_|\\___||___/\\___|\\__,_|___/ \n\n";
