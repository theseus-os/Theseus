#![no_std]
#![feature(alloc)]
// used by the text display

extern crate frame_buffer;
extern crate keycodes_ascii;
extern crate spin;
extern crate dfqueue;
extern crate mod_mgmt;
extern crate spawn;
extern crate task;
extern crate memory;
extern crate input_event_types; 
extern crate window_manager;
extern crate text_display;

extern crate terminal_print;
extern crate print;

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;

use input_event_types::{Event};
use keycodes_ascii::{Keycode, KeyAction, KeyEvent};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use window_manager::displayable::text_display::TextDisplay;
use task::TaskRef;

pub const FONT_COLOR:u32 = 0x93ee90;
pub const BACKGROUND_COLOR:u32 = 0x000000;

#[no_mangle]
/// A main fucntion that calls terminal::new() and waits for the terminal loop to exit before returning an exit value
pub fn main(args: Vec<String>) -> isize {
    let num = match args.get(0) {
        Some(num_as_str) => { num_as_str.parse::<usize>() }
        None => return -1
    };
    let num = match num {
        Ok(num) => num,
        Err(_) => return -1
    };

   let term_task_ref =  match Terminal::new(num) {
        Ok(task_ref) => {task_ref}
        Err(err) => {
            error!("{}", err);
            error!("could not create terminal instance");
            return -1;
        }
    };
    // waits for the terminal loop to exit before exiting the main function
    match spawn::join(&term_task_ref) {
        Ok(_) => { }
        Err(err) => {error!("{}", err)}
    }
    return 0;
}


pub struct Terminal {
    /// The terminal's own window
    window: window_manager::WindowObj,
    /// The reference number that can be used to switch between/correctly identify the terminal object
    term_ref: usize,
    /// The string that stores the users keypresses after the prompt
    input_string: String,
    /// Indicates whether the prompt string + any additional keypresses are the last thing that is printed on the prompt
    /// If this is false, the terminal will reprint out the prompt + the additional keypresses 
    correct_prompt_position: bool,
    /// Vector that stores the history of commands that the user has entered
    command_history: Vec<String>,
    /// Variable used to track the net number of times the user has pressed up/down to cycle through the commands
    /// ex. if the user has pressed up twice and down once, then command shift = # ups - # downs = 1 (cannot be negative)
    history_index: usize,
    /// The string that stores the user's keypresses if a command is currently running
    buffer_string: String,
    /// Variable that stores the task id of any application manually spawned from the terminal
    current_task_id: usize,
    /// The string that is prompted to the user (ex. kernel_term~$)
    prompt_string: String,
    /// The input_event_manager's standard output buffer to store what the terminal instance and its child processes output
    stdout_buffer: String,
    /// The input_event_manager's standard input buffer to store what the user inputs into the terminal application
    stdin_buffer: String,
    /// The input_event_manager's standard error buffer to store any errors logged by the program
    stderr_buffer: String,
    /// The terminal's scrollback buffer which stores a string to be displayed by the text display
    scrollback_buffer: String,
    /// Indicates whether the text display is displaying the last part of the scrollback buffer slice
    is_scroll_end: bool,
    /// The starting index of the scrollback buffer string slice that is currently being displayed on the text display
    scroll_start_idx: usize,
    /// Indicates the rightmost position of the cursor ON THE text display, NOT IN THE SCROLLBACK BUFFER (i.e. one more than the position of the last non_whitespace character
    /// being displayed on the text display)
    absolute_cursor_pos: usize,
    /// Variable that tracks how far left the cursor is from the maximum rightmost position (above)
    /// absolute_cursor_pos - left shift will be the position on the text display where the cursor will be displayed
    left_shift: usize,
    /// The consumer to the terminal's print dfqueue
    print_consumer: DFQueueConsumer<Event>,
    /// The producer to the terminal's print dfqueue
    print_producer: DFQueueProducer<Event>,
}

/// Manual implementation of debug just prints out the terminal reference number
use core::fmt;
impl fmt::Debug for Terminal {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Point {{ terminal reference number: {} }}", self.term_ref)
    }
}


