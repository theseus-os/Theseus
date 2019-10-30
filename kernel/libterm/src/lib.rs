//! Terminal emulator library
//!
//! The terminal roughly does the following things: manages all characters in a String that should be printed to the screen;
//! cuts a slice from this String and send it to window manager to get things actually printed; manages user input command line
//! as well as the cursor position, and delivers keyboard events.

#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate dfqueue;
extern crate window_manager;
extern crate environment;
extern crate print;
extern crate event_types;
extern crate spin;
extern crate text_display;
extern crate displayable;
extern crate frame_buffer_rgb;
extern crate frame_buffer;
extern crate font;
extern crate tsc;
extern crate frame_buffer_drawer;
extern crate frame_buffer_printer;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::boxed::Box;
use event_types::Event;
use text_display::{TextDisplay};
use displayable::Displayable;
use frame_buffer_rgb::FrameBufferRGB;
use frame_buffer::{Coord, FrameBuffer};
use window_manager::WindowGeneric;
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH};
use tsc::{tsc_ticks, TscTicks};

pub const FONT_COLOR: u32 = 0x93ee90;
pub const BACKGROUND_COLOR: u32 = 0x000000;
const DEFAULT_CURSOR_FREQ: u64 = 400000000;

/// Error type for tracking different scroll errors that a terminal
/// application could encounter.
pub enum ScrollError {
    /// Occurs when a index-calculation returns an index that is outside of the 
    /// bounds of the scroll buffer
    OffEndBound
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
pub struct Terminal {
    /// The terminal's own window
    window: WindowGeneric<FrameBufferRGB>,
    // Name of the displayable object of the terminal
    display_name: String,
    /// The terminal's scrollback buffer which stores a string to be displayed by the text display
    scrollback_buffer: String,
    /// Indicates whether the text display is displaying the last part of the scrollback buffer slice
    is_scroll_end: bool,
    /// The starting index of the scrollback buffer string slice that is currently being displayed on the text display
    scroll_start_idx: usize,
    /// The cursor of the terminal.
    pub cursor: Cursor
}

/// Privite methods of `Terminal`.
impl Terminal {
    /// Get the width and height of the text display.
    fn get_displayable_dimensions(&self, name:&str) -> (usize, usize){        
        match self.window.get_concrete_display::<TextDisplay>(&name) {
            Ok(text_display) => {
                return text_display.get_dimensions();
            },
            Err(err) => {
                debug!("get_displayable_dimensions: {}", err);
                return (0, 0);
            }
        }
    }

    /// This function takes in the end index of some index in the scrollback buffer and calculates the starting index of the
    /// scrollback buffer so that a slice containing the starting and ending index would perfectly fit inside the dimensions of 
    /// text display. 
    /// If the text display's first line will display a continuation of a syntactical line in the scrollback buffer, this function 
    /// calculates the starting index so that when displayed on the text display, it preserves that line so that it looks the same
    /// as if the whole physical line is displayed on the buffer
    /// 
    /// Return: starting index of the string and the cursor position(with respect to position on the screen, not in the scrollback buffer) in that order
    fn calc_start_idx(&self, end_idx: usize, display_name:&str) -> (usize, usize) {
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
            // if there are no new lines in the slice
            if new_line_indices.is_empty() {
                if buffer_height * buffer_width > end_idx {
                    return (0, end_idx);
                } else {
                    start_idx -= buffer_height * buffer_width; // text with no newlines will fill the entire buffer
                    return (start_idx, buffer_height * buffer_width -1);
                }
            }

            let mut last_line_chars = 0;
            // Case where the last newline does not occur at the end of the slice
            if new_line_indices[0].0 != slice.len() - 1 {
                start_idx -= slice.len() -1 - new_line_indices[0].0;
                total_lines += (slice.len()-1 - new_line_indices[0].0)/buffer_width + 1;
                last_line_chars = (slice.len() -1 - new_line_indices[0].0) % buffer_width; // fix: account for more than one line
            }
            else {
                start_idx -= 1;
                total_lines += 1;
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
            return (0,0); /* WARNING: should change to Option<> rather than returning (0, 0) */
        }   
    }

