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
extern crate displayable;
extern crate font;
extern crate frame_buffer;
extern crate frame_buffer_drawer;
extern crate frame_buffer_printer;
extern crate tsc;
extern crate window;
extern crate window_manager;
extern crate text_area;
extern crate window_components;
extern crate frame_buffer_compositor;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::boxed::Box;
use cursor::*;
use displayable::{Displayable, TextDisplayable};
use event_types::Event;
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH};
use frame_buffer::{Coord, FrameBuffer, RectArea};
use frame_buffer_compositor::{Block};
use tsc::{tsc_ticks, TscTicks};
use window::{Window, WindowProfile};
use window_components::WindowComponents;

pub mod cursor;

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
    /// The terminal's own window. This is a `Window` trait object which handles the events of user input properly like moving window and close window
    pub window: WindowComponents,
    /// Name of the text displayable of the terminal
    display_name: String,
    /// The terminal's scrollback buffer which stores a string to be displayed by the text display
    scrollback_buffer: String,
    /// Indicates whether the text display is displaying the last part of the scrollback buffer slice
    is_scroll_end: bool,
    /// The starting index of the scrollback buffer string slice that is currently being displayed on the text display
    scroll_start_idx: usize,
    /// The cursor of the terminal.
    pub cursor: Cursor,
}

/// Privite methods of `Terminal`.
impl Terminal {
    /// Get the width and height of the text displayable in the unit of characters.
    fn get_text_dimensions(&self) -> (usize, usize) {
        let text_display = match self.get_text_display() {
            Ok(text_display) => text_display,
            Err(err) => {
                debug!("get_text_dimensions(): {}", err);
                return (0, 0);
            }
        };
        text_display.get_dimensions()
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
        let (buffer_width, buffer_height) = self.get_text_dimensions();
        let mut start_idx = end_idx;
        let result;
        // Grabs a max-size slice of the scrollback buffer (usually does not totally fit because of newlines)
        if end_idx > buffer_width * buffer_height {
            result = self.scrollback_buffer.get(end_idx - buffer_width * buffer_height..end_idx);
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
        let (buffer_width, buffer_height) = self.get_text_dimensions();
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
        let buffer_width = self.get_text_dimensions().0;
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
        let buffer_width = self.get_text_dimensions().0;
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
        let result = self.calc_end_idx(start_idx);
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
            {
                let text_display = self.window.get_displayable_mut(&self.display_name)?.as_text_mut()?;
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
        let (start_idx, _cursor_pos) = self.calc_start_idx(end_idx);
        self.scroll_start_idx = start_idx;

        let result = self.scrollback_buffer.get(start_idx..end_idx);

        if let Some(slice) = result {
            {
                let text_display = self.window.get_displayable_mut(&self.display_name)?.as_text_mut()?;
                text_display.set_text(slice);
            }
            self.window.display(&self.display_name)?;        
        } else {
            return Err("could not get slice of scrollback buffer string");
        }
        Ok(())
    }

    /// Gets a refrence to the text displayable of this terminal
    fn get_text_display(&self) -> Result<(&dyn TextDisplayable), &'static str> {
        let text_primitive = self.window.get_displayable(&self.display_name)?;
        text_primitive.as_text()
    }

    /// Gets a mutable refrence to the text displayable of this terminal
    fn get_text_display_mut(&mut self) -> Result<(&mut dyn TextDisplayable), &'static str> {
        let text_primitive = self.window.get_displayable_mut(&self.display_name)?;
        text_primitive.as_text_mut()
    }
}

