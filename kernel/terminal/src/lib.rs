#![no_std]
#![feature(alloc)]
// used by the vga buffer

extern crate keycodes_ascii;
extern crate vga_buffer;
extern crate spin;
extern crate dfqueue;
extern crate atomic_linked_list; 
extern crate mod_mgmt;
extern crate spawn;
extern crate task;
extern crate memory;
// temporary, should remove this once we fix crate system
extern crate console_types; 


#[macro_use] extern crate lazy_static;
#[macro_use] extern crate alloc;
#[macro_use] extern crate log;


use console_types::{ConsoleEvent};
use vga_buffer::{VgaBuffer};
use keycodes_ascii::{Keycode, KeyAction, KeyEvent};
use alloc::string::String;
use alloc::string::ToString;
use alloc::btree_map::BTreeMap;
use alloc::arc::Arc;
use spin::Mutex;
use alloc::vec::Vec;
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};


lazy_static! {
    // maps the terminal reference number to the current task id
    static ref TERMINAL_TASK_IDS: Arc<Mutex<BTreeMap<usize, usize>>> = Arc::new(Mutex::new(BTreeMap::new()));
    // maps the terminal's reference number to its print producer
    static ref TERMINAL_PRINT_PRODUCERS: Arc<Mutex<BTreeMap<usize, DFQueueProducer<ConsoleEvent>>>> = Arc::new(Mutex::new(BTreeMap::new()));
}
/// Currently, println! and print! macros will call this function to print to the vga buffer from the console crate. 
/// Whenever the println! macro (and thereby this funtion) is called, the task id of the application that called
/// the print function is recorded. The TERMINAL_TASK_ID map then finds which terminal instance is running that task id,
/// and then the TERMINAL_PRINT_PRODUCERS map will give the correct print producer to enqueue the print event
pub fn print_to_console<S: Into<String>>(s: S) -> Result<(), &'static str> {
    // Gets the task id of the task that called this print function
    let result = task::get_my_current_task_id();
    let mut selected_term = 1;
    if let Some(current_task_id) = result {
            
            let terminal_task_ids_lock = TERMINAL_TASK_IDS.lock();
            for term in terminal_task_ids_lock.iter() {
                // finds the corresponding terminal instance running the current task id
                if *term.1 == current_task_id {
                    let number = term.0.clone();
                    selected_term = number;
                }
            }
    }
    // Obtains the correct temrinal print producer and enqueues the print event, which will later be popped off
    // and handled by the infinite temrinal instance loop 
    let print_map = TERMINAL_PRINT_PRODUCERS.lock();
    let result = print_map.get(& selected_term);
    if let Some(selected_term_producer) = result {
        selected_term_producer.enqueue(ConsoleEvent::new_output_event(s));
    }
    Ok(())
}


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
    vga_buffer: VgaBuffer,
    /// The reference number that can be used to switch between/correctly identify the terminal object
    term_ref: usize,
    /// The string that stores the users keypresses after the prompt
    console_input_string: String,
    /// Stores the index of the last character in the console_input_string so that we can make sure
    /// that the kernel prompt + anykeypresses will be the last thing on the buffer being displayed whenever the user types 
    input_string_index: usize,
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
    absolute_cursor_pos: usize,
    /// Variable that tracks how far left the cursor is from the maximum rightmost position (above)
    /// absolute_cursor_pos - left shift will be the position on the vga buffer where the cursor will be displayed
    left_shift: usize,
    /// The consumer to the terminal's print dfqueue
    print_consumer: DFQueueConsumer<ConsoleEvent>,
    /// The consumer for the Terminal's input dfqueue. It will dequeue input events from the terminal's dfqueue and handle them using
    /// the handle keypress function. 
    input_consumer: DFQueueConsumer<ConsoleEvent>,

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
///     - Producers are functions in the event handling crate that send DisplayEvents and
///         Keyevents if the terminal is the one currently being focused on
impl Terminal {
    /// Creates a new terminal object
    pub fn init(ref_num: usize) -> Result<DFQueueProducer<ConsoleEvent>, &'static str> {
        // initialize a dfqueue for the terminal object for console input events to be fed into from the input event handling crate loop
        let terminal_input_queue: DFQueue<ConsoleEvent>  = DFQueue::new();
        let terminal_input_consumer = terminal_input_queue.into_consumer();
        let returned_input_producer = terminal_input_consumer.obtain_producer();