/// Terminal Structure that allows multiple terminals to be individually run.
/// There are now two queues that belong to each termianl instance.
/// 1) The terminal print queue that handles printing from external applications
///     - Consumer is the main terminal loop
///     - Producers are any external application trying to print to the terminal's stdout
/// 
/// 2) The terminal input queue that handles input events from the input event handling crate
///     - Consumer is the main terminal loop
///     - Producers are functions in the event handling cra te that send 
///         Keyevents if the terminal is the one currently being focused on
impl Terminal {
    /// text display: T => any concrete type that implements the TextDisplay trait (i.e. Vga buffer, etc.)
    /// ref num: usize => unique integer number to the terminal that corresponds to its tab number
    pub fn new(ref_num: usize) -> Result<TaskRef, &'static str> {
        // initialize another dfqueue for the terminal object to handle printing from applications
        let terminal_print_dfq: DFQueue<Event>  = DFQueue::new();
        let terminal_print_consumer = terminal_print_dfq.into_consumer();
        let terminal_print_producer = terminal_print_consumer.obtain_producer();
        let terminal_print_producer_copy = terminal_print_producer.obtain_producer();
        // Sets up the kernel to terminal-application printing
        print::DEFAULT_TERMINAL_QUEUE.call_once(|| terminal_print_producer_copy); 
        // Requests a new window object from the window manager
        let window_object = match window_manager::request_new_window() {
            Ok(window_object) => window_object,
            Err(err) => {debug!("new window returned err"); return Err(err)}
        };
        let prompt_string: String;
        prompt_string = "terminal:~$ ".to_string(); // ref numbers are 0-indexed
        // creates a new terminal object
        let mut terminal = Terminal {
            // internal number used to track the terminal object 
            term_ref: ref_num,
            window: window_object,
            input_string: String::new(),
            correct_prompt_position: true,
            command_history: Vec::new(),
            history_index: 0,
            buffer_string: String::new(),
            current_task_id: 0,              
            prompt_string: prompt_string,
            stdout_buffer: String::new(),
            stdin_buffer: String::new(),
            stderr_buffer: String::new(),
            scrollback_buffer: String::new(),
            scroll_start_idx: 0,
            is_scroll_end: true,
            absolute_cursor_pos: 0, 
            left_shift: 0,
            print_consumer: terminal_print_consumer,
            print_producer: terminal_print_producer,
        };
        
        // Inserts a producer for the print queue into global list of terminal print producers
        let prompt_string = terminal.prompt_string.clone();
        if ref_num == 0 {
            terminal.print_to_terminal(format!("input_event_manager says once!\nPress Ctrl+C to quit a task\nKernel Terminal\n{}", prompt_string))?;
        } else {
            terminal.print_to_terminal(format!("input_event_manager says once!\nPress Ctrl+C to quit a task\n{}", prompt_string))?;
        }
        terminal.absolute_cursor_pos = terminal.scrollback_buffer.len();
        let task_ref = spawn::spawn_kthread(terminal_loop, terminal, "terminal loop".to_string(), None)?;
        Ok(task_ref)
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

    fn get_displayable_dimensions(&self, name:&String) -> (usize, usize){
        let display_opt = self.window.get_displayable(name);
        if display_opt.is_none() {
            (0, 0)
        } else {        
            display_opt.unwrap().get_dimensions()  
        }
    }

