//! Terminal emulator library
//!
//! The terminal roughly does the following things: manages all characters in a String that should be printed to the screen;
//! cuts a slice from this String and send it to window manager to get things actually printed; manages user input command line
//! as well as the cursor position, and delivers keyboard events.

#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate dfqueue;
extern crate environment;
extern crate print;
extern crate event_types;
extern crate spin;
extern crate frame_buffer_alpha;
extern crate window_manager_alpha;
extern crate window_components;
extern crate tsc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use event_types::Event;
use alloc::sync::Arc;
use spin::Mutex;
use tsc::{tsc_ticks, TscTicks};

pub const FONT_COLOR: u32 = 0x93ee90;
pub const BACKGROUND_COLOR: u32 = 0x000000;

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
    /// The terminal's own window, this is a WindowComponents object which handles the events of user input properly like moving window and close window
    window: Arc<Mutex<window_components::WindowComponents>>,
    // textarea object of the terminal, it has a weak reference to the window object so that calling function of this object will draw on the screen
    textarea: Arc<Mutex<window_components::TextArea>>,
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
    cursor: Cursor
}

/// Privite methods of `Terminal`.
impl Terminal {
    /// Get the width and height of the text display.
    fn get_displayable_dimensions(&self) -> (usize, usize){
        let textarea = self.textarea.lock();
        (textarea.get_x_cnt(), textarea.get_y_cnt())
    }

