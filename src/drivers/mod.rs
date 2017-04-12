pub mod serial_port;
pub mod input; 
#[macro_use] pub mod vga_buffer;


pub fn early_init() {
    vga_buffer::show_splash_screen();

}

pub fn init() {
    input::keyboard::init();}