    /// This function takes in the end index of some index in the scrollback buffer and calculates the starting index of the
    /// scrollback buffer so that a slice containing the starting and ending index would perfectly fit inside the dimensions of 
    /// text display. 
    /// If the text display's first line will display a continuation of a syntactical line in the scrollback buffer, this function 
    /// calculates the starting index so that when displayed on the text display, it preserves that line so that it looks the same
    /// as if the whole physical line is displayed on the buffer
    fn calc_start_idx(&mut self, end_idx: usize, display_name:&String) -> (usize, usize) {
        let (buffer_width, buffer_height) = self.get_displayable_dimensions(display_name);
        let mut start_idx = end_idx;
        let result;
        // Grabs a max-size slice of the scrollback buffer (usually does not totally fit because of newlines)
        if end_idx > buffer_width * buffer_height {
            result = self.scrollback_buffer.get(end_idx - buffer_width*buffer_height..end_idx);
        } else {
            result = self.scrollback_buffer.get(0..end_idx);
        }

        if let Some(slice) = result {
            let mut total_lines = 0;
            // Obtains a vector of indices of newlines in the slice in the REVERSE order that they occur
            let new_line_indices: Vec<(usize, &str)> = slice.rmatch_indices('\n').collect();
            let mut counter = 0;
            let mut last_line_chars = 0;
            // Case where the last newline does not occur at the end of the slice
            if new_line_indices[0].0 != slice.len() - 1 {
                start_idx -= slice.len() -1 - new_line_indices[0].0;
                total_lines += (slice.len()-1 - new_line_indices[0].0)/buffer_width + 1;
                last_line_chars = (slice.len() -1 - new_line_indices[0].0) % buffer_width // fix: account for more than one line
            }

            // Loops until the string slice bounded by the start and end indices is at most one newline away from fitting on the text display
            while total_lines < buffer_height {
                // Operation finds the number of lines that a single "sentence" will occupy on the text display through the operation length_of_sentence/window_width + 1
                if counter == new_line_indices.len() -1 {
                    return (0, total_lines * buffer_width + last_line_chars); // In  the case that an end index argument corresponded to a string slice that underfits the text display
                }
                // finds  the number of characters between newlines and thereby the number of lines those will take up
                let num_chars = new_line_indices[counter].0 - new_line_indices[counter+1].0;
                let num_lines = if (num_chars-1)%buffer_width != 0 || (num_chars -1) == 0 {(num_chars-1) / buffer_width + 1 } else {(num_chars-1)/buffer_width}; // using (num_chars -1) because that's the number of characters that actually show up on the screen
                if num_chars > start_idx { // prevents subtraction overflow
                    return (0, total_lines * buffer_width + last_line_chars);
                }
                start_idx -= num_chars;
                total_lines += num_lines;
                counter += 1;
            }

            // If the previous loop overcounted, this cuts off the excess string from string. Happens when there are many charcters between newlines at the beginning of the slice
            if total_lines > buffer_height {
                start_idx += (total_lines - buffer_height) * buffer_width;
                total_lines = buffer_height;
            }
            return (start_idx, (total_lines - 1) * buffer_width + last_line_chars);

        } else {
            return (0,0);
        }         
    }

   /// This function takes in the start index of some index in the scrollback buffer and calculates the end index of the
    /// scrollback buffer so that a slice containing the starting and ending index would perfectly fit inside the dimensions of 
    /// text display. 
    fn calc_end_idx(&mut self, start_idx: usize, display_name:&String) -> usize {
        let (buffer_width,buffer_height) = self.get_displayable_dimensions(display_name);
        let scrollback_buffer_len = self.scrollback_buffer.len();
        let mut end_idx = start_idx;
        let result;
        // Grabs a max-size slice of the scrollback buffer (usually does not totally fit because of newlines)
        if start_idx + buffer_width * buffer_height > scrollback_buffer_len {
            result = self.scrollback_buffer.get(start_idx..scrollback_buffer_len-1);
        } else {
            result = self.scrollback_buffer.get(start_idx..start_idx + buffer_width * buffer_height);
        }
        // calculate the starting index for the slice
        if let Some(slice) = result {
            let mut total_lines = 0;
            // Obtains a vector of the indices of the slice where newlines occur in ascending order
            let new_line_indices: Vec<(usize, &str)> = slice.match_indices('\n').collect();
            let mut counter = 0;
            // Covers the case where the start idx argument corresponds to a string that does not start on a newline 
            if new_line_indices[0].0 != 0 {
                end_idx += new_line_indices[0].0;
                total_lines += new_line_indices[0].0/buffer_width + 1;
            }

            // Calculates the end index so that the string slice between the start and end index will fit into the text display within at most 
            // one newline
            while total_lines < buffer_height {
                if counter+1 == new_line_indices.len() {
                    return self.scrollback_buffer.len()-1;
                }
                let num_chars = new_line_indices[counter+1].0 - new_line_indices[counter].0;
                let num_lines = num_chars/buffer_width + 1;
                end_idx += num_chars;
                total_lines += num_lines;
                counter += 1;
            }

            // If the last line is longer than the buffer width,
            // we simply subtract off the line from the end_idx and add the buffer width to the end idx
            if total_lines > buffer_height {
                let num_chars = new_line_indices[counter].0 - new_line_indices[counter -1].0;
                end_idx -= num_chars;
                end_idx += buffer_width;
            }
                return end_idx;
        } else {
            return self.scrollback_buffer.len()-1; 
        }
    }