    /// This function takes in the end index of some index in the scrollback buffer and calculates the starting index of the
    /// scrollback buffer so that a slice containing the starting and ending index would perfectly fit inside the dimensions of 
    /// text display. 
    /// If the text display's first line will display a continuation of a syntactical line in the scrollback buffer, this function 
    /// calculates the starting index so that when displayed on the text display, it preserves that line so that it looks the same
    /// as if the whole physical line is displayed on the buffer
    /// 
    /// Return: starting index of the string and the cursor position(with respect to position on the screen, not in the scrollback buffer) in that order
    fn calc_start_idx(&self, end_idx: usize) -> (usize, usize) {
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
    fn calc_end_idx(&self, start_idx: usize) -> Result<usize, ScrollError> {
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
            return Ok(self.scrollback_buffer.len() - 1) /* WARNING: maybe should return Error? */
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

    /// Updates the cursor to a new position and refreshes display
    fn cursor_handler(&mut self, left_shift: usize) -> Result<(), &'static str> { 
        let (buffer_width, buffer_height) = self.get_displayable_dimensions();

        // We have shifted the cursor out of the screen.
        if left_shift / buffer_width > buffer_height {
            self.cursor.disable();
            return Ok(());
        }

        let new_x = (self.absolute_cursor_pos - left_shift) % buffer_width;
        let new_y = (self.absolute_cursor_pos - left_shift) / buffer_width;

        
        self.cursor.check_time();
        // debug!("absolute_cursor_pos: {}", self.absolute_cursor_pos);
        if self.cursor.enabled {
            if self.cursor.show {
                self.textarea.lock().set_char(new_x, new_y, 221)?;
            } else {
                self.textarea.lock().set_char(new_x, new_y, ' ' as u8)?;
            }
        }
        Ok(())
    }
}

/// Public methods of `Terminal`.
impl Terminal {
    pub fn new() -> Result<Terminal, &'static str> {
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
                wincomps.get_border_size() + TEXTAREA_BORDER, wincomps.get_title_size() + TEXTAREA_BORDER,
                width_inner - 2*TEXTAREA_BORDER, height_inner - 2*TEXTAREA_BORDER,
                &wincomps.winobj, None, None, Some(wincomps.get_background()), None
            ) {
                Ok(m) => m,
                Err(err) => { debug!("new textarea returned err"); return Err(err); }
            }
        };

        // let mut prompt_string = root.lock().get_absolute_path(); // ref numbers are 0-indexed
        let mut terminal = Terminal {
            window: window_object,
            textarea: textarea_object,
            scrollback_buffer: String::new(),
            scroll_start_idx: 0,
            is_scroll_end: true,
            absolute_cursor_pos: 0,
            cursor: Cursor::new()
        };

        // Inserts a producer for the print queue into global list of terminal print producers
        terminal.print_to_terminal(format!("Theseus Terminal Emulator\nPress Ctrl+C to quit a task\n"));
        terminal.absolute_cursor_pos = terminal.scrollback_buffer.len();
        Ok(terminal)
    }

    /// Adds a string to be printed to the terminal to the terminal scrollback buffer.
    /// Note that one needs to call `refresh_display` to get things actually printed. 
    pub fn print_to_terminal(&mut self, s: String) {
        self.scrollback_buffer.push_str(&s);
    }

    /// Actually refresh the screen. Currently it's expensive.
    pub fn refresh_display(&mut self, left_shift: usize) {
        let start_idx = self.scroll_start_idx;
        // handling display refreshing errors here so that we don't clog the main loop of the terminal
        if self.is_scroll_end {
            let buffer_len = self.scrollback_buffer.len();
            match self.update_display_backwards(buffer_len) {
                Ok(_) => { }
                Err(err) => {error!("could not update display backwards: {}", err); return}
            }
            match self.cursor_handler(left_shift) {
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

    /// Insert a character to the screen. The position is specified by parameter `left_shift`,
    /// that is the relative distance to the end of the whole output on the screen.
    /// left_shift == 0 means to append characters onto the screen, while left_shift == 1 means to
    /// insert a character right before the exsiting last character.
    /// One must call `refresh_display` to get things actually showed.
    pub fn insert_char_to_screen(&mut self, c: char, left_shift: usize) -> Result<(), &'static str> {
        let buflen = self.scrollback_buffer.len();
        if buflen < left_shift { return Err("left_shift is larger than length of scrollback buffer"); }
        let insert_idx = buflen - left_shift;
        self.scrollback_buffer.insert_str(insert_idx, &c.to_string());
        Ok(())
    }

    /// Remove a character from the screen. The position is specified by parameter `left_shift`,
    /// that is the relative distance to the end of the whole output on the screen.
    /// left_shift == 1 means to remove the last character on the screen.
    /// left_shift == 0 is INVALID here, since there's nothing at the "end" of the screen.
    /// One must call `refresh_display` to get things actually removed on the screen.
    pub fn remove_char_from_screen(&mut self, left_shift: usize) -> Result<(), &'static str> {
        let buflen = self.scrollback_buffer.len();
        if buflen < left_shift { return Err("left_shift is larger than length of scrollback buffer"); }
        if left_shift == 0 { return Err("cannot remove character at left_shift == 0"); }
        let remove_idx = buflen - left_shift;
        self.scrollback_buffer.remove(remove_idx);
        Ok(())
    }
    
    /// Scroll the screen to the very beginning.
    pub fn move_screen_to_begin(&mut self) {
        // Home command only registers if the text display has the ability to scroll
        if self.scroll_start_idx != 0 {
            self.is_scroll_end = false;
            self.scroll_start_idx = 0; // takes us up to the start of the page
            self.cursor.disable();
        }
    }

    /// Scroll the screen to the very end.
    pub fn move_screen_to_end(&mut self) {
        if !self.is_scroll_end {
            self.is_scroll_end = true;
            let buffer_len = self.scrollback_buffer.len();
            self.scroll_start_idx = self.calc_start_idx(buffer_len).0;
        }
    }

    /// Scroll the screen a line up.
    pub fn move_screen_line_up(&mut self) {
        if self.scroll_start_idx != 0 {
            self.scroll_up_one_line();
            self.cursor.disable();
        }
    }

    /// Scroll the screen a line down.
    pub fn move_screen_line_down(&mut self) {
        if !self.is_scroll_end {
            self.scroll_down_one_line();
        }
    }

    /// Scroll the screen a page up.
    pub fn move_screen_page_up(&mut self) {
        if self.scroll_start_idx <= 1 {
            return;
        }
        self.page_up();
        self.is_scroll_end = false;
        self.cursor.disable();
    }

    /// Scroll the screen a page down.
    pub fn move_screen_page_down(&mut self) {
        if self.is_scroll_end {
            return;
        }
        self.page_down();
    }

    /// Clear all.
    pub fn clear(&mut self) {
        self.scrollback_buffer.clear();
        self.scroll_start_idx = 0;
        self.absolute_cursor_pos = 0;
        self.is_scroll_end = true;
    }

    pub fn blink_cursor(&mut self, left_shift: usize) -> Result<(), &'static str> {
        let buffer_width = self.get_displayable_dimensions().0;
        let mut new_x = self.absolute_cursor_pos % buffer_width;
        let mut new_y = self.absolute_cursor_pos / buffer_width;
        // adjusts to the correct position relative to the max rightmost absolute cursor position
        if new_x >= left_shift  {
            new_x -= left_shift;
        } else {
            new_x = buffer_width + new_x - left_shift;
            new_y -= 1;
        }

        self.cursor.check_time();
        // debug!("absolute_cursor_pos: {}", self.absolute_cursor_pos);
        if self.cursor.enabled {
            if self.cursor.show {
                self.textarea.lock().set_char(new_x, new_y, 221)?;
            } else {
                self.textarea.lock().set_char(new_x, new_y, ' ' as u8)?;
            }
        }
        Ok(())
    }

    /// Get a key event from the underlying window.
    pub fn get_key_event(&self) -> Option<Event> {
        let mut wincomps = self.window.lock();
        match wincomps.handle_event() {
            Err(_e) => { return Some(Event::ExitEvent); }
            _ => {}
        };
        let _event = match wincomps.consumer.peek() {
            Some(ev) => ev,
            _ => { return None; }
        };
        let event = _event.clone();
        _event.mark_completed();
        Some(event)
    }

    pub fn get_width_height(&self) -> (usize, usize) {
        self.get_displayable_dimensions()
    }
}

