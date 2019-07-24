//! Terminal emulator with event-driven architecture
//! Commands that can be run are the names of the crates in the applications directory
//! 
//! The terminal is roughly designed as follows: the main function calls the init() function of the terminal. In here, 
//! the terminal instance is created (defined by the Terminal struct) and an event handling loop is spawned which can handle
//! keypresses, resize, print events, etc. 

#![no_std]
extern crate frame_buffer_alpha;
extern crate keycodes_ascii;
extern crate spin;
extern crate dfqueue;
extern crate mod_mgmt;
extern crate spawn;
extern crate task;
extern crate runqueue;
extern crate memory;
extern crate event_types; 
extern crate window_manager_alpha;
extern crate tsc;
extern crate fs_node;
extern crate path;
extern crate root;
extern crate window_components;

extern crate terminal_print;
extern crate print;
extern crate environment;

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;

use event_types::{Event};
use keycodes_ascii::{Keycode, KeyAction, KeyEvent};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::sync::Arc;
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use spawn::{ApplicationTaskBuilder, KernelTaskBuilder};
use path::Path;
use task::{TaskRef, ExitValue, KillReason};
use environment::Environment;
use spin::Mutex;
use fs_node::FileOrDir;
use tsc::{tsc_ticks, TscTicks};

pub const APPLICATIONS_NAMESPACE_PATH: &'static str = "/namespaces/default/applications";


/// A main function that calls terminal::new() and waits for the terminal loop to exit before returning an exit value
#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {

   let _task_ref =  match Terminal::new() {
        Ok(task_ref) => {task_ref}
        Err(err) => {
            error!("{}", err);
            error!("could not create terminal instance");
            return -1;
        }
    };

    loop {
        // block this task, because it never needs to actually run again
        if let Some(my_task) = task::get_my_current_task() {
            my_task.block();
        }
    }
    // TODO FIXME: once join() doesn't cause interrupts to be disabled, we can use join again instead of the above loop
    // waits for the terminal loop to exit before exiting the main function
    // match term_task_ref.join() {
    //     Ok(_) => { }
    //     Err(err) => {error!("{}", err)}
    // }
}

/// Error type for tracking different scroll errors that a terminal
/// application could encounter.
enum ScrollError {
    /// Occurs when a index-calculation returns an index that is outside of the 
    /// bounds of the scroll buffer
    OffEndBound
}

/// Errors when attempting to invoke an application from the terminal. 
enum AppErr {
    /// The command does not match the name of any existing application in the 
    /// application namespace directory. 
    NotFound(String),
    /// The terminal could not find the application namespace due to a filesystem error. 
    NamespaceErr,
    /// The terminal could not spawn a new task to run the new application.
    /// Includes the String error returned from the task spawn function.
    SpawnErr(String)
}


/// Terminal Structure that allows multiple terminals to be individually run.
/// There are now two queues that constitute the event-driven terminal architecture
/// 1) The terminal print queue that handles printing from external applications
///     - Consumer is the main terminal loop
///     - Producers are any external application trying to print to the terminal's stdout
/// 
/// 2) The input queue (in the window manager) that handles keypresses and resize events
///     - Consumer is the main terminal loop
///     - Producer is the window manager. Window manager is responsible for enqueuing keyevents into the active application
struct Terminal {
    /// The terminal's own window
    window: Arc<Mutex<window_components::WindowComponents>>,
    /// The string that stores the users keypresses after the prompt
    cmdline: String,
    /// Indicates whether the prompt string + any additional keypresses are the last thing that is printed on the prompt
    /// If this is false, the terminal will reprint out the prompt + the additional keypresses 
    correct_prompt_position: bool,
    // textare object of the terminal
    textarea: Arc<Mutex<window_components::TextArea>>,
    /// Vector that stores the history of commands that the user has entered
    command_history: Vec<String>,
    /// Variable used to track the net number of times the user has pressed up/down to cycle through the commands
    /// ex. if the user has pressed up twice and down once, then command shift = # ups - # downs = 1 (cannot be negative)
    history_index: usize,
    /// The string that stores the user's keypresses if a command is currently running
    buffer_string: String,
    /// Variable that stores the task id of any application manually spawned from the terminal
    current_task_ref: Option<TaskRef>,
    /// The string that is prompted to the user (ex. kernel_term~$)
    prompt_string: String,
    /// The input_event_manager's standard output buffer to store what the terminal instance and its child processes output
    stdout_buffer: String,
    /// The input_event_manager's standard input buffer to store what the user inputs into the terminal application
    stdin_buffer: String,
    /// The terminal's scrollback buffer which stores a string to be displayed by the text display
    scrollback_buffer: String,
    /// Indicates whether the text display is displaying the last part of the scrollback buffer slice
    is_scroll_end: bool,
    /// The starting index of the scrollback buffer string slice that is currently being displayed on the text display
    scroll_start_idx: usize,
    /// Indicates the rightmost position of the cursor ON THE text display, NOT IN THE SCROLLBACK BUFFER (i.e. one more than the position of the last non_whitespace character
    /// being displayed on the text display)
    absolute_cursor_pos: usize,
    /// The cursor object owned by the terminal. It contains the current blinking states of the cursor
    cursor: Cursor,
    /// Variable that tracks how far left the cursor is from the maximum rightmost position (above)
    /// absolute_cursor_pos - left shift will be the position on the text display where the cursor will be displayed
    left_shift: usize,
    /// The consumer to the terminal's print dfqueue
    print_consumer: DFQueueConsumer<Event>,
    /// The producer to the terminal's print dfqueue
    print_producer: DFQueueProducer<Event>,
    /// The terminal's current environment
    env: Arc<Mutex<Environment>>
}