        // initialize another dfqueue for the terminal object to handle printing from applications
        let terminal_print_dfq: DFQueue<ConsoleEvent>  = DFQueue::new();
        let terminal_print_consumer = terminal_print_dfq.into_consumer();
        let terminal_print_producer = terminal_print_consumer.obtain_producer();

        let prompt_string: String;
        if ref_num == 1 {
            prompt_string = "kernel:~$ ".to_string();
        } else {
            prompt_string = format!("terminal_{}:~$ ", ref_num);
        }
        // creates a new terminal object
        let mut terminal = Terminal {
            // internal number used to track the terminal object 
            term_ref: ref_num,
            vga_buffer: VgaBuffer::new(),
            console_input_string: String::new(),
            input_string_index: 0,
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
            input_consumer: terminal_input_consumer,
            print_consumer: terminal_print_consumer,            
        };
        
        // Inserts a producer for the print queue into global list of terminal print producers
        {TERMINAL_PRINT_PRODUCERS.lock().insert(ref_num, terminal_print_producer);}
        terminal.print_to_terminal(WELCOME_STRING.to_string())?; 
        let prompt_string = terminal.prompt_string.clone();
        if ref_num == 1 {
            terminal.print_to_terminal(format!("Console says once!\nPress Ctrl+C to quit a task\nKernel Terminal\n{}", prompt_string))?;
        } else {
            terminal.print_to_terminal(format!("Console says once!\nPress Ctrl+C to quit a task\nTerminal {}\n{}", ref_num, prompt_string))?;
        }
        terminal.input_string_index = terminal.scrollback_buffer.len()-1; // updates the input string tracking variable
        terminal.vga_buffer.enable_cursor();
        // Spawns a terminal instance on a new thread
        spawn::spawn_kthread(terminal_loop, terminal, "terminal loop".to_string(), None)?;
        Ok(returned_input_producer)
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
        let (buffer_width, buffer_height) = self.vga_buffer.get_dimensions();
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

            // Case where the last newline does not occur at the end of the slice
            if new_line_indices[0].0 != slice.len() - 1 {
                start_idx -= slice.len() -1 - new_line_indices[0].0;
                total_lines += (slice.len()-1 - new_line_indices[0].0)/buffer_width + 1;
            }

            // Loops until the string slice bounded by the start and end indices is at most one newline away from fitting on the vga buffer
            while total_lines < buffer_height {
                // Operation finds the number of lines that a single "sentence" will occupy on the vga buffer through the operation length_of_sentence/vga_buffer_width + 1
                if counter == new_line_indices.len() -1 {
                    return 0; // In  the case that an end index argument corresponded to a string slice that underfits the vga buffer
                }
                // finds  the number of characters between newlines and thereby the number of lines those will take up
                let num_chars = new_line_indices[counter].0 - new_line_indices[counter+1].0;
                let num_lines = num_chars / buffer_width + 1; // add one because division of a/b when a<b results in 0
                if num_chars > start_idx { // prevents subtraction overflow
                    return 0;
                }
                start_idx -= num_chars;
                total_lines += num_lines;
                counter += 1;
            }
            