    /// This function takes in the start index of some index in the scrollback buffer and calculates the end index of the
    /// scrollback buffer so that a slice containing the starting and ending index would perfectly fit inside the dimensions of 
    /// text display. 
    fn calc_end_idx(&self, start_idx: usize, display_name:&str) -> Result<usize, ScrollError> {
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
            return Ok(self.scrollback_buffer.len() - 1) /* WARNING: maybe should return Error? */
        }
    }

    /// Scrolls the text display up one line
    fn scroll_up_one_line(&mut self) {
        let buffer_width = self.get_displayable_dimensions(&self.display_name).0;
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
        let buffer_width = self.get_displayable_dimensions(&self.display_name).0;
        let prev_start_idx;
        // Prevents the user from scrolling down if already at the bottom of the page
        if self.is_scroll_end == true {
            return;} 
        prev_start_idx = self.scroll_start_idx;
        let result = self.calc_end_idx(prev_start_idx, &self.display_name);
        let mut end_idx = match result {
            Ok(end_idx) => end_idx,
            Err(ScrollError::OffEndBound) => self.scrollback_buffer.len() -1,
        };

        // If the newly calculated end index is the bottom of the scrollback buffer, recalculates the start index and returns
        if end_idx == self.scrollback_buffer.len() -1 {
            self.is_scroll_end = true;
            let new_start = self.calc_start_idx(end_idx, &self.display_name).0;
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
        let start_idx = self.calc_start_idx(new_end_idx, &self.display_name).0;
        self.scroll_start_idx = start_idx;
    }

    /// Shifts the text display up by making the previous first line the last line displayed on the text display
    fn page_up(&mut self) {
        let new_end_idx = self.scroll_start_idx;
        let new_start_idx = self.calc_start_idx(new_end_idx, &self.display_name);
        self.scroll_start_idx = new_start_idx.0;
    }

    /// Shifts the text display down by making the previous last line the first line displayed on the text display
    fn page_down(&mut self) {
        let start_idx = self.scroll_start_idx;
        let result = self.calc_end_idx(start_idx, &self.display_name);
        let new_start_idx = match result {
            Ok(idx) => idx+ 1, 
            Err(ScrollError::OffEndBound) => {
                let scrollback_buffer_len = self.scrollback_buffer.len();
                let new_start_idx = self.calc_start_idx(scrollback_buffer_len, &self.display_name).0;
                self.scroll_start_idx = new_start_idx;
                self.is_scroll_end = true;
                return;
            },
        };
        let result = self.calc_end_idx(new_start_idx, &self.display_name);
        let new_end_idx = match result {
            Ok(end_idx) => end_idx,
            Err(ScrollError::OffEndBound) => {
                let scrollback_buffer_len = self.scrollback_buffer.len();
                let new_start_idx = self.calc_start_idx(scrollback_buffer_len, &self.display_name).0;
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
        let result= self.calc_end_idx(start_idx, &self.display_name); 
        let end_idx = match result {
            Ok(end_idx) => end_idx,
            Err(ScrollError::OffEndBound) => {
                let new_end_idx = self.scrollback_buffer.len() -1;
                let new_start_idx = self.calc_start_idx(new_end_idx, &self.display_name).0;
                self.scroll_start_idx = new_start_idx;
                new_end_idx
            },
        };
        let result  = self.scrollback_buffer.get(start_idx..=end_idx); // =end_idx includes the end index in the slice
        if let Some(slice) = result {
            {
                let text_display = self.window.get_concrete_display_mut::<TextDisplay>(&self.display_name)?;
                text_display.set_text(slice);
            }
            self.window.display(&self.display_name)?;
        } else {
            return Err("could not get slice of scrollback buffer string");
        }
        Ok(())
    }

    /// Updates the text display by taking a string index and displaying as much as it can going backwards from the passed string index (i.e. starts from the bottom of the display and goes up)
    fn update_display_backwards(&mut self, end_idx: usize) -> Result<(), &'static str> {
        let (start_idx, _cursor_pos) = self.calc_start_idx(end_idx, &self.display_name);
        self.scroll_start_idx = start_idx;

        let result = self.scrollback_buffer.get(start_idx..end_idx);

        if let Some(slice) = result {
            {
                let text_display = self.window.get_concrete_display_mut::<TextDisplay>(&self.display_name)?;
                text_display.set_text(slice);
            }
            self.window.display(&self.display_name)?;        
        } else {
            return Err("could not get slice of scrollback buffer string");
        }
        Ok(())
    }
}

/// Public methods of `Terminal`.
impl Terminal {
    pub fn new() -> Result<Terminal, &'static str> {
        // Requests a new window object from the window manager
        let window_object = match window_manager::new_default_window() {
            Ok(window_object) => window_object,
            Err(err) => {debug!("new window returned err"); return Err(err)}
        };

        // let mut prompt_string = root.lock().get_absolute_path(); // ref numbers are 0-indexed
        let mut terminal = Terminal {
            window: window_object,
            display_name: String::from("content"),
            scrollback_buffer: String::new(),
            scroll_start_idx: 0,
            is_scroll_end: true,
            cursor: Cursor::new(FONT_COLOR),
        };

        // Inserts a producer for the print queue into global list of terminal print producers
        terminal.print_to_terminal(format!("Theseus Terminal Emulator\nPress Ctrl+C to quit a task\n"));
        Ok(terminal)
    }

    /// Adds a string to be printed to the terminal to the terminal scrollback buffer.
    /// Note that one needs to call `refresh_display` to get things actually printed. 
    pub fn print_to_terminal(&mut self, s: String) {
        self.scrollback_buffer.push_str(&s);
    }

    /// Actually refresh the screen. Currently it's expensive.
    pub fn refresh_display(&mut self) {
        let start_idx = self.scroll_start_idx;
        // handling display refreshing errors here so that we don't clog the main loop of the terminal
        if self.is_scroll_end {
            let buffer_len = self.scrollback_buffer.len();
            match self.update_display_backwards(buffer_len) {
                Ok(_) => { }
                Err(err) => {error!("could not update display backwards: {}", err); return}
            }
        } else {
            match self.update_display_forwards(start_idx) {
                Ok(_) => { }
                Err(err) => {error!("could not update display forwards: {}", err); return}
            }
        }
    }

    /// Insert a character to the terminal.
    ///
    /// # Arguments
    ///
    /// * `c`: the new character to insert.
    /// * `offset_from_end`: the position to insert the character. It represents the distance relative to the end of the whole output in the terminal in units of characters.
    /// 
    /// # Examples
    ///
    /// * `terminal.insert_char(char, 0)` will append `char` to the end of existing text.
    /// * `terminal.insert_char(char, 5)` will insert `char` right before the last 5 characters.
    ///
    /// After invoke this function, one must call `refresh_display` to get the updates actually showed on the screen.
    pub fn insert_char(&mut self, c: char, offset_from_end: usize) -> Result<(), &'static str> {
        let buflen = self.scrollback_buffer.len();
        if buflen < offset_from_end { return Err("offset_from_end is larger than length of scrollback buffer"); }
        let insert_idx = buflen - offset_from_end;
        self.scrollback_buffer.insert_str(insert_idx, &c.to_string());
        Ok(())
    }