    /// Scrolls up by the text display equivalent of one line
    fn scroll_up_one_line(&mut self, display_name:&String) {
        let buffer_width = self.get_displayable_dimensions(display_name).0;
        let mut start_idx = self.scroll_start_idx;
        //indicates that the user has scrolled to the top of the page
        if start_idx < 1 {
            return; 
        } else {
            start_idx -= 1;
        }
        let new_start_idx;
        let result;
        let slice_len;
        if buffer_width < start_idx {
            result = self.scrollback_buffer.as_str().get(start_idx - buffer_width .. start_idx);
            slice_len = buffer_width;
        } else {
            result = self.scrollback_buffer.as_str().get(0 .. start_idx);
            slice_len = start_idx;
        }
        // Searches this slice for a newline
        if let Some(slice) = result {
            let index = slice.rfind('\n');   
            new_start_idx = match index {
                Some(index) => { start_idx - slice_len + index }, // Moves the starting index back to the position of the nearest newline back
                None => { start_idx - slice_len}, // If no newline is found, moves the start index back by the buffer width value
            }; 
        } else {
            return
        }   
        self.scroll_start_idx = new_start_idx;
        // Recalculates the end index after the new starting index is found
        self.is_scroll_end = false;
    }

    /// Scrolls down the text display equivalent of one line
    fn scroll_down_one_line(&mut self, display_name:&String) {
        let buffer_width = self.get_displayable_dimensions(display_name).0;
        let prev_start_idx;
        // Prevents the user from scrolling down if already at the bottom of the page
        if self.is_scroll_end == true {
            return;
        } else {
            prev_start_idx = self.scroll_start_idx;
        }
        let mut end_idx = self.calc_end_idx(prev_start_idx, display_name);
        // If the newly calculated end index is the bottom of the scrollback buffer, recalculates the start index and returns
        if end_idx == self.scrollback_buffer.len() -1 {
            self.is_scroll_end = true;
            let new_start = self.calc_start_idx(end_idx, display_name).0;
            self.scroll_start_idx = new_start;
            return;
        }
        end_idx += 1; // Advances to the next character for the calculation
        let new_end_idx;
        {
            let result;
            let slice_len; // specifies the length of the grabbed slice
            // Grabs a slice (the size of the buffer width at most) of the scrollback buffer that is directly below the current slice being displayed on the text display
            if self.scrollback_buffer.len() > end_idx + buffer_width {
                slice_len = buffer_width;
                result = self.scrollback_buffer.as_str().get(end_idx .. end_idx + buffer_width);
            } else {
                slice_len = self.scrollback_buffer.len() - end_idx -1; 
                result = self.scrollback_buffer.as_str().get(end_idx .. self.scrollback_buffer.len());
            }
            // Searches the grabbed slice for a newline
            if let Some(slice) = result {
                let index = slice.find('\n');   
                new_end_idx = match index {
                    Some(index) => { end_idx + index + 1}, // Moves end index forward to the next newline
                    None => { end_idx + slice_len}, // If no newline is found, moves the end index forward by the buffer width value
                }; 
            } else {
                return;
            }
        }
        // Recalculates new starting index
        let start_idx = self.calc_start_idx(new_end_idx, display_name).0;
        self.scroll_start_idx = start_idx;
    }
    
    /// Shifts the text display up by making the previous first line the last line displayed on the text display
    fn page_up(&mut self, display_name:&String) {
        let new_end_idx = self.scroll_start_idx;
        let new_start_idx = self.calc_start_idx(new_end_idx, display_name);
        self.scroll_start_idx = new_start_idx.0;
    }

    /// Shifts the text display down by making the previous last line the first line displayed on the text display
    fn page_down(&mut self, display_name:&String) {
        let start_idx = self.scroll_start_idx;
        let new_start_idx = self.calc_end_idx(start_idx, display_name);
        let new_end_idx = self.calc_end_idx(new_start_idx, display_name);
        if new_end_idx == self.scrollback_buffer.len() -1 {
            // if the user page downs near the bottom of the page so only gets a partial shift
            self.is_scroll_end = true;
            return;
        }
        self.scroll_start_idx = new_start_idx;
    }

