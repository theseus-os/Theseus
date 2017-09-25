use port_io::Port;
use spin::{Once, Mutex}; 
use core::sync::atomic::{Ordering};
use interrupts::pit_clock;

///port-mapped IO registers to access ISA DMA
///registers 0 and 1 are unusable 
const DMA0_CHAN0_ADDR_REG: u16 = 0x00;
const DMA0_CHAN0_COUNT_REG: u16 = 0x01;
const DMA0_CHAN1_ADDR_REG: u16 = 0x02;
const DMA0_CHAN1_COUNT_REG: u16 = 0x03;
const DMA0_CHAN2_ADDR_REG: u16 = 0x04;
const DMA0_CHAN2_COUNT_REG: u16 = 0x05;
const DMA0_CHAN3_ADDR_REG: u16 = 0x06;
const DMA0_CHAN3_COUNT_REG: u16 = 0x07;

///registers that store top 8 bits of page address
///allows DMA to access up to 16MB of memory
///named ADDRBYTE2 to distinguish more from address ports
const DMA_PAGE_CHAN1_ADDRBYTE2: u16 = 0x83;
const DMA_PAGE_CHAN2_ADDRBYTE2: u16 = 0x81;
const DMA_PAGE_CHAN3_ADDRBYTE2: u16 = 0x82;


const DMA0_STATUS_AND_COMMAND_REG: u16 = 0x08;
const DMA0_REQUEST_REG: u16 = 0x09;
const DMA0_CHANMASK_REG: u16 = 0x0a;
const DMA0_MODE_REG: u16 = 0x0b;
const DMA0_CLEARBYTE_FLIPFLOP_REG: u16 = 0x0c;
const DMA0_TEMP_REG: u16 = 0x0d;
const DMA0_CLEAR_MASK_REG: u16 = 0x0e;
const DMA0_MASK_REG: u16 = 0x0f;

///might need to actually be 0x0E, 0xDC might be for DMA1
const DMA_UNMASK_ALL_REG: u16 = 0xdc;

///dma mode setting values 
const DMA_MODE_READ_TRANSFER: u8 = 0x04;
const DMA_MODE_WRITE_TRANSFER: u8 = 0x08;
const DMA_MODE_TRANSFER_SINGLE: u8 = 0x40;


///the addresses above used to define ports
static DMA0_CHAN1_ADDR_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(DMA0_CHAN1_ADDR_REG));
static DMA0_CHAN1_COUNT_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(DMA0_CHAN1_COUNT_REG));
static DMA0_CHAN2_ADDR_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(DMA0_CHAN2_ADDR_REG));
static DMA0_CHAN2_COUNT_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(DMA0_CHAN2_COUNT_REG));
static DMA0_CHAN3_ADDR_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(DMA0_CHAN3_ADDR_REG));
static DMA0_CHAN3_COUNT_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(DMA0_CHAN3_COUNT_REG));

///defining the ports using addresses above
static DMA0_STATUS_AND_COMMAND_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(DMA0_STATUS_AND_COMMAND_REG));
static DMA0_REQUEST_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(DMA0_REQUEST_REG));
static DMA0_CHANMASK_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(DMA0_CHANMASK_REG));
static DMA0_MODE_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(DMA0_MODE_REG));
static DMA0_CLEARBYTE_FLIPFLOP_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(DMA0_CLEARBYTE_FLIPFLOP_REG));
static DMA0_TEMP_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(DMA0_TEMP_REG));
static DMA0_CLEAR_MASK_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(DMA0_CLEAR_MASK_REG));
static DMA0_MASK_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(DMA0_MASK_REG));
static DMA_UNMASK_ALL_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(DMA_UNMASK_ALL_REG));

