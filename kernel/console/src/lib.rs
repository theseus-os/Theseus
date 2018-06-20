#![no_std]
#![feature(alloc)]
// used by the vga buffer

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

/// Queues up the given string to be printed out to the default kernel terminal.
/// If no terminals have been initialized yet, it prints to the VGA buffer directly using `print_raw!()`.
pub fn print_to_console<S: Into<String>>(s: S) -> Result<(), &'static str> {
    if let Some(kernel_term) = RUNNING_TERMINALS.lock().get_mut(0) {
        // temporary hack to print the text output to the default kernel terminal; replace once we abstract standard streams
        kernel_term.push_to_stdout(s.into());
        Ok(())
    }
    else {
//        print_raw!("[RAW] {}", s.into());
        Ok(())
    }
}

use core::fmt;
/// Converts the given `core::fmt::Arguments` to a `String` and queues it up to be printed out to the console.
/// If the console hasn't yet been initialized, it prints to the VGA buffer directly using `print_raw!()`.
pub fn print_to_console_args(fmt_args: fmt::Arguments) {
    let _result = print_to_console(format!("{}", fmt_args));
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
    /// The reference number that can be used to switch between/correctly identify the terminal object
    term_ref: usize,
    /// The string that stores the users keypresses after the prompt
    console_input_string: String,
    /// Vector that stores the history of commands that the user has entered
    command_history: Vec<String>,
    /// Variable used to track the net number of times the user has pressed up/down to cycle through the commands
    /// ex. if the user has pressed up twice and down once, then command shift = # ups - # downs = 1 (cannot be negative)
    history_index: usize,
    /// The string that stores the user's keypresses if a command is currently running
    console_buffer_string: String,
    /// Variable that stores the task id of any application manually spawned from the terminal
    current_task_id: usize,
    /// The string that is prompted to the user (ex. kernel_term~$)
    prompt_string: String,
    /// The console's standard output buffer to store what the terminal instance and its child processes output
    stdout_buffer: String,
    /// The console's standard input buffer to store what the user inputs into the terminal application
    stdin_buffer: String,
    /// The console's standard error buffer to store any errors logged by the program
    stderr_buffer: String,
    /// The terminal's scrollback buffer which stores a string to be displayed by the VGA buffer
    scrollback_buffer: String,
    /// Indicates whether the vga buffer is displaying the last part of the scrollback buffer slice
    is_scroll_end: bool,
    /// The starting index of the scrollback buffer string slice that is currently being displayed on the vga buffer
    scroll_start_idx: usize,
    /// The ending index of the scrollback buffer string slice that is currently being displayed on the vga buffer
    scroll_end_idx: usize,
    /// Indicates the rightmost position of the cursor on the vga buffer (i.e. one more than the position of the last non_whitespace character
    /// being displayed on the vga buffer)
    /// The x and y coordinates on the vga buffer can be calculated as:
    /// x = absolute_cursor_pos % VGA BUFFER WIDTH
    /// y = asolute_cursor_pos / VGA BUFFER WIDTH
    absolute_cursor_pos: usize,
    /// Variable that tracks how far left the cursor is from the maximum rightmost position (above)
    /// absolute_cursor_pos - left shift will be the position on the vga buffer where the cursor will be displayed
    left_shift: usize,

}
 