    /// Updates the text display by taking a string index and displaying as much as it starting from the passed string index (i.e. starts from the top of the display and goes down)
    fn update_display_forwards(&mut self, display_name:&String, start_idx: usize) -> Result<(), &'static str> {
        let end_idx = self.calc_end_idx(start_idx, display_name); 
        self.scroll_start_idx = start_idx;
        let result  = self.scrollback_buffer.get(start_idx..=end_idx);
        if let Some(slice) = result {
            let display_opt = self.window.get_displayable(display_name);
            if display_opt.is_none() {
                return Err("fail to the the text display component");
            } else {
                let (x, y) = self.window.get_content_position();
                let text_display = display_opt.unwrap();
                text_display.display_string(x, y, slice, FONT_COLOR, BACKGROUND_COLOR)?;
            }

        } else {
            return Err("could not get slice of scrollback buffer string");
        }
        Ok(())
    }


    /// Updates the text display by taking a string index and displaying as much as it can going backwards from the passed string index (i.e. starts from the bottom of the display and goes up)
    fn update_display_backwards(&mut self, display_name:&String, end_idx: usize) -> Result<(), &'static str> {
        let (start_idx, cursor_pos) = self.calc_start_idx(end_idx, display_name);
        self.scroll_start_idx = start_idx;

        let result = self.scrollback_buffer.get(start_idx..end_idx);

        if let Some(slice) = result {
            let display_opt = self.window.get_displayable(display_name);
            if display_opt.is_none() {
                return Err("fail to the the text display component");
            } else {
                let (x, y) = self.window.get_content_position();
                let text_display = display_opt.unwrap();
                text_display.display_string(x, y, slice, FONT_COLOR, BACKGROUND_COLOR)?;
                self.absolute_cursor_pos = cursor_pos;
            }
        } else {
            return Err("could not get slice of scrollback buffer string");
        }
        Ok(())
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
                                    if let Some(val) = val {
                                        self.print_to_terminal(format!("task returned with exit value {:?}\n", val))?;
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
                            // Removes the task_id from the task_map
                            terminal_print::remove_child(self.current_task_id)?;
                            // Resets the current task id to be ready for the next command
                            self.current_task_id = 0;
                            let prompt_string = self.prompt_string.clone();
                            self.print_to_terminal(prompt_string)?;

                            // Pushes the keypresses onto the input_event_manager that were tracked whenever another command was running
                            if self.buffer_string.len() > 0 {
                                let temp = self.buffer_string.clone();
                                self.print_to_terminal(temp.clone())?;
                                
                                self.input_string = temp;
                                self.buffer_string.clear();
                            }
                            // Resets the bool to true once the print prompt has been redisplayed
                            self.correct_prompt_position = true;
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
    fn cursor_handler(&mut self, display_name:&String) -> Result<(), &'static str> { 
        let buffer_width = self.get_displayable_dimensions(display_name).0;
        let mut new_x = self.absolute_cursor_pos %buffer_width;
        let mut new_y = self.absolute_cursor_pos /buffer_width;
        // adjusts to the correct position relative to the max rightmost absolute cursor position
        if new_x >= self.left_shift  {
            new_x -= self.left_shift;
        } else {
            new_x = buffer_width  + new_x - self.left_shift;
            new_y -=1;
        }

        self.window.set_cursor(new_y as u16, new_x as u16, true);
        return Ok(());
    }

    /// Called whenever the main loop consumes an input event off the DFQueue to handle a key event
    pub fn handle_key_event(&mut self, keyevent: KeyEvent, display_name:&String) -> Result<(), &'static str> {
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
                self.input_string.clear();
                self.buffer_string.clear();
                self.print_to_terminal("^C\n".to_string())?;
                let prompt_string = self.prompt_string.clone();
                self.print_to_terminal(prompt_string)?;
                self.correct_prompt_position = true;
            }
            return Ok(());
        }


        // Tracks what the user does whenever she presses the backspace button
        if keyevent.keycode == Keycode::Backspace  {
            // Prevents user from moving cursor to the left of the typing bounds
            if self.input_string.len() == 0 || self.input_string.len() - self.left_shift == 0 { 
                return Ok(());
            } else {
                // Subtraction by accounts for 0-indexing
                self.window.disable_cursor();
                let remove_idx: usize =  self.input_string.len() - self.left_shift -1;
                self.input_string.remove(remove_idx);
            }
        }

        // Attempts to run the command whenever the user presses enter and updates the cursor tracking variables 
        if keyevent.keycode == Keycode::Enter && keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
            // Does nothing if the user presses enter without any command
            if self.input_string.len() == 0 {
                return Ok(());
            } else if self.current_task_id != 0 { // prevents the user from trying to execute a new command while one is currently running
                self.print_to_terminal("Wait until the current command is finished executing\n".to_string())?;
            } else {
                // Calls the parse_input function to see if the command exists in the command table and obtains a command struct
                let input_string = self.input_string.clone();
                let command_structure = self.parse_input(&input_string);
                let prompt_string = self.prompt_string.clone();
                let current_input = self.input_string.clone();
                self.command_history.push(current_input);
                self.command_history.dedup(); // Removes any duplicates
                self.history_index = 0;
                match self.run_command_new_thread(command_structure) {
                    Ok(new_task_id) => { 
                        self.current_task_id = new_task_id;
                        terminal_print::add_child(self.current_task_id, self.print_producer.obtain_producer())?;
                    } Err("Error: no module with this name found!") => {
                        self.print_to_terminal(format!("\n{}: command not found\n\n{}",input_string, prompt_string))?;
                        self.input_string.clear();
                        self.left_shift = 0;
                        self.correct_prompt_position = true;
                        return Ok(());
                    } Err(&_) => {
                        self.print_to_terminal(format!("\nrunning command on new thread failed\n\n{}", prompt_string))?;
                        self.input_string.clear();
                        self.left_shift = 0;
                        self.correct_prompt_position = true;
                        return Ok(())
                    }
                }
            };
            // Clears the buffer for another command once current command is finished executing
            self.input_string.clear();
            self.left_shift = 0;
        }

        // home, end, page up, page down, up arrow, down arrow for the input_event_manager
        if keyevent.keycode == Keycode::Home && keyevent.modifiers.control {
            // Home command only registers if the text display has the ability to scroll
            if self.scroll_start_idx != 0 {
                self.is_scroll_end = false;
                self.scroll_start_idx = 0;
                self.window.disable_cursor();
            }
            return Ok(());
        }
        if keyevent.keycode == Keycode::End && keyevent.modifiers.control{
            if !self.is_scroll_end {
                self.is_scroll_end = true;
                let buffer_len = self.scrollback_buffer.len();
                self.scroll_start_idx = self.calc_start_idx(buffer_len, display_name).0;
            }
            return Ok(());
        }
        if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Up  {
            if self.scroll_start_idx != 0 {
                self.scroll_up_one_line(display_name);
                self.window.disable_cursor();                
            }
            return Ok(());
        }
        if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Down  {
            if !self.is_scroll_end {
                self.scroll_down_one_line(display_name);
            }
            return Ok(());
        }

        if keyevent.keycode == Keycode::PageUp && keyevent.modifiers.shift {
            if self.scroll_start_idx <= 1 {
                return Ok(())
            }
            self.page_up(display_name);
            self.is_scroll_end = false;
            self.window.disable_cursor();
            return Ok(());
        }

        if keyevent.keycode == Keycode::PageDown && keyevent.modifiers.shift {
            if self.is_scroll_end {
                return Ok(());
            }
            self.page_down(display_name);
            return Ok(());
        }

        // Cycles to the next previous command
        if  keyevent.keycode == Keycode::Up {
            if self.history_index == self.command_history.len() {
                return Ok(());
            }
            if !self.correct_prompt_position {
                let prompt_string = self.prompt_string.clone();
                self.print_to_terminal(prompt_string)?;
                self.correct_prompt_position  = true;
            }
            self.left_shift = 0;
            let previous_input = self.input_string.clone();
            for _i in 0..previous_input.len() {
                self.pop_from_stdin();
            }
            if self.history_index == 0 && self.input_string.len() != 0 {
                self.command_history.push(previous_input);
                self.history_index += 1;
            } 
            self.history_index += 1;
            let selected_command = self.command_history[self.command_history.len() - self.history_index].clone();
            let selected_command2 = selected_command.clone();
            self.input_string = selected_command;
            self.push_to_stdin(selected_command2);
            self.correct_prompt_position = true;
            return Ok(());
        }
        // Cycles to the next most recent command
        if keyevent.keycode == Keycode::Down {
            if self.history_index <= 1 {
                return Ok(());
            }
            self.left_shift = 0;
            let previous_input = self.input_string.clone();
            for _i in 0..previous_input.len() {
                self.pop_from_stdin();
            }
            self.history_index -=1;
            if self.history_index == 0 {return Ok(())}
            let selected_command = self.command_history[self.command_history.len() - self.history_index].clone();
            let selected_command2 = selected_command.clone();
            self.input_string = selected_command;
            self.push_to_stdin(selected_command2);
            self.correct_prompt_position = true;
            return Ok(());
        }

        // Jumps to the beginning of the input string
        if keyevent.keycode == Keycode::Home {
            self.left_shift = self.input_string.len();
            return Ok(());
        }

        // Jumps to the end of the input string
        if keyevent.keycode == Keycode::End {
            self.left_shift = 0;
        }   

        // Adjusts the cursor tracking variables when the user presses the left and right arrow keys
        if keyevent.keycode == Keycode::Left {
            if self.left_shift < self.input_string.len() {
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

        // Tracks what the user has typed so far, excluding any keypresses by the backspace and Enter key, which are special and are handled directly below
        if keyevent.keycode != Keycode::Enter && keyevent.keycode.to_ascii(keyevent.modifiers).is_some()
            && keyevent.keycode != Keycode::Backspace && keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
                if self.left_shift == 0 {
                    if keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
                        match keyevent.keycode.to_ascii(keyevent.modifiers) {
                            Some(c) => {
                                // Appends to the temporary buffer string if the user types while a command is running
                                if self.current_task_id != 0 {
                                    self.buffer_string.push(c);
                                    return Ok(());
                                } else {
                                    self.input_string.push(c);
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
                            let insert_idx: usize = self.input_string.len() - self.left_shift;
                            self.input_string.insert(insert_idx, c);
                        },
                        None => {
                            return Err("Couldn't get key event");
                        }
                    }
                }

                // If the prompt and any keypresses aren't already the last things being displayed on the buffer, it reprints
                if !self.correct_prompt_position{
                    let prompt_string = self.prompt_string.clone();
                    let mut input_string = self.input_string.clone();
                    let _result = input_string.pop();
                    self.print_to_terminal(prompt_string)?;
                    self.print_to_terminal(input_string)?;
                    self.correct_prompt_position = true;
                }
        }
        
        // Pushes regular keypresses (ie ascii characters and non-meta characters) into the standard-in buffer
        match keyevent.keycode.to_ascii(keyevent.modifiers) {
            Some(c) => { 
                // If the keypress is Enter
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
    
    /// Parses the string that the user inputted when Enter is pressed into the form of command (String) + arguments (Vec<String>)
    fn parse_input(&self, input_string: &String) -> (String, Vec<String>) {
        let mut words: Vec<String> = input_string.split_whitespace().map(|s| s.to_string()).collect();
        // This will never panic because pressing the enter key does not register if she has not entered anything
        let mut command_string = words.remove(0);
        // Formats the string into the application module syntax
        command_string.insert(0, '_');
        command_string.insert(0, 'a');
        command_string.insert(0, '_');
        command_string.insert(0, '_');
        return (command_string.to_string(), words);
    }


    /// Execute the command on a new thread 
    fn run_command_new_thread(&mut self, (command_string, arguments): (String, Vec<String>)) -> Result<usize, &'static str> {
        let module = memory::get_module(&command_string).ok_or("Error: no module with this name found!")?;
        let taskref = spawn::spawn_application(module, arguments, None, None)?;
        // Gets the task id so we can reference this task if we need to kill it with Ctrl+C
        let new_task_id = taskref.read().id;
        return Ok(new_task_id);
        
    }
    
    fn refresh_display(&mut self, display_name:&String) {
        let start_idx = self.scroll_start_idx;
        if self.is_scroll_end {
            let buffer_len = self.scrollback_buffer.len();
            let _result = self.update_display_backwards(display_name, buffer_len);
            let _result = self.cursor_handler(display_name);
        } else {
            let _result = self.update_display_forwards(display_name, start_idx);
        }
    }
}

/// This function is called for each terminal instance and handles all input and output events
/// of that terminal instance. There are two queues being handled in this loop.
/// 
/// 1) The print queue handles print events from applications. The producer to this queue
/// is any EXTERNAL application that prints to the terminal (any printing from within this crate
/// is simply pushed to the scrollback buffer using the associated print_to_terminal method)
/// 
/// 2) The input queue handles any input events from the input event handling crate (currently still named
/// the input_event_manager crate but will change soon)
/// 
/// The print queue is handled first inside the loop iteration, which means that all print events in the print
/// queue will always be printed to the text display before input events or any other managerial functions are handled. 
/// This allows for clean appending to the scrollback buffer and prevents interleaving of text
fn terminal_loop(mut terminal: Terminal) -> Result<(), &'static str> {
    let display_name = String::from("content");
    { 
        let rs = terminal.window.add_displayable(String::clone(&display_name), 
            TextDisplay::new(100, 50, 400, 300));
        if rs.is_err(){
            //handle error
        }
    }

    // Refreshes the text display with the default terminal upon boot, will fix once we refactor the terminal as an application
    if terminal.term_ref == 0 {
        terminal.update_display_forwards(&display_name, 0)?; // displays forward from the starting index of the scrollback buffer
        terminal.cursor_handler(&display_name)?;        
    }

    use core::ops::Deref;
    let mut refresh_display = true;

    loop {
        // Handles events from the print queue. The queue is "empty" is peek() returns None
        // If it is empty, it passes over this conditional
        if let Some(print_event) = terminal.print_consumer.peek() {
            match print_event.deref() {
                &Event::OutputEvent(ref s) => {
                    terminal.push_to_stdout(s.text.clone());

                    // Sets this bool to true so that on the next iteration the TextDisplay will refresh AFTER the 
                    // task_handler() function has cleaned up, which does its own printing to the console
                    refresh_display = true;
                    terminal.refresh_display(&display_name);
                    terminal.correct_prompt_position = false;
                },
                _ => { },
            }
            print_event.mark_completed();
            // Goes to the next iteration of the loop after processing print event to ensure that printing is handled before keypresses
            continue;
        }


        // Handles the cleanup of any application task that has finished running
        terminal.task_handler()?;
        if refresh_display == true {
            terminal.refresh_display(&display_name);
            refresh_display = false;
        }
        // Looks at the input queue. 
        // If it has unhandled items, it handles them with the match
        // If it is empty, it proceeds directly to the next loop iteration
        let event = match terminal.window.get_key_event() {
                Some(ev) => {
                    ev
                },
                _ => { continue; }
        };

        match event {
            // Used by the windowing manager to indicate to the termianl to refresh its display upon terminal resizing
            Event::DisplayEvent => {
                terminal.refresh_display(&display_name);
            }
            // Returns from the loop so that the terminal object is dropped
            Event::ExitEvent => {
                trace!("exited terminal");
                return Ok(());
            }

            // Handles ordinary keypresses
            Event::InputEvent(ref input_event) => {
                terminal.handle_key_event(input_event.key_event, &display_name)?;
                if input_event.key_event.action == KeyAction::Pressed {
                    // only refreshes the display on keypresses to improve display performance 
                    terminal.refresh_display(&display_name);
                }
            }
            _ => { }
        }
        
    }  
    Ok(())
}

// const WELCOME_STRING: &'static str = "\n\n
//  _____ _                              
// |_   _| |__   ___  ___  ___ _   _ ___ 
//   | | | '_ \\ / _ \\/ __|/ _ \\ | | / __|
//   | | | | | |  __/\\__ \\  __/ |_| \\__ \\
//   |_| |_| |_|\\___||___/\\___|\\__,_|___/ \n\n";







