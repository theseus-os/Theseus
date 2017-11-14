pub mod input; 
pub mod ata_pio;
pub mod pci;

use dfqueue::DFQueueProducer;
use console::ConsoleEvent;
use vga_buffer;


/// This is for functions that DO NOT NEED dynamically allocated memory. 
pub fn early_init() {
    assert_has_not_been_called!("drivers::early_init was called more than once!");
    vga_buffer::show_splash_screen();
}

/// This is for functions that require the memory subsystem to be initialized. 
pub fn init(console_producer: DFQueueProducer<ConsoleEvent>) {
    ata_pio::init_ata_devices();
    assert_has_not_been_called!("drivers::init was called more than once!");
    input::keyboard::init(console_producer);
    
    pci::init_pci_buses();



    
    //testing ata pio read, write, and IDENTIFY functionality, example of uses, can be deleted 
    /*
    let test_arr: [u16; 256] = [630;256];
    println!("Value from ATA identification function: {}", ata_pio::ATA_DEVICES.try().expect("ATA_DEVICES used before initialization").primary_master);
    let begin = ata_pio::pio_read(0xE0,0);
    //only use value if Result is ok
    if begin.is_ok(){
        println!("Value from drive at sector 0 before write:  {}", begin.unwrap()[0]);
    }
    ata_pio::pio_write(0xE0,0,test_arr);
    let end = ata_pio::pio_read(0xE0,0);
    if end.is_ok(){
    println!("Value from drive at sector 0 after write: {}", end.unwrap()[0]);
    }
    */

    // pci::init_pci_buses();
    /*
    let bus_array = pci::PCI_BUSES.try().expect("PCI_BUSES not initialized");
    let ref bus_zero = bus_array[0];
    let slot_zero = bus_zero.connected_devices[0]; 
    println!("pci config data for bus 0, slot 0: dev id - {:#x}, class code - {:#x}", slot_zero.device_id, slot_zero.class_code);
    println!("pci config data {:#x}",pci::pci_config_read(0,0,0,0x0c));
    println!("{:?}", bus_zero);
    */
}