    /// Remove a character from the terminal.
    ///
    /// # Arguments
    ///
    /// * `offset_from_end`: the position of the character to remove. It represents the distance relative to the end of the whole output in the terminal in units of characters. `offset_from_end == 0` is *invalid* here.
    /// 
    /// # Examples
    ///
    /// * `terminal.remove_char(1)` will remove the last character in the screen.
    ///
    /// After invoke this function, one must call `refresh_display` to get the updates actually showed on the screen.
    pub fn remove_char(&mut self, offset_from_end: usize) -> Result<(), &'static str> {
        let buflen = self.scrollback_buffer.len();
        if buflen < offset_from_end { return Err("offset_from_end is larger than length of scrollback buffer"); }
        if offset_from_end == 0 { return Err("cannot remove character at offset_from_end == 0"); }
        let remove_idx = buflen - offset_from_end;
        self.scrollback_buffer.remove(remove_idx);
        Ok(())
    }
    
    /// Scroll the screen to the very beginning.
    pub fn move_screen_to_begin(&mut self) -> Result<(), &'static str> {
        // Home command only registers if the text display has the ability to scroll
        if self.scroll_start_idx != 0 {
            self.is_scroll_end = false;
            self.scroll_start_idx = 0; // takes us up to the start of the page
            self.display_cursor()?;
        }
        
        Ok(())
    }

    /// Scroll the screen to the very end.
    pub fn move_screen_to_end(&mut self) -> Result<(), &'static str> {
        if !self.is_scroll_end {
            self.cursor.disable();
            self.display_cursor()?;
            self.is_scroll_end = true;
            let buffer_len = self.scrollback_buffer.len();
            self.scroll_start_idx = self.calc_start_idx(buffer_len, &self.display_name).0;
            self.cursor.enable();
        }
        Ok(())
    }

    /// Scroll the screen a line up.
    pub fn move_screen_line_up(&mut self) -> Result<(), &'static str> {
        if self.scroll_start_idx != 0 {
            self.scroll_up_one_line();
            self.display_cursor()?;
        }
        Ok(())
    }

    /// Scroll the screen a line down.
    pub fn move_screen_line_down(&mut self) -> Result<(), &'static str> {
        if !self.is_scroll_end {
            self.cursor.disable();
            self.display_cursor()?;
            self.scroll_down_one_line();
            self.cursor.enable();
        }
        Ok(())
    }

    /// Scroll the screen a page up.
    pub fn move_screen_page_up(&mut self) -> Result<(), &'static str> {
        if self.scroll_start_idx <= 1 {
            return Ok(());
        }
        self.page_up();
        self.is_scroll_end = false;
        self.display_cursor()
    }

    /// Scroll the screen a page down.
    pub fn move_screen_page_down(&mut self) -> Result<(), &'static str> {
        if self.is_scroll_end {
            return Ok(());
        }
        self.cursor.disable();
        self.display_cursor()?;
        self.page_down();
        self.cursor.enable();
        Ok(())
    }

    /// Clear all.
    pub fn clear(&mut self) {
        self.scrollback_buffer.clear();
        self.scroll_start_idx = 0;
        self.is_scroll_end = true;
    }

    pub fn initialize_screen(&mut self) -> Result<(), &'static str> {
        let display_name = self.display_name.clone();
        { 
            let (width, height) = self.window.dimensions();
            let width  = width  - 2*window_manager::WINDOW_MARGIN;
            let height = height - 2*window_manager::WINDOW_MARGIN;
            let text_display = TextDisplay::new(width, height, FONT_COLOR, BACKGROUND_COLOR)?;
            let displayable: Box<dyn Displayable> = Box::new(text_display);
            self.window.add_displayable(&display_name, Coord::new(0, 0),displayable)?;
        }
        Ok(())
    }

    /// Get a key event from the underlying window.
    pub fn get_event(&self) -> Option<Event> {
        self.window.get_event()
    }

    pub fn get_width_height(&self) -> (usize, usize) {
        self.get_displayable_dimensions(&self.display_name)
    }

    /// Display the cursor of the terminal.
    pub fn display_cursor(
        &mut self
    ) -> Result<(), &'static str> {
        let coordinate = self.window.get_displayable_position(&self.display_name)?;
        
        // get info about the text displayable
        let (col_num, line_num, text_next_pos) = {
            let text_display = self.window.get_concrete_display::<TextDisplay>(&self.display_name)?;
            let text_next_pos = text_display.get_next_pos();
            let (col_num, line_num) = text_display.get_dimensions();
            (col_num, line_num, text_next_pos)
        };
        
        // return if the cursor is not in the screen
        if text_next_pos >= col_num * line_num {
            return Ok(())
        }

        // calculate the cursor position
        let cursor_pos = text_next_pos - self.cursor.offset_from_end;
        let cursor_line = cursor_pos / col_num;
        let cursor_col = cursor_pos % col_num;
                
        self.cursor.display(
            coordinate,
            cursor_col,
            cursor_line,
            &mut self.window.framebuffer,
        );

        // update to the end of the text if the cursor is at the last line
        let text_width = if text_next_pos / col_num == cursor_line {
            text_next_pos % col_num
        } else {
            col_num * CHARACTER_WIDTH
        };

        let block = vec![(cursor_line, text_width)];
        self.window.render(Some(block.into_iter()))
    }

    /// Gets the position of the cursor relative to the end of text in units of characters.
    pub fn get_cursor_offset_from_end(&self) -> usize {
        self.cursor.offset_from_end
    }

    /// Updates the position of a cursor.
    /// # Arguments
    /// * `offset_from_end`: the position of the cursor relative to the end of text in units of characters.
    /// * `underlying_char`: the ASCII code of the underlying character when the cursor is unseen.
    pub fn update_cursor_pos(&mut self, offset_from_end: usize, underlying_char: u8) {
        self.cursor.offset_from_end = offset_from_end;
        self.cursor.underlying_char = underlying_char;
    }

}

