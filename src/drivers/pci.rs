use port_io::Port;
use spin::Mutex; 
use core::sync::atomic::{AtomicUsize, Ordering};
use interrupts::pit_clock;


//data written here sets information at CONFIG_DATA
const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

//drive select port for primary bus (bus 0)
const BUS_SELECT_PRIMARY: u16 = 0x1F6;
//set "DRIVE_SELECT" port to choose master or slave drive
const IDENTIFY_MASTER_DRIVE: u16 = 0xA0;
const IDENTIFY_SLAVE_DRIVE: u16 = 0xB0;

const IDENTIFY_COMMAND: u16 = 0xEC;

const READ_MASTER: u16 = 0xE0;



//access to CONFIG_ADDRESS 
static PCI_CONFIG_ADDRESS_PORT: Mutex<Port<u32>> = Mutex::new( Port::new(CONFIG_ADDRESS));
//acccess to CONFIG_DATA
static PCI_CONFIG_DATA_PORT: Mutex<Port<u32>> = Mutex::new( Port::new(CONFIG_DATA));

//ports used in IDENTIFY command 
static PRIMARY_BUS_IO: Mutex<Port<u16>> = Mutex::new( Port::new(BUS_SELECT_PRIMARY));
static SECTORCOUNT: Mutex<Port<u16>> = Mutex::new( Port::new(0x1F2));
static LBALO: Mutex<Port<u16>> = Mutex::new( Port::new(0x1F3));
static LBAMID: Mutex<Port<u16>> = Mutex::new( Port::new(0x1F4));
static LBAHI: Mutex<Port<u16>> = Mutex::new( Port::new(0x1F5));
static COMMAND_IO: Mutex<Port<u16>> = Mutex::new( Port::new(0x1F7));
static PRIMARY_DATA_PORT: Mutex<Port<u16>> = Mutex::new( Port::new(0x1F0));



//used to read from PCI config, additionally initializes PCI buses to be used
//might be better to set input paramters as u8 (method used in osdev)
pub fn pciConfigRead(bus: u32, slot: u32, func: u32, offset: u32)->u16{
    
    //data to be written to CONFIG_ADDRESS
    let address:u32 = ((bus<<16) | (slot<<11) |  (func << 8) | (offset&0xfc) | 0x80000000);

    unsafe{PCI_CONFIG_ADDRESS_PORT.lock().write(address);}

    ((PCI_CONFIG_DATA_PORT.lock().read() >> (offset&2) * 8) & 0xffff)as u16

}


//returns 0 if there is no ATA compatible device connected 
pub fn ATADriveExists(drive:u16)-> u16{
    
    let mut command_value: u16 = COMMAND_IO.lock().read();;
    
    //set port values for bus 0 to detect ATA device 
    unsafe{PRIMARY_BUS_IO.lock().write(drive);
           
           SECTORCOUNT.lock().write(0);
           LBALO.lock().write(0);
           LBAMID.lock().write(0);
           LBAHI.lock().write(0);

           //COMMAND_IO.lock().write(0xEC);


    }
    command_value = COMMAND_IO.lock().read();
    //if value is 0, no drive exists
    if command_value == 0{
        return 0;
    }
    
    
    //wait for update-in-progress value (bit 7 of COMMAND_IO port) to be set to 0
    command_value =(COMMAND_IO.lock().read());
    while ((command_value>>7)%2 != 0)  {
        //trace to debug and view value being received
        trace!("{}", command_value);
        command_value = (COMMAND_IO.lock().read());
    }
    
    
    //if LBAhi or LBAlo values at this point are nonzero, drive is not ATA compatible
    if LBAMID.lock().read() != 0 || LBAHI.lock().read() !=0 {
        return LBAHI.lock().read();
    }
    

    command_value = COMMAND_IO.lock().read();
    while((command_value>>3)%2 ==0 && command_value%2 == 0){
        trace!("{}",command_value);
        trace!("{}", command_value>>7);
        command_value = COMMAND_IO.lock().read();
    }
    
    if command_value>>3%2 != 1{
            return 5
        }
    return 1


}

//read from disk at address input 
pub fn pio_read(lba:u32)->u16{

    //selects master drive(using 0xE0 value) in primary bus (by writing to PRIMARY_BUS_IO-port 0x1F6)
    let master_select: u16 = 0xE0 | (0 << 4) | ((lba >> 24) & 0x0F)as u16;
    unsafe{PRIMARY_BUS_IO.lock().write(master_select);

    SECTORCOUNT.lock().write(0);

    //lba is written into disk ports
    LBALO.lock().write((lba&0xFF)as u16);
    trace!("{} here",lba>>8&0xFF);
    LBAMID.lock().write((lba>>8 &0xFF)as u16);
    LBAHI.lock().write((lba>>16 &0xFF)as u16);
    }

    //just returning this during testing to make sure program compiles
    //return COMMAND_IO.lock().read()>>3;
    PRIMARY_DATA_PORT.lock().read()



}

pub fn handle_primary_interrupt(){
    trace!("Got IRQ 14!")
}