            // If the previous loop overcounted, this cuts off the excess string from string. Happens when there are many charcters between newlines at the beginning of the slice
            if total_lines > buffer_height {
                start_idx += (total_lines - buffer_height) * buffer_width;
            }
            return start_idx;

        } else {
            return 0;
        }
             
    }

   /// This function takes in the start index of some index in the scrollback buffer and calculates the end index of the
    /// scrollback buffer so that a slice containing the starting and ending index would perfectly fit inside the dimensions of 
    /// vga buffer. 
    fn calc_end_idx(&mut self, start_idx: usize) -> usize {
        let (buffer_width,buffer_height) = self.vga_buffer.get_dimensions();
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

                // Calculates the end index so that the string slice between the start and end index will fit into the vga buffer within at most 
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

    /// Scrolls up by the vga buffer equivalent of one line
    fn scroll_up_one_line(&mut self) {
        let buffer_width = self.vga_buffer.get_dimensions().0;
        let prev_end_idx = self.scroll_end_idx;
        let mut start_idx = self.calc_start_idx(prev_end_idx);
        //indicates that the user has scrolled to the top of the page
        if start_idx < 1 {
            return; 
        } else {
            start_idx -= 1;
        }

        let new_start_idx;
        {
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
        }

        self.scroll_start_idx = new_start_idx;
        // Recalculates the end index after the new starting index is found
        let end_idx = self.calc_end_idx(new_start_idx);
        self.scroll_end_idx = end_idx;
        self.is_scroll_end = false;
    }

    /// Scrolls down the vga buffer equivalent of one line
    fn scroll_down_one_line(&mut self) {
        let buffer_width = self.vga_buffer.get_dimensions().0;
        let prev_start_idx;
        // Prevents the user from scrolling down if already at the bottom of the page
        if self.is_scroll_end == true {
            return;
        } else {
            prev_start_idx = self.scroll_start_idx;
        }
        let mut end_idx = self.calc_end_idx(prev_start_idx);
        // If the newly calculated end index is the bottom of the scrollback buffer, recalculates the start index and returns
        if end_idx == self.scrollback_buffer.len() -1 {
            self.is_scroll_end = true;
            self.scroll_end_idx = end_idx;
            let new_start = self.calc_start_idx(end_idx);
            self.scroll_start_idx = new_start;
            return;
        }
        end_idx += 1; // Advances to the next character for the calculation
        let new_end_idx;
        {
            let result;
            let slice_len; // specifies the length of the grabbed slice
            // Grabs a slice (the size of the buffer width at most) of the scrollback buffer that is directly below the current slice being displayed on the vga buffer
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
        self.scroll_end_idx = new_end_idx;
        let start_idx = self.calc_start_idx(new_end_idx);
        self.scroll_start_idx = start_idx;
    }
    
    /// Shifts the vga buffer up by making the previous first line the last line displayed on the vga buffer
    fn page_up(&mut self) {
        let mut new_end_idx = self.scroll_start_idx;
        let new_start_idx = self.calc_start_idx(new_end_idx);
        if new_start_idx <= 1 {
            // if the user page ups near the top of the page so only gets a partial shift
            new_end_idx = self.calc_end_idx(new_start_idx);
        }
        self.scroll_end_idx = new_end_idx;
        self.scroll_start_idx = new_start_idx;
    }

    /// Shifts the vga buffer down by making the previous last line the first line displayed on the vga buffer
    fn page_down(&mut self) {
        let new_start_idx = self.scroll_end_idx;
        let new_end_idx = self.calc_end_idx(new_start_idx);
        if new_end_idx == self.scrollback_buffer.len() -1 {
            // if the user page downs near the bottom of the page so only gets a partial shift
            self.is_scroll_end = true;
            self.vga_buffer.enable_cursor();
            let _result = self.print_to_vga(new_end_idx);
            return;
        }
        self.scroll_start_idx = new_start_idx;
        self.scroll_end_idx = new_end_idx;
    }

     /// Takes in a usize that corresponds to the end index of a string slice of the scrollback buffer that will be displayed on the vga buffer
    fn print_to_vga(&mut self, end_idx: usize) -> Result<(), &'static str> {
        // Calculates a starting index that will correspond to a slice of the scrollback buffer that will perfectly fit on the vga buffer
        let start_idx = self.calc_start_idx(end_idx);
        self.scroll_start_idx = start_idx;
        self.scroll_end_idx = end_idx;
        let result  = self.scrollback_buffer.get(start_idx..end_idx);
        if let Some(slice) = result {
            self.absolute_cursor_pos = self.vga_buffer.display_string(slice)?;
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
                            // Removes the task_id from the task_map
                            TERMINAL_TASK_IDS.lock().remove(&self.term_ref);
                            // Resets the current task id to be ready for the next command
                            self.current_task_id = 0;
                            let prompt_string = self.prompt_string.clone();
                            self.print_to_terminal(prompt_string)?;

                            // Pushes the keypresses onto the console that were tracked whenever another command was running
                            if self.console_buffer_string.len() > 0 {
                                let temp = self.console_buffer_string.clone();
                                self.print_to_terminal(temp.clone())?;
                                
                                self.console_input_string = temp;
                                self.console_buffer_string.clear();
                            }
                            self.input_string_index = self.scrollback_buffer.len() -1;
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
        let buffer_width = self.vga_buffer.get_dimensions().0;
        let mut new_x = self.absolute_cursor_pos %buffer_width;
        let mut new_y = self.absolute_cursor_pos /buffer_width;
        // adjusts to the correct position relative to the max rightmost absolute cursor position
        if new_x > self.left_shift  {
            new_x -= self.left_shift;
        } else {
            new_x = buffer_width  + new_x - self.left_shift;
            new_y -=1;
        }
        vga_buffer::update_cursor(new_x as u16, new_y as u16);
        return Ok(());
    }

    /// Called whenever the main loop consumes an input event off the DFQueue to handle a key event
    pub fn handle_key_event(&mut self, keyevent: KeyEvent) -> Result<(), &'static str> {
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
                        TERMINAL_TASK_IDS.lock().insert(self.term_ref, self.current_task_id);
                    } Err("Error: no module with this name found!") => {
                        self.print_to_terminal(format!("\n{}: command not found\n\n{}",console_input_string, prompt_string))?;
                        self.console_input_string.clear();
                        self.left_shift = 0;
                        self.input_string_index = self.scrollback_buffer.len() -1;
                        return Ok(());
                    } Err(&_) => {
                        self.print_to_terminal(format!("\nrunning command on new thread failed\n\n{}", prompt_string))?;
                        self.console_input_string.clear();
                        self.left_shift = 0;
                        self.input_string_index = self.scrollback_buffer.len() - 1;
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
                self.vga_buffer.disable_cursor();
            }
            return Ok(());
        }
        if keyevent.keycode == Keycode::End {
            if !self.is_scroll_end {
                self.is_scroll_end = true;
                self.scroll_end_idx = self.scrollback_buffer.len();
                let end_idx = self.scroll_end_idx;
                self.scroll_start_idx = self.calc_start_idx(end_idx);
                self.vga_buffer.enable_cursor();
            }
            return Ok(());
        }
        if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Up  {
            if self.scroll_end_idx != 0 {
                self.scroll_up_one_line();
                self.vga_buffer.disable_cursor();                
            }
            return Ok(());
        }
        if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Down  {
            if !self.is_scroll_end {
                self.scroll_down_one_line();
            }
            if self.is_scroll_end {
                self.vga_buffer.enable_cursor();
            }
            return Ok(());
        }

        if keyevent.keycode == Keycode::PageUp {
            self.page_up();
            self.is_scroll_end = false;
            self.vga_buffer.disable_cursor();
            return Ok(());
        }

        if keyevent.keycode == Keycode::PageDown {
            if self.is_scroll_end {
                return Ok(());
            }
            self.page_down();
            return Ok(());
        }

        // Cycles to the next previous command
        if  keyevent.keycode == Keycode::Up {
            if self.history_index == self.command_history.len() {
                return Ok(());
            }
            if self.input_string_index != self.scrollback_buffer.len() -1 {
                let prompt_string = self.prompt_string.clone();
                self.print_to_terminal(prompt_string)?;
                self.input_string_index = self.scrollback_buffer.len() -1;
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
            self.input_string_index = self.scrollback_buffer.len() -1;
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

                // If the prompt and any keypresses aren't already the last things being displayed on the buffer, it reprints
                if self.input_string_index != self.scrollback_buffer.len() -1 {
                    // debug!("input string index {}, scrollback_buffer_len {}\n", self.input_string_index, self.scrollback_buffer.len());
                    let prompt_string = self.prompt_string.clone();
                    let mut console_input_string = self.console_input_string.clone();
                    let _result = console_input_string.pop();
                    self.print_to_terminal(prompt_string)?;
                    self.print_to_terminal(console_input_string)?;
                    self.input_string_index = self.scrollback_buffer.len() -1;
                }
        }
        
        // Pushes regular keypresses (ie ascii characters and non-meta characters) into the standard-in buffer
        match keyevent.keycode.to_ascii(keyevent.modifiers) {
            Some(c) => { 
                // If the keypress is Enter
                if c == '\u{8}' {
                    self.pop_from_stdin();
                    self.input_string_index -= 1;
                } else {
                    self.push_to_stdin(c.to_string());
                    self.input_string_index += 1;
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
        let module = memory::get_module(&command_structure.command_str).ok_or("Error: no module with this name found!")?;
        let args = command_structure.arguments; 
        let taskref = spawn::spawn_application(module, args, None, None)?;
        // Gets the task id so we can reference this task if we need to kill it with Ctrl+C
        let new_task_id = taskref.read().id;
        return Ok(new_task_id);
        
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
/// the console crate but will change soon). This includes keypresses and DisplayEvents that signal to the
/// terminal that it should print to the vga buffer.
/// 
/// The print queue is handled first inside the loop iteration, which means that all print events in the print
/// queue will always be printed to the vga buffer before input events or any other managerial functions are handled. 
/// This allows for clean appending to the scrollback buffer and prevents interleaving of text
fn terminal_loop(mut terminal: Terminal) -> Result<(), &'static str> {
    use core::ops::Deref;
    loop {
        // Handles events from the print queue. The queue is "empty" is peek() returns None
        // If it is empty, it passes over this conditional
        if let Some(print_event) = terminal.print_consumer.peek() {
            match print_event.deref() {
                &ConsoleEvent::OutputEvent(ref s) => {
                    terminal.push_to_stdout(s.text.clone());
                },
                _ => { },
            }
            print_event.mark_completed();

            continue;
        }

        // Handles the cleanup of any application task that has finished running
        terminal.task_handler()?;

        // Looks at the input queue. 
        // If it has unhandled items, it handles them with the match
        // If it is empty, it proceeds directly to the next loop iteration
        let event = match terminal.input_consumer.peek() {
                Some(ev) => ev,
                _ => { continue; }
        };

        match event.deref() {
            &ConsoleEvent::ExitEvent => {
                let _result = print_to_console("\nSmoothly exiting console main loop.\n".to_string())?;
                return Ok(());
            }

            // Signals to the terminal instance that it is being focused on and therefore needs
            // to print to the vga buffer
            &ConsoleEvent::DisplayEvent => {
                let end_idx = terminal.scrollback_buffer.len();
                if terminal.is_scroll_end {
                    terminal.print_to_vga(end_idx)?;
                } else {
                    let scroll_end_idx = terminal.scroll_end_idx;
                    terminal.print_to_vga(scroll_end_idx)?;
                }
                // handles cursor movement
                terminal.cursor_handler()?;
            }

            &ConsoleEvent::InputEvent(ref input_event) => {
                terminal.handle_key_event(input_event.key_event)?;
            }

            // prints output events are handled by the print queue
            _ => { }
        }
        event.mark_completed();
    }  
    Ok(())
}

const WELCOME_STRING: &'static str = "\n\n
 _____ _                              
|_   _| |__   ___  ___  ___ _   _ ___ 
  | | | '_ \\ / _ \\/ __|/ _ \\ | | / __|
  | | | | | |  __/\\__ \\  __/ |_| \\__ \\
  |_| |_| |_|\\___||___/\\___|\\__,_|___/ \n\n";







