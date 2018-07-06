#![no_std]
/// Defines the methods that a DisplayProvider must have
pub trait DisplayProvider {
    /// Creates a new DisplayProvider
    /// Requires Self: Sized to make DisplayProvider object-safe for use in trait objects
    fn new() -> Self where Self: Sized;
    /// Takes a mutable string slice and display as much as it can from it
    fn display_string(&mut self, slice: &str) -> Result<usize, &'static str>;
    // Gets the dimensions of the text area to display
    fn get_dimensions(&self) -> (usize, usize);
    // Function to set a cursor on the display at an (x,y) position
    fn set_cursor(&self, x: u16, y: u16); 
    // Take the cursor off the display
    fn disable_cursor(&self);  
}

