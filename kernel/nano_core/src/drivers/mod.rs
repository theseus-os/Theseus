pub mod input; 
pub mod ata_pio;
pub mod pci;
pub mod e1000;

use dfqueue::DFQueueProducer;
use console::ConsoleEvent;
use vga_buffer;
use drivers::e1000::e1000_nc;
use drivers::pci::PciDevice;

//????
use drivers::pci::get_pci_device_vd;
static INTEL_VEND: u16 =  0x8086;  // Vendor ID for Intel 
static E1000_DEV:  u16 =  0x100E;  // Device ID for the e1000 Qemu, Bochs, and VirtualBox emmulated NICs



/// This is for functions that DO NOT NEED dynamically allocated memory. 
pub fn early_init() {
    assert_has_not_been_called!("drivers::early_init was called more than once!");
    vga_buffer::show_splash_screen();
}

/// This is for functions that require the memory subsystem to be initialized. 
pub fn init(console_producer: DFQueueProducer<ConsoleEvent>) {
    assert_has_not_been_called!("drivers::init was called more than once!");
    input::keyboard::init(console_producer);
    
    for dev in pci::pci_device_iter() {
        debug!("Found pci device: {:?}", dev);
    }

        //create a NIC device and memory map it
        let pci_dev = get_pci_device_vd(INTEL_VEND,E1000_DEV);
        debug!("e1000 Device found: {:?}", pci_dev);
        let e1000_pci = pci_dev.unwrap();
        debug!("e1000 Device unwrapped: {:?}", pci_dev);
        let mut e1000_nc = e1000_nc::new(e1000_pci);
        //debug!("e1000_nc bar_type: {0}, mem_base: {1}, io_base: {2}", e1000_nc.bar_type, e1000_nc.mem_base, e1000_nc.io_base);

        e1000_nc.mem_map(e1000_pci);

        e1000_nc.mem_map_dma();

        e1000_nc.detectEEProm();

        e1000_nc.readMACAddress();

        e1000_nc.startLink();

        e1000_nc.clearMulticast();

        //e1000_nc.enableInterrupt();

        e1000_nc.rxinit();
        e1000_nc.txinit();
    
    
    //e1000_nc.checkState();
    
        

        //create a message
        //let a: usize = 0x0000_ffff_0000_ffff_0000_ffff_0000_ffff;
        let a :[usize;8] = [0;8];
        let length :u16 = 64;
        let add = &a as *const usize;
        let add1 = add as usize;
        e1000_nc.sendPacket(add1, length);
    
    //e1000_nc.checkState();
    

    

    // testing ata pio read, write, and IDENTIFY functionality, example of uses, can be deleted 
    /*
    ata_pio::init_ata_devices();
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

    /*
    let bus_array = pci::PCI_BUSES.try().expect("PCI_BUSES not initialized");
    let ref bus_zero = bus_array[0];
    let slot_zero = bus_zero.connected_devices[0]; 
    println!("pci config data for bus 0, slot 0: dev id - {:#x}, class code - {:#x}", slot_zero.device_id, slot_zero.class_code);
    println!("pci config data {:#x}",pci::pci_config_read(0,0,0,0x0c));
    println!("{:?}", bus_zero);
    */
}