///defining ports to input upper byte of memory address
static DMA_PAGE_CHAN1_ADDRBYTE_PORT2: Mutex<Port<u8>> = Mutex::new( Port::new(DMA_PAGE_CHAN1_ADDRBYTE2));
static DMA_PAGE_CHAN2_ADDRBYTE_PORT2: Mutex<Port<u8>> = Mutex::new( Port::new(DMA_PAGE_CHAN2_ADDRBYTE2));
static DMA_PAGE_CHAN3_ADDRBYTE_PORT2: Mutex<Port<u8>> = Mutex::new( Port::new(DMA_PAGE_CHAN3_ADDRBYTE2));
///used to set the memory addresses dma channels to write to
pub fn dma_set_mem_address(channel: u8, mem_address_low: u8, mem_address_high: u8)->Result<u16, u16>{
    if channel>3 || channel == 0 {
        return Err(0);
    }
    
    unsafe{
    match channel {
        1 => DMA0_CHAN1_ADDR_PORT.lock().write(mem_address_low),
        2 => DMA0_CHAN2_ADDR_PORT.lock().write(mem_address_low),
        3 => DMA0_CHAN3_ADDR_PORT.lock().write(mem_address_low),
        _ => return Err(0),
    };
    match channel {
        1 => DMA0_CHAN1_ADDR_PORT.lock().write(mem_address_high),
        2 => DMA0_CHAN2_ADDR_PORT.lock().write(mem_address_high),
        3 => DMA0_CHAN3_ADDR_PORT.lock().write(mem_address_high),
        _ => return Err(0),
    };
    }
    Ok(1)
}

///setting transfer length in bytes minus 1
pub fn dma_set_transfer_count(channel: u8, count_low: u8, count_high: u8)->Result<u16, u16>{

    unsafe{
    match channel {
        1 => DMA0_CHAN1_COUNT_PORT.lock().write(count_low),
        2 => DMA0_CHAN2_COUNT_PORT.lock().write(count_low),
        3 => DMA0_CHAN3_COUNT_PORT.lock().write(count_low),
        _ => return Err(0),
    };
    match channel {
        1 => DMA0_CHAN1_COUNT_PORT.lock().write(count_high),
        2 => DMA0_CHAN2_COUNT_PORT.lock().write(count_high),
        3 => DMA0_CHAN3_COUNT_PORT.lock().write(count_high),
        _ => return Err(0),

    };
    }
    Ok(1)
}


///sets the dma mode
pub fn dma_set_mode(channel: u8, mode: u8) -> Result<u16, u16> {
    let dma: u8 = 0;
    
    if channel > 3 {
        return Err(0);
    }

    unsafe {
    DMA0_CHANMASK_PORT.lock().write(channel);
    DMA0_MODE_PORT.lock().write(channel | mode);
    }
    dma_unmask_all();
    Ok(1)
}

///unmasks all DMA registers
pub fn dma_unmask_all () { 

    unsafe{DMA_UNMASK_ALL_PORT.lock().write(0xff)};

}

///sets channel to read mode
pub fn dma_set_read(channel: u8) -> Result<u16, u16> {

    dma_set_mode(channel, DMA_MODE_READ_TRANSFER | DMA_MODE_TRANSFER_SINGLE)

}

pub fn dma_set_write(channel: u8) -> Result<u16, u16> {

    dma_set_mode(channel, DMA_MODE_WRITE_TRANSFER | DMA_MODE_TRANSFER_SINGLE)

}


pub fn dma_set_external_page_register( reg: u8, val: u8) -> Result<u16, u16> {
    
    unsafe{
    match reg {
        1 => DMA_PAGE_CHAN1_ADDRBYTE_PORT2.lock().write(val),
        2 => DMA_PAGE_CHAN2_ADDRBYTE_PORT2.lock().write(val),
        3 => DMA_PAGE_CHAN3_ADDRBYTE_PORT2.lock().write(val),
        _ => return Err(0),
    };
    }
    
    Ok(1)
}

///prepares DMA registers for low byte of address or command
pub fn dma_reset_flipflop() {

    unsafe {DMA0_CLEARBYTE_FLIPFLOP_PORT.lock().write(0xFF)};

}

///fully resets the DMA controller
pub fn dma_reset_full() {
    
    unsafe {DMA0_TEMP_PORT.lock().write(0xFF)};

}

///prepares a DMA channel between ISA port and memory
///set read to true to set to read mode and false to set to write mode
pub fn dma_setup(chan: u8, bytes: u16, read: bool ) -> Result<u32, u32> {
    
    //this is where the frame allocator will be used
    let mem_address: u32 = 0;
    dma_reset_full();
    dma_reset_flipflop();

    dma_set_mem_address(chan, mem_address as u8, (mem_address >> 8) as u8);
    dma_reset_flipflop();
    dma_set_transfer_count(chan, bytes as u8, (bytes>>8) as u8 );
    if read {
        dma_set_read(chan);
    }
    else {
        dma_set_write(chan);
    }
    dma_unmask_all();
    Ok(mem_address)

}