impl Terminal {
    /// ref num: usize => unique integer number to the terminal that corresponds to its tab number
    pub fn new() -> Result<TaskRef, &'static str> {
        // initialize another dfqueue for the terminal object to handle printing from applications
        let terminal_print_dfq: DFQueue<Event>  = DFQueue::new();
        let terminal_print_consumer = terminal_print_dfq.into_consumer();
        let terminal_print_producer = terminal_print_consumer.obtain_producer();

        // Sets up the kernel to print to this terminal instance
        print::set_default_print_output(terminal_print_producer.obtain_producer()); 

        // Requests a new window object from the window manager
        let (window_width, window_height) = window_manager_alpha::get_screen_size()?;
        const WINDOW_MARGIN: usize = 20;
        let window_object = match window_components::WindowComponents::new(
            WINDOW_MARGIN as isize, WINDOW_MARGIN as isize, window_width - 2*WINDOW_MARGIN, window_height - 2*WINDOW_MARGIN
        ) {
            Ok(window_object) => window_object,
            Err(err) => {debug!("new window returned err"); return Err(err)}
        };

        let textarea_object = {
            let wincomps = window_object.lock();
            let (width_inner, height_inner) = wincomps.inner_size();
            debug!("new window done width: {}, height: {}", width_inner, height_inner);
            // next add textarea to wincomps
            const TEXTAREA_BORDER: usize = 4;
            match window_components::TextArea::new(
                wincomps.get_bias_x() + TEXTAREA_BORDER, wincomps.get_bias_y() + TEXTAREA_BORDER,
                width_inner - 2*TEXTAREA_BORDER, height_inner - 2*TEXTAREA_BORDER,
                &wincomps.winobj, None, None, Some(wincomps.get_background()), None
            ) {
                Ok(m) => m,
                Err(err) => { debug!("new textarea returned err"); return Err(err); }
            }
        };

        let root = root::get_root();
        
        let env = Environment {
            working_dir: Arc::clone(root::get_root()), 
        };

        let mut prompt_string = root.lock().get_absolute_path(); // ref numbers are 0-indexed
        prompt_string = format!("{}: ",prompt_string);
        let mut terminal = Terminal {
            window: window_object,
            cmdline: String::new(),
            textarea: textarea_object,
            correct_prompt_position: true,
            command_history: Vec::new(),
            history_index: 0,
            buffer_string: String::new(),
            current_task_ref: None,              
            prompt_string: prompt_string,
            stdout_buffer: String::new(),
            stdin_buffer: String::new(),
            scrollback_buffer: String::new(),
            scroll_start_idx: 0,
            is_scroll_end: true,
            absolute_cursor_pos: 0, 
            cursor: Cursor::new(),
            left_shift: 0,
            print_consumer: terminal_print_consumer,
            print_producer: terminal_print_producer,
            env: Arc::new(Mutex::new(env))
        };
        
