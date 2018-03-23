pub mod ata_pio;
pub mod pci;
pub mod acpi;
pub mod e1000;
pub mod test_nic_driver;


use dfqueue::DFQueueProducer;
use console_types::ConsoleEvent;
use memory::{MemoryManagementInfo, PageTable};
use drivers::e1000::init_nic;

/// This is for early-stage initialization of things like VGA, ACPI, (IO)APIC, etc.
pub fn early_init(kernel_mmi: &mut MemoryManagementInfo) -> Result<acpi::madt::MadtIter, &'static str> {
    assert_has_not_been_called!("drivers::early_init was called more than once!");
    
    // destructure the kernel's MMI so we can access its page table and vmas
    let &mut MemoryManagementInfo { 
        page_table: ref mut kernel_page_table, 
        ..  // don't need to access the kernel's vmas or stack allocator, we already allocated a kstack above
    } = kernel_mmi;

    match kernel_page_table {
        &mut PageTable::Active(ref mut active_table) => {
            // first, init the local apic info
            try!(::interrupts::apic::init(active_table));
            
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



pub fn init(console_producer: DFQueueProducer<ConsoleEvent>) -> Result<(), &'static str>  {
    assert_has_not_been_called!("drivers::init was called more than once!");
    
    // call keyboard::init(console_producer)
    if let Some(section) = ::mod_mgmt::metadata::get_symbol("keyboard::init").upgrade() {
        let keyboard_init_func: fn(DFQueueProducer<ConsoleEvent>) = unsafe { ::core::mem::transmute(section.virt_addr()) };
        keyboard_init_func(console_producer);
    }
    else {
        return Err("getting keyboard::init symbol failed!");
    }


    
    for dev in pci::pci_device_iter() {
        debug!("Found pci device: {:?}", dev);
    }

    // try!(init_nic());

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
    Ok(())

}