/// The cursor structure is mainly a timer for cursor to blink properly, which also has multiple status recorded. 
/// When `enabled` is false, it should remain the original word. When `enabled` is true and `show` is false, it should display blank character, only when `enabled` is true, and `show` is true, it should display cursor character.
pub struct Cursor {
    /// Terminal will set this variable to enable blink or not. When this is not enabled, function `blink` will always return `false` which means do not refresh the cursor
    enabled: bool,
    /// The time of blinking interval. Initially set to `DEFAULT_CURSOR_BLINK_INTERVAL`, however, can be changed during run-time
    blink_interval: u64,
    /// Record the time of last blink state change. This variable is updated when `reset` is called or `blink` is called and the time duration is larger than `DEFAULT_CURSOR_BLINK_INTERVAL`
    time: TscTicks,
    /// If function `blink` returns true, then this variable indicates whether display the cursor or not. To fully determine whether to display the cursor, user should call `is_show` function
    show: bool,
}

const DEFAULT_CURSOR_BLINK_INTERVAL: u64 = 400000000;
impl Cursor {
    /// Create a new cursor object which is initially enabled. The `blink_interval` is initialized as `DEFAULT_CURSOR_BLINK_INTERVAL` however one can change this at any time. `time` is set to current time.
    pub fn new() -> Cursor {
        Cursor {
            enabled: true,
            blink_interval: DEFAULT_CURSOR_BLINK_INTERVAL,
            time: tsc_ticks(),
            show: true,
        }
    }

    /// Reset the cursor by setting `show` to true and `time` to current time. This doesn't effect `enabled` variable. This will not effect the display unless terminal application refresh the textarea by using the status of `Cursor` object
    pub fn reset(&mut self) {
        self.show = true;
        self.time = tsc_ticks();
    }

    /// Enable a cursor and call `reset` internally to make sure the behavior is the same after enable it (same initial state and same interval to change)
    pub fn enable(&mut self) {
        self.enabled = true;
        self.reset();
    }

    /// Disable a cursor by setting `enabled` to false
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Change the blink state shown/hidden of a cursor. The terminal calls this function in a loop. It does not effect the cursor directly, see doc of `Cursor` about how to use this.
    pub fn check_time(&mut self) -> bool {
        if self.enabled {
            let time = tsc_ticks();
            if let Some(duration) = time.sub(&(self.time)) {
                if let Some(ns) = duration.to_ns() {
                    if ns >= self.blink_interval {
                        self.time = time;
                        self.show = !self.show;
                        return true;
                    }
                }
            }
        }
        false
    }
}