        // Inserts a producer for the print queue into global list of terminal print producers
        let prompt_string = terminal.prompt_string.clone();
        terminal.print_to_terminal(format!("Theseus Terminal Emulator\nPress Ctrl+C to quit a task\n{}", prompt_string))?;
        terminal.absolute_cursor_pos = terminal.scrollback_buffer.len();
        terminal.refresh_display();
        let task_ref = KernelTaskBuilder::new(terminal_loop, terminal)
            .name("terminal_loop".to_string())
            .spawn()?;
        Ok(task_ref)
    }

    /// Printing function for use within the terminal crate
    fn print_to_terminal(&mut self, s: String) -> Result<(), &'static str> {
        self.scrollback_buffer.push_str(&s);
        Ok(())
    }

    /// Redisplays the terminal prompt (does not insert a newline before it)
    fn redisplay_prompt(&mut self) {
        let curr_env = self.env.lock();
        let mut prompt = curr_env.working_dir.lock().get_absolute_path();
        prompt = format!("{}: ",prompt);
        self.scrollback_buffer.push_str(&prompt);
    }

    /// Pushes a string to the standard out buffer and the scrollback buffer with a new line
    fn push_to_stdout(&mut self, s: String) {
        self.stdout_buffer.push_str(&s);
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
    /// Pop_left is true if the caller wants to remove the character to the left of the cursor
    /// otherwise, removes the character at the current cursor position
    fn pop_from_stdin(&mut self, pop_left: bool) {
        let mut dir = 0;
        if pop_left {
            dir = 1;
        }
        let buffer_len = self.stdin_buffer.len();
        self.stdin_buffer.remove(buffer_len - self.left_shift - dir);
        let buffer_len = self.scrollback_buffer.len();
        self.scrollback_buffer.remove(buffer_len - self.left_shift - dir);
    }

    fn get_displayable_dimensions(&self) -> (usize, usize){
        let textarea = self.textarea.lock();
        (textarea.x_cnt, textarea.y_cnt)
    }

    /// This function takes in the end index of some index in the scrollback buffer and calculates the starting index of the
    /// scrollback buffer so that a slice containing the starting and ending index would perfectly fit inside the dimensions of 
    /// text display. 
    /// If the text display's first line will display a continuation of a syntactical line in the scrollback buffer, this function 
    /// calculates the starting index so that when displayed on the text display, it preserves that line so that it looks the same
    /// as if the whole physical line is displayed on the buffer
    /// 
    /// Return: starting index of the string and the cursor position(with respect to position on the screen, not in the scrollback buffer) in that order
    fn calc_start_idx(&mut self, end_idx: usize) -> (usize, usize) {
        let (buffer_width, buffer_height) = self.get_displayable_dimensions();
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
            // if there are no new lines in the slice
            if new_line_indices.is_empty() {
                if buffer_height * buffer_width > end_idx {
                    return (0,buffer_height * buffer_width -1);
                } else {
                    start_idx -= buffer_height * buffer_width; // text with no newlines will fill the entire buffer
                    return (start_idx, buffer_height * buffer_width -1);
                }
            }

            let mut last_line_chars = 1;
            // Case where the last newline does not occur at the end of the slice
            if new_line_indices[0].0 != slice.len() - 1 {
                start_idx -= slice.len() -1 - new_line_indices[0].0;
                total_lines += (slice.len()-1 - new_line_indices[0].0)/buffer_width + 1;
                last_line_chars = (slice.len() -1 - new_line_indices[0].0) % buffer_width; // fix: account for more than one line
            }

            // covers everything *up to* the characters between the beginning of the slice and the first new line character
            for i in 0..new_line_indices.len()-1 {
                if total_lines >= buffer_height {
                    break;
                }
                let num_chars = new_line_indices[i].0 - new_line_indices[i+1].0;
                let num_lines = if (num_chars-1)%buffer_width != 0 || (num_chars -1) == 0 {
                                    (num_chars-1) / buffer_width + 1 
                                } else {
                                    (num_chars-1)/buffer_width}; // using (num_chars -1) because that's the number of characters that actually show up on the screen
                if num_chars > start_idx { // prevents subtraction overflow
                    return (0, total_lines * buffer_width + last_line_chars);
                }  
                start_idx -= num_chars;
                total_lines += num_lines;
            }

            // tracks the characters between the beginning of the slice and the first new line character
            let first_chars = new_line_indices[new_line_indices.len() -1].0;
            let first_chars_lines = first_chars/buffer_width + 1;

            // covers the case where the text inside the new_lines_indices array overflow the text buffer 
            if total_lines > buffer_height {
                start_idx += (total_lines - buffer_height) * buffer_width; // adds back the overcounted lines to the starting index
                total_lines = buffer_height;
            // covers the case where the text between the last newline and the end of the slice overflow the text buffer
            } else if first_chars_lines + total_lines > buffer_height {
                let diff = buffer_height - total_lines;
                total_lines += diff;
                start_idx -= diff * buffer_width;
            // covers the case where the text between the last newline and the end of the slice exactly fits the text buffer
            } else if first_chars_lines + total_lines == buffer_height {
                total_lines += first_chars_lines;
                start_idx -= first_chars;
            // covers the case where the slice fits within the text buffer (i.e. there is not enough output to fill the screen)
            } else {
                return (0, total_lines * buffer_width + last_line_chars); // In  the case that an end index argument corresponded to a string slice that underfits the text display
            }

            // If the previous loop overcounted, this cuts off the excess string from string. Happens when there are many charcters between newlines at the beginning of the slice
            return (start_idx, (total_lines - 1) * buffer_width + last_line_chars);

        } else {
            return (0,0);
        }   
    }

    /// This function takes in the start index of some index in the scrollback buffer and calculates the end index of the
    /// scrollback buffer so that a slice containing the starting and ending index would perfectly fit inside the dimensions of 
    /// text display. 
    fn calc_end_idx(&mut self, start_idx: usize) -> Result<usize, ScrollError> {
        let (buffer_width,buffer_height) = self.get_displayable_dimensions();
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
            // if there are no new lines in the slice
            if new_line_indices.len() == 0 {
                // indicates that the text is just one continuous string with no newlines and will therefore fill the buffer completely
                end_idx += buffer_height * buffer_width;
                if end_idx <= self.scrollback_buffer.len() -1 {
                    return Ok(end_idx); 
                } else {
                    return Err(ScrollError::OffEndBound);
                }
            }

            let mut counter = 0;
            // Covers the case where the start idx argument corresponds to a string that does not start on a newline 
            if new_line_indices[0].0 != 0 {
                end_idx += new_line_indices[0].0;
                total_lines += new_line_indices[0].0/buffer_width + 1;
            }
            // the characters between the last newline and the end of the slice
            let last_line_chars = slice.len() -1 - new_line_indices[new_line_indices.len() -1].0;  
            let num_last_lines = last_line_chars%buffer_width + 1; // +1 to account for the physical line that the last characters will take up

            for i in 0..new_line_indices.len()-1 {
                if total_lines >= buffer_height {
                    break;
                }
                let num_chars = new_line_indices[i+1].0 - new_line_indices[i].0;
                let num_lines = num_chars/buffer_width + 1;
                end_idx += num_chars;
                total_lines += num_lines;
                counter += 1;
            }
            // covers the case where the text inside the new_line_indices array overflows the text buffer capacity            
            if total_lines > buffer_height {
                let num_chars = new_line_indices[counter].0 - new_line_indices[counter -1].0;
                end_idx -= num_chars;
                end_idx += buffer_width;
            // covers the case where the characters between the last newline and the end of the slice overflow the text buffer capacity
            } else if total_lines + num_last_lines >= total_lines {
                let diff = buffer_height - total_lines;
                end_idx += diff * buffer_width;
            // covers the case where the entire slice exactly fits or is smaller than the text buffer capacity
            } else {
                end_idx += last_line_chars;
            }

            if end_idx <= self.scrollback_buffer.len() -1 {
                return Ok(end_idx); 
            } else {
                return Err(ScrollError::OffEndBound);
            }
        } else {
            return Ok(self.scrollback_buffer.len() - 1)
        }
    }

    /// Scrolls the text display up one line
    fn scroll_up_one_line(&mut self) {
        let buffer_width = self.get_displayable_dimensions().0;
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
            }; // we're moving the cursor one position to the right relative to the end of the input string
        } else {
            return;
        }
        self.scroll_start_idx = new_start_idx;
        // Recalculates the end index after the new starting index is found
        self.is_scroll_end = false;
    }

    /// Scrolls the text display down one line
    fn scroll_down_one_line(&mut self) {
        let buffer_width = self.get_displayable_dimensions().0;
        let prev_start_idx;
        // Prevents the user from scrolling down if already at the bottom of the page
        if self.is_scroll_end == true {
            return;} 
        prev_start_idx = self.scroll_start_idx;
        let result = self.calc_end_idx(prev_start_idx);
        let mut end_idx = match result {
            Ok(end_idx) => end_idx,
            Err(ScrollError::OffEndBound) => self.scrollback_buffer.len() -1,
        };

        // If the newly calculated end index is the bottom of the scrollback buffer, recalculates the start index and returns
        if end_idx == self.scrollback_buffer.len() -1 {
            self.is_scroll_end = true;
            let new_start = self.calc_start_idx(end_idx).0;
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
        let start_idx = self.calc_start_idx(new_end_idx).0;
        self.scroll_start_idx = start_idx;
    }
    
    /// Shifts the text display up by making the previous first line the last line displayed on the text display
    fn page_up(&mut self) {
        let new_end_idx = self.scroll_start_idx;
        let new_start_idx = self.calc_start_idx(new_end_idx);
        self.scroll_start_idx = new_start_idx.0;
    }

    /// Shifts the text display down by making the previous last line the first line displayed on the text display
    fn page_down(&mut self) {
        let start_idx = self.scroll_start_idx;
        let result = self.calc_end_idx(start_idx);
        let new_start_idx = match result {
            Ok(idx) => idx+ 1, 
            Err(ScrollError::OffEndBound) => {
                let scrollback_buffer_len = self.scrollback_buffer.len();
                let new_start_idx = self.calc_start_idx(scrollback_buffer_len).0;
                self.scroll_start_idx = new_start_idx;
                self.is_scroll_end = true;
                return;
            },
        };
        let result = self.calc_end_idx(new_start_idx);
        let new_end_idx = match result {
            Ok(end_idx) => end_idx,
            Err(ScrollError::OffEndBound) => {
                let scrollback_buffer_len = self.scrollback_buffer.len();
                let new_start_idx = self.calc_start_idx(scrollback_buffer_len).0;
                self.scroll_start_idx = new_start_idx;
                self.is_scroll_end = true;
                return;
            },
        };
        if new_end_idx == self.scrollback_buffer.len() -1 {
            // if the user page downs near the bottom of the page so only gets a partial shift
            self.is_scroll_end = true;
            return;
        }
        self.scroll_start_idx = new_start_idx;
    }

    /// Updates the text display by taking a string index and displaying as much as it starting from the passed string index (i.e. starts from the top of the display and goes down)
    fn update_display_forwards(&mut self, start_idx: usize) -> Result<(), &'static str> {
        self.scroll_start_idx = start_idx;
        let result= self.calc_end_idx(start_idx); 
        let end_idx = match result {
            Ok(end_idx) => end_idx,
            Err(ScrollError::OffEndBound) => {
                let new_end_idx = self.scrollback_buffer.len() -1;
                let new_start_idx = self.calc_start_idx(new_end_idx).0;
                self.scroll_start_idx = new_start_idx;
                new_end_idx
            },
        };
        let result  = self.scrollback_buffer.get(start_idx..=end_idx); // =end_idx includes the end index in the slice
        if let Some(slice) = result {
            self.textarea.lock().display_string_basic(slice)?;
        } else {
            return Err("could not get slice of scrollback buffer string");
        }
        Ok(())
    }


    /// Updates the text display by taking a string index and displaying as much as it can going backwards from the passed string index (i.e. starts from the bottom of the display and goes up)
    fn update_display_backwards(&mut self, end_idx: usize) -> Result<(), &'static str> {
        let (start_idx, cursor_pos) = self.calc_start_idx(end_idx);
        self.scroll_start_idx = start_idx;

        let result = self.scrollback_buffer.get(start_idx..end_idx);

        if let Some(slice) = result {
            self.textarea.lock().display_string_basic(slice)?;
            self.absolute_cursor_pos = cursor_pos;
        } else {
            return Err("could not get slice of scrollback buffer string");
        }
        Ok(())
    }


    /// Called by the main loop to handle the exiting of tasks initiated in the terminal
    fn task_handler(&mut self) -> Result<(), &'static str> {
        let task_ref_copy = match self.current_task_ref.clone() {
            Some(task_ref) => task_ref,
            None => { return Ok(());}
        };
        let exit_result = task_ref_copy.take_exit_value();
        // match statement will see if the task has finished with an exit value yet
        match exit_result {
            Some(exit_val) => {
                match exit_val {
                    ExitValue::Completed(exit_status) => {
                        // here: the task ran to completion successfully, so it has an exit value.
                        // we know the return type of this task is `isize`,
                        // so we need to downcast it from Any to isize.
                        let val: Option<&isize> = exit_status.downcast_ref::<isize>();
                        info!("terminal: task returned exit value: {:?}", val);
                        if let Some(val) = val {
                            if *val < 0 {
                                self.print_to_terminal(format!("task returned error value {:?}\n", val))?;
                            }
                        }
                    },

                    ExitValue::Killed(KillReason::Requested) => {
                        self.print_to_terminal("^C\n".to_string())?;
                    },
                    // If the user manually aborts the task
                    ExitValue::Killed(kill_reason) => {
                        warn!("task was killed because {:?}", kill_reason);
                        self.print_to_terminal(format!("task was killed because {:?}\n", kill_reason))?;
                    }
                }
                
                terminal_print::remove_child(task_ref_copy.lock().id)?;
                // Resets the current task id to be ready for the next command
                self.current_task_ref = None;
                self.redisplay_prompt();
                // Pushes the keypresses onto the input_event_manager that were tracked whenever another command was running
                if self.buffer_string.len() > 0 {
                    let temp = self.buffer_string.clone();
                    self.print_to_terminal(temp.clone())?;
                    self.cmdline = temp;
                    self.buffer_string.clear();
                }
                // Resets the bool to true once the print prompt has been redisplayed
                self.correct_prompt_position = true;
                self.refresh_display();
            },
        // None value indicates task has not yet finished so does nothing
        None => { },
        }
        return Ok(());
    }

    /// Called whenever the main loop consumes an input event off the DFQueue to handle a key event
    pub fn handle_key_event(&mut self, keyevent: KeyEvent) -> Result<(), &'static str> {       
        // EVERYTHING BELOW HERE WILL ONLY OCCUR ON A KEY PRESS (not key release)
        if keyevent.action != KeyAction::Pressed {
            return Ok(()); 
        }

        // Ctrl+C signals the main loop to exit the task
        if keyevent.modifiers.control && keyevent.keycode == Keycode::C {
            let task_ref_copy = match self.current_task_ref {
                Some(ref task_ref) => task_ref.clone(), 
                None => {
                    self.cmdline.clear();
                    self.buffer_string.clear();
                    self.print_to_terminal("^C\n".to_string())?;
                    self.redisplay_prompt();
                    self.correct_prompt_position = true;
                    self.left_shift = 0;
                    return Ok(());
                }
            };
            match task_ref_copy.kill(KillReason::Requested) {
                Ok(_) => {
                    if let Err(e) = runqueue::remove_task_from_all(&task_ref_copy) {
                        error!("Killed task but could not remove it from runqueue: {}", e);
                    }
                }
                Err(e) => error!("Could not kill task, error: {}", e),
            }
            return Ok(());
        }


        // Tracks what the user does whenever she presses the backspace button
        if keyevent.keycode == Keycode::Backspace  {
            // Prevents user from moving cursor to the left of the typing bounds
            if self.cmdline.len() == 0 || self.cmdline.len() - self.left_shift == 0 { 
                return Ok(());
            } else {
                // Subtraction by accounts for 0-indexing
                let remove_idx: usize =  self.cmdline.len() - self.left_shift -1;
                self.cmdline.remove(remove_idx);
                self.pop_from_stdin(true);
                return Ok(());
            }
        }

        if keyevent.keycode == Keycode::Delete {
            // if there's no characters to the right of the cursor, does nothing
            if self.cmdline.len() == 0 || self.left_shift == 0 { 
                return Ok(());
            } else {
                // Subtraction by accounts for 0-indexing
                let remove_idx: usize =  self.cmdline.len() - self.left_shift;
                // we're moving the cursor one position to the right relative to the end of the input string
                self.cmdline.remove(remove_idx);
                self.pop_from_stdin(false);
                self.left_shift -= 1; 
                return Ok(());
            }
        }

        // Attempts to run the command whenever the user presses enter and updates the cursor tracking variables 
        if keyevent.keycode == Keycode::Enter && keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
            if self.cmdline.len() == 0 {
                // reprints the prompt on the next line if the user presses enter and hasn't typed anything into the prompt
                self.print_to_terminal("\n".to_string())?;
                self.redisplay_prompt();
                return Ok(());
            } else if self.current_task_ref.is_some() { // prevents the user from trying to execute a new command while one is currently running
                self.print_to_terminal("Wait until the current command is finished executing\n".to_string())?;
            } else {
                self.command_history.push(self.cmdline.clone());
                self.command_history.dedup(); // Removes any duplicates
                self.history_index = 0;
                match self.eval_cmdline() {
                    Ok(new_task_ref) => { 
                        let task_id = {new_task_ref.lock().id};
                        self.current_task_ref = Some(new_task_ref);
                        terminal_print::add_child(task_id, self.print_producer.obtain_producer())?; // adds the terminal's print producer to the terminal print crate
                    }
                    Err(err) => {
                        let err_msg = match err {
                            AppErr::NotFound(command) => format!("\n{:?} command not found.\n", command),
                            AppErr::NamespaceErr      => format!("\nFailed to find directory of application executables.\n"),
                            AppErr::SpawnErr(e)       => format!("\nFailed to spawn new task to run command. Error: {}.\n", e),
                        };
                        self.print_to_terminal(err_msg)?;
                        self.redisplay_prompt();
                        self.cmdline.clear();
                        self.left_shift = 0;
                        self.correct_prompt_position = true;
                        return Ok(());
                    }
                }
            }
            // Clears the buffer for another command once current command is finished executing
            self.cmdline.clear();
            self.left_shift = 0;
        }

        // home, end, page up, page down, up arrow, down arrow for the input_event_manager
        if keyevent.keycode == Keycode::Home && keyevent.modifiers.control {
            // Home command only registers if the text display has the ability to scroll
            if self.scroll_start_idx != 0 {
                self.is_scroll_end = false;
                self.scroll_start_idx = 0; // takes us up to the start of the page
                self.cursor.disable();   
            }
            return Ok(());
        }
        if keyevent.keycode == Keycode::End && keyevent.modifiers.control{
            if !self.is_scroll_end {
                self.is_scroll_end = true;
                let buffer_len = self.scrollback_buffer.len();
                self.scroll_start_idx = self.calc_start_idx(buffer_len).0;
            }
            return Ok(());
        }
        if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Up  {
            if self.scroll_start_idx != 0 {
                self.scroll_up_one_line();
                self.cursor.disable();
            }
            return Ok(());
        }
        if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Down  {
            if !self.is_scroll_end {
                self.scroll_down_one_line();
            }
            return Ok(());
        }

        if keyevent.keycode == Keycode::PageUp && keyevent.modifiers.shift {
            if self.scroll_start_idx <= 1 {
                return Ok(())
            }
            self.page_up();
            self.is_scroll_end = false;
            self.cursor.disable();
            return Ok(());
        }

        if keyevent.keycode == Keycode::PageDown && keyevent.modifiers.shift {
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
            if !self.correct_prompt_position {
                self.redisplay_prompt();
                self.correct_prompt_position  = true;
            }
            self.left_shift = 0;
            let previous_input = self.cmdline.clone();
            for _i in 0..previous_input.len() {
                self.pop_from_stdin(true);
            }
            if self.history_index == 0 && self.cmdline.len() != 0 {
                self.command_history.push(previous_input);
                self.history_index += 1;
            } 
            self.history_index += 1;
            let selected_command = self.command_history[self.command_history.len() - self.history_index].clone();
            let selected_command2 = selected_command.clone();
            self.cmdline = selected_command;
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
            let previous_input = self.cmdline.clone();
            for _i in 0..previous_input.len() {
                self.pop_from_stdin(true);
            }
            self.history_index -=1;
            if self.history_index == 0 {return Ok(())}
            let selected_command = self.command_history[self.command_history.len() - self.history_index].clone();
            let selected_command2 = selected_command.clone();
            self.cmdline = selected_command;
            self.push_to_stdin(selected_command2);
            self.correct_prompt_position = true;
            return Ok(());
        }

        // Jumps to the beginning of the input string
        if keyevent.keycode == Keycode::Home {
            self.left_shift = self.cmdline.len();
            return Ok(());
        }

        // Jumps to the end of the input string
        if keyevent.keycode == Keycode::End {
            self.left_shift = 0;
        }   

        // Adjusts the cursor tracking variables when the user presses the left and right arrow keys
        if keyevent.keycode == Keycode::Left {
            if self.left_shift < self.cmdline.len() {
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
        if keyevent.keycode != Keycode::Enter && keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
                if self.left_shift == 0 {
                    if keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
                        match keyevent.keycode.to_ascii(keyevent.modifiers) {
                            Some(c) => {
                                // Appends to the temporary buffer string if the user types while a command is running
                                if self.current_task_ref.is_some() {
                                    self.buffer_string.push(c);
                                    return Ok(());
                                } else {
                                    self.cmdline.push(c);
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
                            let insert_idx: usize = self.cmdline.len() - self.left_shift;
                            self.cmdline.insert(insert_idx, c);
                        },
                        None => {
                            return Err("Couldn't get key event");
                        }
                    }
                }

                // If the prompt and any keypresses aren't already the last things being displayed on the buffer, it reprints
                if !self.correct_prompt_position{
                    let mut cmdline = self.cmdline.clone();
                    match cmdline.pop() {
                        Some(_) => { }
                        None => {return Err("couldn't pop newline from input event string")}
                    }
                    self.redisplay_prompt();
                    self.print_to_terminal(cmdline)?;
                    self.correct_prompt_position = true;
                }
        }
        
        // Pushes regular keypresses (ie ascii characters and non-meta characters) into the standard-in buffer
        match keyevent.keycode.to_ascii(keyevent.modifiers) {
            Some(c) => self.push_to_stdin(c.to_string()),
            _ => { } 
        }
        Ok(())
    }
    

    /// Execute the command on a new thread 
    fn eval_cmdline(&mut self) -> Result<TaskRef, AppErr> {
        // Parse the cmdline
        let mut args: Vec<String> = self.cmdline.split_whitespace().map(|s| s.to_string()).collect();
        let command = args.remove(0);

	    // Check that the application actually exists
        let app_path = Path::new(APPLICATIONS_NAMESPACE_PATH.to_string());
        let app_list = match app_path.get(root::get_root()) {
            Some(FileOrDir::Dir(app_dir)) => {app_dir.lock().list()},
            _ => return Err(AppErr::NamespaceErr)
        };
        let mut executable = command.clone();
        executable.push_str(".o");
        if !app_list.contains(&executable) {
            return Err(AppErr::NotFound(command));
        }

        let taskref = match ApplicationTaskBuilder::new(Path::new(command))
            .argument(args)
            .spawn() {
                Ok(taskref) => taskref, 
                Err(e) => return Err(AppErr::SpawnErr(e.to_string()))
            };
        
        taskref.set_env(Arc::clone(&self.env)); // Set environment variable of application to the same as terminal task

        // Gets the task id so we can reference this task if we need to kill it with Ctrl+C
        return Ok(taskref);
    }
    
    fn refresh_display(&mut self) {
        let start_idx = self.scroll_start_idx;
        // handling display refreshing errors here so that we don't clog the main loop of the terminal
        if self.is_scroll_end {
            let buffer_len = self.scrollback_buffer.len();
            match self.update_display_backwards(buffer_len) {
                Ok(_) => { }
                Err(err) => {error!("could not update display backwards: {}", err); return}
            }
            match self.display_cursor() {
                Ok(_) => { }
                Err(err) => {error!("could not update cursor: {}", err); return}
            }
        } else {
            match self.update_display_forwards(start_idx) {
                Ok(_) => { }
                Err(err) => {error!("could not update display forwards: {}", err); return}
            }
        }
    }

    fn display_cursor(&mut self) -> Result<(), &'static str> {
        self.cursor.blink();
        // debug!("absolute_cursor_pos: {}", self.absolute_cursor_pos);
        if self.cursor.show {
            self.textarea.lock().set_char_absolute(self.absolute_cursor_pos - self.left_shift, 221)?;
        } else {
            self.textarea.lock().set_char_absolute(self.absolute_cursor_pos - self.left_shift, ' ' as u8)?;
        }
        Ok(())
    }
}

