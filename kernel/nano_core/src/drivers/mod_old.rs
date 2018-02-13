pub mod input; 
pub mod ata_pio;
pub mod pci;
<<<<<<< HEAD
pub mod e1000;
pub mod arp;
=======
pub mod acpi;

>>>>>>> theseus_main

use dfqueue::DFQueueProducer;
use console::ConsoleEvent;
use vga_buffer;
<<<<<<< HEAD
use drivers::e1000::e1000_nc;
use drivers::pci::PciDevice;
use drivers::arp::arp_packet;

//????
use drivers::pci::get_pci_device_vd;
static INTEL_VEND: u16 =  0x8086;  // Vendor ID for Intel 
static E1000_DEV:  u16 =  0x100E;  // Device ID for the e1000 Qemu, Bochs, and VirtualBox emmulated NICs

=======
use memory::{MemoryManagementInfo, PageTable};
>>>>>>> theseus_main


/// This is for early-stage initialization of things like VGA, ACPI, (IO)APIC, etc.
pub fn early_init(kernel_mmi: &mut MemoryManagementInfo) -> Result<acpi::madt::MadtIter, &'static str> {
    assert_has_not_been_called!("drivers::early_init was called more than once!");
    vga_buffer::show_splash_screen();
    
    // destructure the kernel's MMI so we can access its page table and vmas
    let &mut MemoryManagementInfo { 
        page_table: ref mut kernel_page_table, 
        ..  // don't need to access the kernel's vmas or stack allocator, we already allocated a kstack above
    } = kernel_mmi;

    match kernel_page_table {
        &mut PageTable::Active(ref mut active_table) => {
            // first, init the local apic info
            ::interrupts::apic::init(active_table);
            
            // then init/parse the ACPI tables to fill in the APIC details, among other things
            // this returns an iterator over the "APIC" (MADT) tables, which we use to boot AP cores
            let madt_iter = try!(acpi::init(active_table));

            Ok(madt_iter)
        }
        _ => {
            error!("drivers::early_init(): couldn't get kernel's active_table!");
            Err("Couldn't get kernel's active_table")
        }
    }
}



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
    //register interrupts
    //e1000_nc.enableInterrupt();

    e1000_nc.rxinit();
    e1000_nc.txinit();

    let mut mac_low : u16 = 0;
    let mut mac_next : u16 = 0;
    let mut mac_high : u16 = 0;

    e1000_nc.get_mac(&mut mac_low, &mut mac_next,&mut mac_high);

    debug!("{:x} {:x} {:x}", mac_low, mac_next, mac_high);
    
    /*
    //e1000_nc.checkState();  
        

    //create a message to test Tx
    let a: usize = 0x0000_ffff_4444_3333_2222_1111_2222_ffff;
    //let a :[usize;1] = [15;1];
    let length :u16 = 6;
    let add = &a as *const usize;
    let add1 = add as usize;
    e1000_nc.sendPacket(add1, length);
    
    //e1000_nc.checkState();*/

   /* //create arp announcement packet to test Rx

    let packet = arp_packet{
        dest1:  0xffff,
        dest2:  0xffff,
        dest3:  0xffff,
        source1:    mac_low,
        source2:    mac_next,
        source3:    mac_high,
        packet_type:   0x0608, //ARP
        h_type: 0x0100, //ethernet
        p_type: 0x0008, //ipv4
        hlen:   6, //ethernet
        plen:   4, //ipv4
        oper:   0x0100, //request
        sha1:   mac_low, //sender hw address, first 2 bytes
        sha2:   mac_next, // ", next 2 bytes
        sha3:   mac_high, // ", last 2 bytes
        spa1:   0x000A, // sender protocol address, first 2 B
        spa2:   0x0202, // ", last 2 B
        tha1:   0, //target ", first
        tha2:   0, // ", next
        tha3:   0, // ", last
        tpa1:   0x000A, // ", first
        tpa2:   0x0202, // ", last
    };

    let addr = &packet as *const arp_packet;
    let addr: usize = addr as usize;
    let length: u16 = 42;

    e1000_nc.sendPacket(addr, length);
*/
    //mac: 00:0b:82:01:fc:42
    //create DHCP discover packet
    let packet:[u8;314] = [0xff,0xff,0xff,0xff,0xff,0xff,0x00,0x0b,0x82,0x01,0xfc,0x42,0x08,0x00,0x45,0x00,
                               0x01,0x2c,0xa8,0x36,0x00,0x00,0xfa,0x11,0x17,0x8b,0x00,0x00,0x00,0x00,0xff,0xff,
                               0xff,0xff,0x00,0x44,0x00,0x43,0x01,0x18,0x59,0x1f,0x01,0x01,0x06,0x00,0x00,0x00,
                               0x3d,0x1d,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x0b,0x82,0x01,0xfc,0x42,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
                               0x00,0x00,0x00,0x00,0x00,0x00,0x63,0x82,0x53,0x63,0x35,0x01,0x01,0x3d,0x07,0x01,
                               0x00,0x0b,0x82,0x01,0xfc,0x42,0x32,0x04,0x00,0x00,0x00,0x00,0x37,0x04,0x01,0x03,
                               0x06,0x2a,0xff,0x00,0x00,0x00,0x00,0x00,0x00,0x00];
    let addr = &packet as *const u8;
    let addr: usize = addr as usize;
    let length: u16 = 314;
    e1000_nc.sendPacket(addr, length);
    //e1000_nc.poll_rx();
    //e1000_nc.get_mac_mem();

    

    

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
