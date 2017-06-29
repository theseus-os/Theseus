pub mod serial_port;
pub mod input; 
pub mod pci;
#[macro_use] pub mod vga_buffer;


pub fn early_init() {
    assert_has_not_been_called!("drivers::early_init was called more than once!");
    vga_buffer::show_splash_screen();
}

pub fn init() {
    assert_has_not_been_called!("drivers::init was called more than once!");
    input::keyboard::init();
}