/// This main loop is the core component of the terminal's event-driven architecture. The terminal receives events
/// from two queues
/// 
/// 1) The print queue handles print events from applications. The producer to this queue
///    is any EXTERNAL application that prints to the terminal (any printing from within the terminal
///    is simply pushed to the scrollback buffer using the associated print_to_terminal method)
/// 
/// 2) The input queue (provided by the window manager when the temrinal request a window) gives key events
///    and resize event to the application
/// 
/// The print queue is handled first inside the loop iteration, which means that all print events in the print
/// queue will always be printed to the text display before input events or any other managerial functions are handled. 
/// This allows for clean appending to the scrollback buffer and prevents interleaving of text.
/// 
fn terminal_loop(mut terminal: Terminal) -> Result<(), &'static str> {
    use core::ops::Deref;
    terminal.refresh_display();
    loop {
        // Handle cursor show
        terminal.display_cursor()?;

        // Handles events from the print queue. The queue is "empty" is peek() returns None
        // If it is empty, it passes over this conditional
        if let Some(print_event) = terminal.print_consumer.peek() {
            match print_event.deref() {
                &Event::OutputEvent(ref s) => {
                    terminal.push_to_stdout(s.text.clone());

                    // Sets this bool to true so that on the next iteration the TextDisplay will refresh AFTER the 
                    // task_handler() function has cleaned up, which does its own printing to the console
                    terminal.refresh_display();
                    terminal.correct_prompt_position = false;
                },
                _ => { },
            }
            print_event.mark_completed();
            // Goes to the next iteration of the loop after processing print event to ensure that printing is handled before keypresses
            continue;
        } 


        // Handles the cleanup of any application task that has finished running, including refreshing the display
        terminal.task_handler()?;
        if !terminal.correct_prompt_position {
            terminal.redisplay_prompt();
            terminal.correct_prompt_position = true;
        }
        
        // Looks at the input queue from the window manager
        // If it has unhandled items, it handles them with the match
        // If it is empty, it proceeds directly to the next loop iteration
        let event = {
            let mut wincomps = terminal.window.lock();
            wincomps.handle_event()?;
            let _event = match wincomps.consumer.peek() {
                Some(ev) => ev,
                _ => { continue; }
            };
            let event = _event.clone();
            _event.mark_completed();
            event
        };

        match event {
            // Returns from the main loop so that the terminal object is dropped
            Event::ExitEvent => {
                trace!("exited terminal");
                error!("method not implemented!");
                // window_manager::delete(terminal.window)?;
                return Ok(());
            }

            Event::ResizeEvent(ref _rev) => {
                terminal.refresh_display(); // application refreshes display after resize event is received
            }

            // Handles ordinary keypresses
            Event::InputEvent(ref input_event) => {
                terminal.handle_key_event(input_event.key_event)?;
                if input_event.key_event.action == KeyAction::Pressed {
                    // only refreshes the display on keypresses to improve display performance 
                    terminal.refresh_display();
                }
            }
            _ => { }
        }
        
    }  
}

pub struct Cursor {
    enabled: bool,
    freq: u64,
    time: TscTicks,
    show: bool,
}

const DEFAULT_CURSOR_FREQ: u64 = 400000000;
impl Cursor {
    /// create a new cursor struct
    pub fn new() -> Cursor {
        Cursor {
            enabled: true,
            freq: DEFAULT_CURSOR_FREQ,
            time: tsc_ticks(),
            show: true,
        }
    }

    /// reset the cursor
    pub fn reset(&mut self) {
        self.show = true;
        self.time = tsc_ticks();
    }

    /// enable a cursor
    pub fn enable(&mut self) {
        self.enabled = true;
        self.reset();
    }

    /// disable a cursor
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// change the blink state show/hidden of a cursor. The terminal calls this function in a loop
    pub fn blink(&mut self) -> bool {
        if self.enabled {
            let time = tsc_ticks();
            if let Some(duration) = time.sub(&(self.time)) {
                if let Some(ns) = duration.to_ns() {
                    if ns >= self.freq {
                        self.time = time;
                        self.show = !self.show;
                        return true;
                    }
                }
            }
        }
        false
    }

    /// check if the cursor should be displayed
    pub fn show(&self) -> bool {
        self.enabled && self.show
    }
}
