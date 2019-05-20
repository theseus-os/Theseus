
use super::super::{TextVFrameBuffer, WindowObj, display_text, Arc, VFRAME_BUFFER, Display, Mutex};
use super::super::{String};
use display_text::font::{CHARACTER_WIDTH, CHARACTER_HEIGHT};

/// A displayable component for text display
pub struct TextDisplay {
    name:String,
    width:usize,
    height:usize,
    textbuffer:TextVFrameBuffer,
}

impl TextDisplay
{
    pub fn new(name:&str, width:usize, height:usize) -> Result <TextDisplay, &'static str> {
        //let (width, height) = frame_buffer::get_resolution()?;
        let tf = TextVFrameBuffer::new(width, height)?;
        Ok(TextDisplay{
            name:String::from(name),
            width:width,
            height:height,
            textbuffer:tf,
        })
    }

    /// takes in a str slice and display as much as it can to the text area
    pub fn display_string(&self, window:&WindowObj, slice:&str, font_color:u32, bg_color:u32) -> Result<(), &'static str>{       
        match self.get_display_pos(window) {
            Ok((x, y)) => {
                // Do not need  x, y
                self.textbuffer.print_by_bytes(0, 0, self.width, self.height,
                    slice, font_color, bg_color)?;
                return frame_buffer::display(&self.textbuffer.vbuffer);
            },
            Err(err) => {return Err(err);}
        }
    }

    /// Gets the dimensions of the text area to display
    pub fn get_dimensions(&self) -> (usize, usize){
        (self.width/CHARACTER_WIDTH, self.height/CHARACTER_HEIGHT)
    }

    ///Gets the size of the text area
    pub fn get_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    ///resize the text displayable area
    pub fn resize(&mut self, width:usize, height:usize) {
        self.width = width;
        self.height = height;
    }

    /// Function to set a cursor on the display at a position. 
    pub fn set_cursor(&self, window:&WindowObj, line: u16, column: u16, color:u32, reset:bool) -> Result<(), &'static str>{
        let mut cursor = self.textbuffer.cursor.lock();
        cursor.enable();
        cursor.update(line as usize, column as usize, reset);
        match self.get_display_pos(window) {
            Ok((x, y)) => {
                match VFRAME_BUFFER.try(){
                    Some(buffer) =>  buffer.lock().fill_rectangle(x + (column as usize) * CHARACTER_WIDTH, 
                            y + (line as usize) * CHARACTER_HEIGHT, 
                            CHARACTER_WIDTH, CHARACTER_HEIGHT, color),
                    None => { return Err("Fail to get the virtual frame buffer")}
                }
            },
            Err(err) => { return Err( err);}
        }
        Ok(())   
    } 

    /// Take the cursor off the display
    pub fn disable_cursor(&self){
        let mut cursor = self.textbuffer.cursor.lock();
        cursor.disable();
    }

    /// Display the cursor and let it blinks. Called in a loop
    pub fn cursor_blink(&self, window:&WindowObj, font_color:u32, bg_color:u32) -> Result<(), &'static str>{
        let mut cursor = self.textbuffer.cursor.lock();
        if cursor.blink() {
            let (line, column, show) = cursor.get_info();
            match self.get_display_pos(window) {
                Ok((x, y)) => {
                    let color = if show { font_color } else { bg_color };
                    match VFRAME_BUFFER.try(){
                        Some(buffer) => { buffer.lock().fill_rectangle(x + (column as usize) * CHARACTER_WIDTH, 
                                y + (line as usize) * CHARACTER_HEIGHT, 
                                CHARACTER_WIDTH, CHARACTER_HEIGHT, color) 
                            },
                        None => {return Err("Fail to get the virtual frame buffer")}
                    }

                },
                Err(e) => { return Err(e) }
            }                
        }
        Ok(())
    }

    // Get the position of this displayable
    fn get_display_pos(&self, window:&WindowObj) -> Result<(usize, usize), &'static str> {
        let content_pos = window.get_content_position();
        match window.get_displayable_position(&(self.name)) {
            Ok(display_pos) => {return Ok((content_pos.0 + display_pos.0, content_pos.1 + display_pos.1));}
            Err(err) => {return Err(err);}
        }       
    }

    pub fn buffer(&self) -> &TextVFrameBuffer {
        &self.textbuffer
    }
}