/// Terminal Structure that allows multiple terminals to be individually run
impl Terminal {
    /// Creates a new terminal object
    fn new(ref_num: usize) -> Terminal {
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
            frame_buffer: FrameTextBuffer::new(),
            console_input_string: String::new(),
            command_history: Vec::new(),
            history_index: 0,
            console_buffer_string: String::new(),
            current_task_id: 0,              
            prompt_string: prompt_string,
            stdout_buffer: String::new(),
            stdin_buffer: String::new(),
            stderr_buffer: String::new(),
            scrollback_buffer: String::new(),
            scroll_start_idx: 0,
            scroll_end_idx: 0,
            is_scroll_end: true,
            absolute_cursor_pos: 0, 
            left_shift: 0,
        }
    }
    

    /// Printing function for use within the terminal crate
    fn print_to_terminal(&mut self, s: String) -> Result<(), &'static str> {
        self.scrollback_buffer.push_str(&s);
        Ok(())
    }

    /// Pushes a string to the standard out buffer and the scrollback buffer with a new line
    fn push_to_stdout(&mut self, s: String) {
        self.stdout_buffer.push_str(&s);
        self.scrollback_buffer.push_str(&s);
    }

    /// Pushes a string to the standard error buffer and the scrollback buffer with a new line
    fn push_to_stderr(&mut self, s: String) {
        self.stderr_buffer.push_str(&s);
        self.scrollback_buffer.push_str(&s);
    }
    /// Pushes a string to the standard in buffer and the scrollback buffer with a new line
    fn push_to_stdin(&mut self, s: String) {
        let buffer_len = self.stdin_buffer.len();
        self.stdin_buffer.insert_str(buffer_len - self.left_shift, &s);
        let buffer_len = self.scrollback_buffer.len();
        self.scrollback_buffer.insert_str(buffer_len - self.left_shift , &s);
    }

    /// Removes a character from the stdin buffer; will remove the character specified by the left shift field
    fn pop_from_stdin(&mut self) {
        let buffer_len = self.stdin_buffer.len();
        self.stdin_buffer.remove(buffer_len - self.left_shift - 1);
        let buffer_len = self.scrollback_buffer.len();
        self.scrollback_buffer.remove(buffer_len - self.left_shift -1);
    }

    /// This function takes in the end index of some index in the scrollback buffer and calculates the starting index of the
    /// scrollback buffer so that a slice containing the starting and ending index would perfectly fit inside the dimensions of 
    /// vga buffer. 
    /// If the vga buffer's first line will display a continuation of a syntactical line in the scrollback buffer, this function 
    /// calculates the starting index so that when displayed on the vga buffer, it preserves that line so that it looks the same
    /// as if the whole physical line is displayed on the buffer
    fn calc_start_idx(&mut self, end_idx: usize) -> usize{
        let (buffer_width, buffer_height) = self.frame_buffer.get_dimensions();
        let mut start_idx = end_idx;
        let result;
        if end_idx > buffer_width * buffer_height {
            result = self.scrollback_buffer.get(end_idx - buffer_width*buffer_height..end_idx);
        } else {
            result = self.scrollback_buffer.get(0..end_idx);
        }        
            // calculate the starting index for the slice
            if let Some(slice) = result {
            let mut num_lines = 0;
            let mut curr_column = 0;
            for byte in slice.bytes().rev() {
                if byte == b'\n' {
                    num_lines += 1;
                    if num_lines >= buffer_height-1 {
                        break;
                    }
                    curr_column = 0;
                } else {
                    if curr_column == 80 {
                        curr_column = 0;
                        num_lines += 1;
                        if num_lines >= buffer_height -1 {
                            break;
                        }
                    }
                    curr_column += 1;
                }
                if start_idx > 0 {
                    start_idx -=1;
                } else {
                    return 0;
                }
            }
            // for the very first line of the vga buffer, finds how many characters will fit 
            if start_idx <= 1 {
                return 0;
            }
            let mut one_line_back = start_idx - 2;
            loop {
                if self.scrollback_buffer.as_str().chars().nth(one_line_back) == Some('\n') {
                    break;
                } else {
                    one_line_back -=1;
                }
            }

            let diff = ((start_idx-2) - one_line_back)%buffer_width as usize;
            return start_idx - 1 - diff;
            } else {
                return 0;
            }
    }

   /// This function takes in the start index of some index in the scrollback buffer and calculates the end index of the
    /// scrollback buffer so that a slice containing the starting and ending index would perfectly fit inside the dimensions of 
    /// vga buffer. 
    fn calc_end_idx(&mut self, start_idx: usize) -> usize {
        let (buffer_width,buffer_height) = self.frame_buffer.get_dimensions();
        let scrollback_buffer_len = self.scrollback_buffer.len();
        let mut end_idx = start_idx + 1;
        let result;
        if start_idx + buffer_width * buffer_height > scrollback_buffer_len {
            result = self.scrollback_buffer.get(start_idx..scrollback_buffer_len-1);
        } else {
            result = self.scrollback_buffer.get(start_idx..start_idx + buffer_width * buffer_height);
        }
            // calculate the starting index for the slice
            if let Some(slice) = result {
            let mut num_lines = 0;
            let mut curr_column = 0;
            for byte in slice.bytes(){
                if byte == b'\n' {
                    num_lines += 1;
                    if num_lines >= buffer_height {
                        break;
                    }
                    curr_column = 0;
                } else {
                    if curr_column == 80 {
                        curr_column = 0;
                        num_lines += 1;
                        if num_lines >= buffer_height {
                            break;
                        }
                    }
                    curr_column += 1;
                }
                if start_idx < scrollback_buffer_len {
                    end_idx += 1;
                } else {
                    return scrollback_buffer_len
                }
            }
            return end_idx
            } else {
                return self.scrollback_buffer.len();
            }
    }

    /// Takes in a usize that corresponds to the end index of a string slice of the scrollback buffer that will be displayed on the vga buffer
    fn print_to_vga(&mut self, end_idx: usize) -> Result<(), &'static str> {
        let start_idx = self.calc_start_idx(end_idx);
        self.scroll_start_idx = start_idx;
        self.scroll_end_idx = end_idx;
        let result  = self.scrollback_buffer.get(start_idx..end_idx);
        if let Some(slice) = result {
            self.absolute_cursor_pos = self.frame_buffer.display_string(slice)?;
        } else {
            return Err("could not get slice of scrollback buffer string");
        }
        Ok(())
    }

    /// Scrolls up by the vga buffer equivalent of one line
    fn scroll_up_one_line(&mut self) {
        let prev_end_idx;
        if self.is_scroll_end == true {
            prev_end_idx = self.scrollback_buffer.len();
        } else {
            prev_end_idx = self.scroll_end_idx;
        }
        
        let mut start_idx = self.calc_start_idx(prev_end_idx);
        //indicates that the user has scrolled to the top of the page
        if start_idx < 2 {
            return; 
        } else {
            start_idx -= 2;
        }
        let mut num_chars = 0;
        loop {
            if self.scrollback_buffer.as_str().chars().nth(start_idx) == Some('\n') {
                break;
            }
            num_chars += 1;
            if num_chars > 80 {
                break;
            }
            start_idx -= 1;
        }
        self.scroll_start_idx = start_idx;
        let end_idx = self.calc_end_idx(start_idx);
        self.scroll_end_idx = end_idx;
        self.is_scroll_end = false;
    }

    /// Scrolls down the vga buffer equivalent of one line
    fn scroll_down_one_line(&mut self) {
        let prev_start_idx;
        if self.is_scroll_end == true {
            return;
        } else {
            prev_start_idx = self.scroll_start_idx;
        }
        let mut end_idx = self.calc_end_idx(prev_start_idx);
        end_idx += 2;
        let mut num_chars = 0;
        loop {
            if self.scrollback_buffer.as_str().chars().nth(end_idx) == Some('\n') {
                break;
            }
            num_chars += 1;
            if num_chars == 80 {
                break;
            }
            end_idx += 1;
            if end_idx == self.scrollback_buffer.len() {
                self.is_scroll_end = true;
                break;
            }
        }
        self.scroll_end_idx = end_idx;
        let start_idx = self.calc_start_idx(end_idx);
        self.scroll_start_idx = start_idx;
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
    

    /// Updates the cursor to a new position and refreshes display
    fn cursor_handler(&mut self) -> Result<(), &'static str> {    
        let (buffer_width, buffer_height) = self.frame_buffer.get_dimensions();
        let mut new_x = self.absolute_cursor_pos %buffer_width;
        let mut new_y = self.absolute_cursor_pos /buffer_width;
        // adjusts to the correct position relative to the max rightmost absolute cursor position
        if new_x > self.left_shift  {
            new_x -= self.left_shift;
        } else {
            new_x = buffer_width  + new_x - self.left_shift;
            new_y -=1;
        }
        frame_buffer_text::update_cursor(new_x as u16, new_y as u16);
        return Ok(());
    }

    /// Called whenever the main loop consumes an input event off the DFQueue to handle a key event
    pub fn handle_key_event(&mut self, keyevent: KeyEvent, current_terminal_num: &mut usize, num_running: usize) -> Result<(), &'static str> {
        // Finds current coordinates of the VGA buffer
        let absolute_position = self.scrollback_buffer.len() - self.scroll_start_idx ;
        let (buffer_width, buffer_height) = self.frame_buffer.get_dimensions();
        let x = absolute_position%buffer_width;
        let y = absolute_position/buffer_width;
        // Ctrl+D or Ctrl+Alt+Del kills the OS
        if keyevent.modifiers.control && keyevent.keycode == Keycode::D
        || 
                keyevent.modifiers.control && keyevent.modifiers.alt && keyevent.keycode == Keycode::Delete {
        panic!("Ctrl+D or Ctrl+Alt+Del was pressed, abruptly (not cleanly) stopping the OS!"); //FIXME do this better, by signaling the main thread
        }
        
        // EVERYTHING BELOW HERE WILL ONLY OCCUR ON A KEY PRESS (not key release)
        if keyevent.action != KeyAction::Pressed {
            return Ok(()); 
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
                self.print_to_terminal("^C\n".to_string())?;
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
                if self.left_shift == 0 {
                    if keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
                        match keyevent.keycode.to_ascii(keyevent.modifiers) {
                            Some(c) => {
                                // Appends to the temporary buffer string if the user types while a command is running
                                if self.current_task_id != 0 {
                                    self.console_buffer_string.push(c);
                                    return Ok(());
                                } else {
                                    self.console_input_string.push(c);
                                }
                            },
                            None => {
                                return Err("Couldn't get key event");
                            }
                        }

                    }
                } else {
                    // controls cursor movement and associated variables if the cursor is not at the end of the current line
                    match keyevent.keycode.to_ascii(keyevent.modifiers) {
                        Some(c) => {
                            let insert_idx: usize = self.console_input_string.len() - self.left_shift;
                            self.console_input_string.insert(insert_idx, c);
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
            if self.console_input_string.len() == 0 || self.console_input_string.len() - self.left_shift == 0 { 
                return Ok(());
            } else {
                // Subtraction by accounts for 0-indexing
                let remove_idx: usize =  self.console_input_string.len() - self.left_shift -1;
                self.console_input_string.remove(remove_idx);
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
                let console_input = self.console_input_string.clone();
                self.command_history.push(console_input);
                self.history_index = 0;
                match self.run_command_new_thread(command_structure) {
                    Ok(new_task_id) => { 
                        self.current_task_id = new_task_id;
                    } Err("Error: no module with this name found!") => {
                        self.print_to_terminal(format!("\n{}: command not found\n\n{}",console_input_string, prompt_string))?;
                        self.console_input_string.clear();
                        self.left_shift = 0;
                        return Ok(());
                    } Err(&_) => {
                        self.print_to_terminal(format!("\nrunning command on new thread failed\n\n{}", prompt_string))?;
                        self.console_input_string.clear();
                        self.left_shift = 0;
                        return Ok(())
                    }
                }
            };
            // Clears the buffer for another command once current command is finished executing
            self.console_input_string.clear();
            self.left_shift = 0;
        }

        // home, end, page up, page down, up arrow, down arrow for the console
        if keyevent.keycode == Keycode::Home {
            // Home command only registers if the vga buffer has the ability to scroll
            if self.scroll_start_idx != 0 {
                self.is_scroll_end = false;
                self.scroll_start_idx = 0;
                self.scroll_end_idx = self.calc_end_idx(0);
                self.frame_buffer.disable_cursor();
            }
            return Ok(());
        }
        if keyevent.keycode == Keycode::End {
            if !self.is_scroll_end {
                self.is_scroll_end = true;
                self.scroll_end_idx = self.scrollback_buffer.len();
                let end_idx = self.scroll_end_idx;
                self.scroll_start_idx = self.calc_start_idx(end_idx);
                self.frame_buffer.enable_cursor();
            }
            return Ok(());
        }
        if keyevent.keycode == Keycode::PageUp {
            if self.scroll_end_idx != 0 {
                self.scroll_up_one_line();
                self.frame_buffer.disable_cursor();                
            }
            return Ok(());
        }
        if keyevent.keycode == Keycode::PageDown {
            if !self.is_scroll_end {
                self.scroll_down_one_line();
                self.frame_buffer.enable_cursor();
            }
            return Ok(());
        }

        // Cycles to the next previous command
        if  keyevent.keycode == Keycode::Up {
            if self.history_index == self.command_history.len() {
                return Ok(());
            }
            self.left_shift = 0;
            let console_input = self.console_input_string.clone();
            for _i in 0..console_input.len() {
                self.pop_from_stdin();
            }
            if self.history_index == 0 && self.console_input_string.len() != 0 {
                self.command_history.push(console_input);
                self.history_index += 1;
            } 
            self.history_index += 1;
            let selected_command = self.command_history[self.command_history.len() - self.history_index].clone();
            let selected_command2 = selected_command.clone();
            self.console_input_string = selected_command;
            self.push_to_stdin(selected_command2);
            return Ok(());
        }
        // Cycles to the next most recent command
        if keyevent.keycode == Keycode::Down {
            if self.history_index <= 1 {
                return Ok(());
            }
            self.left_shift = 0;
            let console_input = self.console_input_string.clone();
            for _i in 0..console_input.len() {
                self.pop_from_stdin();
            }
            self.history_index -=1;
            if self.history_index == 0 {return Ok(())}
            let selected_command = self.command_history[self.command_history.len() - self.history_index].clone();
            let selected_command2 = selected_command.clone();
            self.console_input_string = selected_command;
            self.push_to_stdin(selected_command2);
            return Ok(());
        }

        // Adjusts the cursor tracking variables when the user presses the left and right arrow keys
        if keyevent.keycode == Keycode::Left {
            if self.left_shift < self.console_input_string.len() {
                self.left_shift += 1;
            }
                return Ok(());
            }
        
        if keyevent.keycode == Keycode::Right {
            if self.left_shift > 0 {
                self.left_shift -= 1;
                return Ok(());
            }
        }
        
        // Pushes regular keypresses (ie ascii characters and non-meta characters) into the standard-in buffer
        match keyevent.keycode.to_ascii(keyevent.modifiers) {
            Some(c) => { 
                if c == '\u{8}' {
                    self.pop_from_stdin();
                } else {
                    self.push_to_stdin(c.to_string());
                }
            }
            _ => { } 
        }
        Ok(())
    }
    
    /// Parses the string that the user inputted when Enter is pressed and populates the CommandStruct
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


    /// Execute the command on a new thread 
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


/// Initializes the console by spawning a new thread to handle all console events, and creates a new event queue. 
/// This event queue's consumer is given to that console thread, and a producer reference to that queue is returned. 
/// This allows other modules to push console events onto the queue. 
pub fn init() -> Result<DFQueueProducer<ConsoleEvent>, &'static str> {
    let console_dfq: DFQueue<ConsoleEvent> = DFQueue::new();
    let console_consumer = console_dfq.into_consumer();
    let returned_producer = console_consumer.obtain_producer();
    // Initializes the default kernel terminal
    let mut kernel_term = Terminal::new(1);
    kernel_term.print_to_terminal(WELCOME_STRING.to_string())?; 
    let prompt_string = kernel_term.prompt_string.clone();
    kernel_term.print_to_terminal(format!("Console says once!\nPress Ctrl+C to quit a task\nKernel Terminal\n{}", prompt_string))?;
    kernel_term.frame_buffer.enable_cursor();
    // Adds this default kernel terminal to the static list of running terminals
    // Note that the list owns all the terminals that are spawned
    RUNNING_TERMINALS.lock().push(kernel_term);
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

                let end_idx = term.scrollback_buffer.len();
                if term.is_scroll_end {
                    term.print_to_vga(end_idx)?;
                } else {
                    let scroll_end_idx = term.scroll_end_idx;
                    term.print_to_vga(scroll_end_idx)?;
                }
                term.cursor_handler()?;
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
            _ => { }
        }

        if current_terminal_num  == 0 {
            // Creates a new terminal object whenever handle keyevent sets the current_terminal_num to 0
            current_terminal_num = num_running + 1;
            let mut new_term_obj = Terminal::new(current_terminal_num.clone());
            let prompt_string = new_term_obj.prompt_string.clone();
            let ref_num = new_term_obj.term_ref;
            new_term_obj.print_to_terminal(WELCOME_STRING.to_string())?;
            new_term_obj.print_to_terminal(format!("Console says hello!\nPress Ctrl+C to quit a task\nTerminal_{}\n{}", ref_num, prompt_string))?;  
            new_term_obj.frame_buffer.enable_cursor();
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