/// Public methods of `Terminal`.
impl Terminal {
    pub fn new(
        window: WindowComponents,
        mut text_display: Box<dyn Displayable>
    ) -> Result<Terminal, &'static str> {
        if text_display.as_text().is_err() {
            return Err("Terminal::new(): the displayable is not a text displayable");
        }
        // let mut prompt_string = root.lock().get_absolute_path(); // ref numbers are 0-indexed
        let display_name = "text_display";
        let mut terminal = Terminal {
            window: window,
            display_name: String::from(display_name),
            scrollback_buffer: String::new(),
            scroll_start_idx: 0,
            is_scroll_end: true,
            cursor: Cursor::new(),
        };
        terminal
            .window
            .add_displayable(&display_name, Coord::new(0, 0), text_display)?;
        terminal.window.init_displayable(&display_name)?;

        // terminal.window.render(None)?;

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
    pub fn refresh_display(&mut self) -> Result<(), &'static str> {
        let start_idx = self.scroll_start_idx;
        // handling display refreshing errors here so that we don't clog the main loop of the terminal
        if self.is_scroll_end {
            let buffer_len = self.scrollback_buffer.len();
            self.update_display_backwards(buffer_len)?;
        } else {
            self.update_display_forwards(start_idx)?;
        }

        Ok(())
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
            self.cursor.disable();
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
            self.scroll_start_idx = self.calc_start_idx(buffer_len).0;
            self.cursor.enable();
        }
        Ok(())
    }

    /// Scroll the screen a line up.
    pub fn move_screen_line_up(&mut self) -> Result<(), &'static str> {
        if self.scroll_start_idx != 0 {
            self.scroll_up_one_line();
            self.cursor.disable();
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
        self.cursor.disable();
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

    /// Get a key event from the underlying window.
    pub fn get_event(&mut self) -> Option<Event> {
        match self.window.handle_event() {
            Err(_e) => {
                return Some(Event::ExitEvent);
            }
            _ => {}
        };
        let _event = match self.window.consumer().peek() {
            Some(ev) => ev,
            _ => {
                return None;
            }
        };
        let event = _event.clone();
        _event.mark_completed();
        Some(event)
    }

    /// Get (width, height) of the text area in units of characters
    pub fn get_width_height(&self) -> (usize, usize) {
        self.get_text_dimensions()
    }

    /// Display the cursor of the terminal.
    pub fn display_cursor(
        &mut self
    ) -> Result<(), &'static str> {
        let coordinate = self.window.get_displayable_position(&self.display_name)?;
        // get info about the text displayable
        let (col_num, line_num, text_next_pos) = {
            let text_primitive = self.get_text_display_mut()?;
            let text_next_pos = text_primitive.get_next_index();
            let (col_num, line_num) = text_primitive.get_dimensions();
            (col_num, line_num, text_next_pos)
        };

        // return if the cursor is not in the screen
        if text_next_pos >= col_num * line_num {
            return Ok(());
        }

        // calculate the cursor position
        let cursor_pos = text_next_pos - self.cursor.offset_from_end();
        let cursor_line = cursor_pos / col_num;
        let cursor_col = cursor_pos % col_num;

        // Get the container to display the cursor in
        let update_area = {
            let mut window = self.window.winobj.lock();
            let area = self.cursor.display(
                coordinate,
                cursor_col,
                cursor_line,
                window.framebuffer_mut(),
            )?;
            area
        };   

        // update to the end of the text if the cursor is at the last line
        let text_width = if text_next_pos / col_num == cursor_line {
            text_next_pos % col_num
        } else {
            col_num * CHARACTER_WIDTH
        };

        // TODO: optimize with block
        self.window.render(Some(update_area))
    }

    /// Gets the position of the cursor relative to the end of text in units of characters.
    pub fn get_cursor_offset_from_end(&self) -> usize {
        self.cursor.offset_from_end()
    }

    /// Updates the position of a cursor.
    /// # Arguments
    /// * `offset_from_end`: the position of the cursor relative to the end of text in units of characters.
    /// * `underlying_char`: the ASCII code of the underlying character when the cursor is unseen.
    pub fn update_cursor_pos(&mut self, offset_from_end: usize, underlying_char: u8) {
        self.cursor.set_offset_from_end(offset_from_end);
        self.cursor.set_underlying_char(underlying_char);
    }
}