/// The cursor structure.
/// A cursor is a special symbol shown in the text box of a terminal. It indicates the position of character where the next input would be put or the delete operation works on.
/// Terminal invokes its `display` method in a loop to let a cursor blink.
pub struct Cursor {
    /// Whether the cursor is enabled in the terminal.
    enabled: bool,
    /// The blinking frequency.
    freq: u64,
    /// The last time it blinks.
    time: TscTicks,
    /// The current blinking state show/hidden
    show: bool,
    /// The color of the cursor
    color: u32,
    /// The position of the cursor relative to the end of terminal text in units of number of characters.
    offset_from_end: usize,
    /// The underlying character at the position of the cursor.
    /// It is shown when the cursor is unseen.
    underlying_char: u8,
}

impl Cursor {
    /// Creates a new cursor structure with a default offset to the end of the text as 0 and the underlying character as `\0`. The initial `show` state is  `true` and the blinking frequency is defined as `libterm::DEFAULT_CURSOR_FREQ`. The parameter `color` specifies the color of the cursor.
    pub fn new(color: u32) -> Cursor {
        Cursor {
            enabled: true,
            freq: DEFAULT_CURSOR_FREQ,
            time: tsc_ticks(),
            show: true,
            color: color,
            offset_from_end: 0,
            underlying_char: 0,
        }
    }

