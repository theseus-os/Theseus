pub mod serial_port;
pub mod input; 
#[macro_use] pub mod vga_buffer;

/// This is for functions that DO NOT NEED dynamically allocated memory. 
pub fn early_init() {
    assert_has_not_been_called!("drivers::early_init was called more than once!");
    vga_buffer::show_splash_screen();
}

/// This is for functions that require the memory subsystem to be initialized. 
pub fn init() {
    assert_has_not_been_called!("drivers::init was called more than once!");
    input::keyboard::init();
}