#![no_std]
/// Defines the methods that a TextDisplay must have
/// set_cursor() should accept coordinates within those specified by get_dimensions() and display to window
pub trait TextDisplay {
    // takes in a str slice and display as much as it can to the screen
    fn display_string(&mut self, slice: &str) -> Result<(), &'static str>;
    // Gets the dimensions of the text area to display
    fn get_dimensions(&self) -> (usize, usize);
    // Function to set a cursor on the display at an (x,y) position
    fn set_cursor(&self, x: u16, y: u16); 
    // Take the cursor off the display
    fn disable_cursor(&self);  
}