    /// Resets the blink state of the cursor.
    pub fn reset(&mut self) {
        self.show = true;
        self.time = tsc_ticks();
    }

    /// Enables a cursor.
    pub fn enable(&mut self) {
        self.enabled = true;
        self.reset();
    }

    /// Disables a cursor.
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Changes the blink state show/hidden of a cursor based on its frequency.
    /// It returns whether the cursor should be re-display. If the cursor is enabled, it returns whether the show/hidden state has been changed. Otherwise it returns true because the cursor is disabled and should refresh.
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
        true
    }

    /// Checks if the cursor should be shown.
    pub fn show(&self) -> bool {
        self.enabled && self.show
    }

    /// Displays a cursor in a text block onto a frame buffer. An application calls this function in a loop to make it blinking.
    /// # Arguments
    /// * `coordinate`: the left top coordinate of the text block relative to the origin(top-left point) of the frame buffer.
    /// * `(column, line)`: the location of the cursor in the text block in units of characters.
    /// * `framebuffer`: the framebuffer to display onto.
    pub fn display(&mut self, coordinate: Coord, column: usize, line: usize, framebuffer: &mut dyn FrameBuffer) {
        if self.blink() {
            if self.show() {
                frame_buffer_drawer::fill_rectangle(
                    framebuffer,
                    coordinate + ((column * CHARACTER_WIDTH) as isize, (line * CHARACTER_HEIGHT) as isize) + (0, 1),
                    CHARACTER_WIDTH,
                    CHARACTER_HEIGHT - 2,
                    self.color,
                );
            } else {
                frame_buffer_printer::print_ascii_character(
                    framebuffer,
                    self.underlying_char,
                    FONT_COLOR,
                    BACKGROUND_COLOR,
                    coordinate,
                    column,
                    line,
                )
            }
        }
    }
}