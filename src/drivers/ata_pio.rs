use port_io::Port;
use spin::{Once, Mutex}; 
use core::sync::atomic::{Ordering};
use interrupts::pit_clock;

#[allow(dead_code)]

//"PRIMARY" here refers to primary drive, drive connected at bus 0

//port data is read/write from
const PRIMARY_DATA_PORT_ADDRESS: u16 = 0x1F0;
const PRIMARY_ERROR_REGISTER_ADDRESS: u16 = 0x1F1;
//port which number of consecutive sectors to be read/written is sent to 
const PRIMARY_SECTORCOUNT_ADDRESS: u16 = 0x1F2;
//specificy lower, middle, and upper bytes of lba address
const PRIMARY_LBALO_ADDRESS: u16 = 0x1F3;
const PRIMARY_LBAMID_ADDRESS: u16 = 0x1F4;
const PRIMARY_LBAHI_ADDRESS: u16 = 0x1F5;
//select port for primary bus (bus 0)
const PRIMARY_BUS_SELECT_ADDRESS: u16 = 0x1F6;
//port which commands are sent to for primary ATA
const PRIMARY_COMMAND_IO_ADDRESS: u16 = 0x1F7;

//commands which set ATA drive to read or write mode
const PIO_WRITE_COMMAND: u8 = 0x30;
const PIO_READ_COMMAND: u8 = 0x20;

const IDENTIFY_COMMAND: u8 = 0xEC;
const READ_MASTER: u16 = 0xE0;
const MASTER_DRIVE_SELECT: u8 = 0xA0;


//initializing addresses mentioned above
static PRIMARY_BUS_SELECT: Mutex<Port<u8>> = Mutex::new( Port::new(PRIMARY_BUS_SELECT_ADDRESS));
static PRIMARY_DATA_PORT: Mutex<Port<u16>> = Mutex::new( Port::new(PRIMARY_DATA_PORT_ADDRESS));
static PRIMARY_ERROR_REGISTER: Mutex<Port<u8>> = Mutex::new( Port::new(PRIMARY_ERROR_REGISTER_ADDRESS));
static SECTORCOUNT: Mutex<Port<u8>> = Mutex::new( Port::new(PRIMARY_SECTORCOUNT_ADDRESS));
static LBALO: Mutex<Port<u8>> = Mutex::new( Port::new(PRIMARY_LBALO_ADDRESS));
static LBAMID: Mutex<Port<u8>> = Mutex::new( Port::new(PRIMARY_LBAMID_ADDRESS));
static LBAHI: Mutex<Port<u8>> = Mutex::new( Port::new(PRIMARY_LBAHI_ADDRESS));
static COMMAND_IO: Mutex<Port<u8>> = Mutex::new( Port::new(PRIMARY_COMMAND_IO_ADDRESS));


//holds AtaIdentifyData for primary and secondary bus
pub static ATA_DEVICES: Once<AtaDevices> = Once::new();



//port data is read/write from
const SECONDARY_DATA_PORT_ADDRESS: u16 = 0x170;
const SECONDARY_ERROR_REGISTER_ADDRESS: u16 = 0x171;
//port which number of consecutive sectors to be read/written is sent to 
const SECONDARY_SECTORCOUNT_ADDRESS: u16 = 0x172;
//specificy lower, middle, and upper bytes of lba address
const SECONDARY_LBALO_ADDRESS: u16 = 0x173;
const SECONDARY_LBAMID_ADDRESS: u16 = 0x174;
const SECONDARY_LBAHI_ADDRESS: u16 = 0x175;
//select port for primary bus (bus 0)
const SECONDARY_BUS_SELECT_ADDRESS: u16 = 0x176;
//port which commands are sent to for primary ATA
const SECONDARY_COMMAND_IO_ADDRESS: u16 = 0x177;


//initializing addresses mentioned above
static SECONDARY_BUS_SELECT: Mutex<Port<u8>> = Mutex::new( Port::new(SECONDARY_BUS_SELECT_ADDRESS));
static SECONDARY_DATA_PORT: Mutex<Port<u16>> = Mutex::new( Port::new(SECONDARY_DATA_PORT_ADDRESS));
static SECONDARY_ERROR_REGISTER: Mutex<Port<u8>> = Mutex::new( Port::new(SECONDARY_ERROR_REGISTER_ADDRESS));
static SECONDARY: Mutex<Port<u8>> = Mutex::new( Port::new(SECONDARY_SECTORCOUNT_ADDRESS));
static SECONDARY_LBALO: Mutex<Port<u8>> = Mutex::new( Port::new(SECONDARY_LBALO_ADDRESS));
static SECONDARY_LBAMID: Mutex<Port<u8>> = Mutex::new( Port::new(SECONDARY_LBAMID_ADDRESS));
static SECONDARY_LBAHI: Mutex<Port<u8>> = Mutex::new( Port::new(SECONDARY_LBAHI_ADDRESS));
static SECONDARY_COMMAND_IO: Mutex<Port<u8>> = Mutex::new( Port::new(SECONDARY_COMMAND_IO_ADDRESS));




pub fn init_ata_devices(){
	let mut identify_drives: AtaDevices = AtaDevices{..Default::default()};
	
	ATA_DEVICES.call_once( || {
		identify_drives.primary_master = get_ata_identify_data(0xA0);
		identify_drives.primary_slave = get_ata_identify_data(0xB0);
		identify_drives.secondary_master = AtaIdentifyData{..Default::default()};
		identify_drives.secondary_slave = AtaIdentifyData{..Default::default()};


	identify_drives});
	
	
	
	
}

//reads 256 u16s from primary ata data port
fn read_primary_data_port()-> Result<[u16; 256], u16>{
    let mut arr: [u16; 256] = [0;256];
	
	for word in 0..256{
		let mut loop_count = 0;

    	while(!ata_data_transfer_ready()){
			loop_count +=1;
			trace!("data port not ready in read_primary_data_port function");
			if loop_count > 1000{
				return Err(loop_count)
			}
			}
		arr[word] = PRIMARY_DATA_PORT.lock().read();

    }
	
    Ok(arr)

}

//writes 256 u16s from an array to primary ata data port
fn write_primary_data_port(arr: [u16;256])-> Result<u16, u16>{
	
	for index in 0..256{

		let mut loop_count = 0;
		while(!ata_data_transfer_ready()){
			//breaks loop and returns 
			loop_count += 1;
			trace!("data port not ready in write_primary_data_port function");
			if loop_count > 1000{
				return Err(index);
			//end of if 
			}
			//end of while
			}

		unsafe{PRIMARY_DATA_PORT.lock().write(arr[0])};
	}
	
	//pausing two pit ticks so that a read is never immediately after a write
	let start = pit_clock::PIT_TICKS.load(Ordering::SeqCst);
	while start+2 > pit_clock::PIT_TICKS.load(Ordering::SeqCst){}
	
	return Ok(256); 
}
//basic abstraction: returns True if ata is ready to transfer data, False otherwise
pub fn ata_data_transfer_ready() -> bool{

	(COMMAND_IO.lock().read()>>3)%2 ==1

}

//returns ATA identify information drive should be 0xA0 for master or 0xB0 for slave
pub fn get_ata_identify_data( drive:u8 )-> AtaIdentifyData{
    
    let mut command_value: u8 = COMMAND_IO.lock().read();
	let identify_data = AtaIdentifyData{..Default::default()};
    //let mut arr: [u16; 256] = [0; 256];
    //set port values for bus 0 to detect ATA device 
    unsafe{PRIMARY_BUS_SELECT.lock().write(drive);
           
           SECTORCOUNT.lock().write(0);
           LBALO.lock().write(0);
           LBAMID.lock().write(0);
           LBAHI.lock().write(0);

           COMMAND_IO.lock().write(IDENTIFY_COMMAND);


    }

	
    command_value = COMMAND_IO.lock().read();
    //if value is 0, no drive exists
    if command_value == 0{
        trace!("No Drive Exists");
		return identify_data;

    }
    
    
    //wait for update-in-progress value (bit 7 of COMMAND_IO port) to be set to 0
    command_value =(COMMAND_IO.lock().read());
    while ((command_value>>7)%2 != 0)  {
        //trace to debug and view value being received
        trace!("{}: update-in-progress in disk drive COMMAND_IO bit 7 not cleared", command_value);
        command_value = (COMMAND_IO.lock().read());
    }
    
    
    //if LBAhi or LBAlo values at this point are nonzero, drive is not ATA compatible
    if LBAMID.lock().read() != 0 || LBAHI.lock().read() !=0 {
        trace!("mid or hi LBA not set to 0 when it should be");
    }
    
	//waits for error bit or data ready bit to set, one of these should set at this point
    command_value = COMMAND_IO.lock().read();
    while((command_value>>3)%2 ==0 && command_value%2 == 0){
        trace!("{} is bit 0 of COMMAND_IO which should be cleared, {} is bit 6 which should be set",command_value, command_value>>3);
        command_value = COMMAND_IO.lock().read();
    }

	//if error is the value set, returns all 0 AtaIdentify
	if command_value%2 == 1{
		trace!("Error bit is set");
		
		return identify_data;

	}
    


	let identify_data = AtaIdentifyData::new(read_primary_data_port().unwrap()); 
    identify_data 
    
}

//read from disk at address input, drive = 0xE0 for master drive, 0xF0 for slave drive
pub fn pio_read(drive:u8, lba:u32)->Result<[u16; 256],u16>{
	let mut chosen_drive = &AtaIdentifyData{..Default::default()};

	if drive == 0xE0 {
		chosen_drive = &ATA_DEVICES.try().expect("ATA_DEVICES used before initialization").primary_master;
	}

	if drive == 0xF0{
		chosen_drive = &ATA_DEVICES.try().expect("ATA_DEVICES used before initialization").primary_slave;
	}
	trace!("{} number of sectors", chosen_drive.sector_count_28);
	if drive != 0xE0 && drive != 0xF0 {
		trace!("input drive value is unacceptable");
		return Err(0);
	}
	if lba+1> chosen_drive.sector_count_28{
		trace!("lba out of range of sectors");
		trace!("{} number of sectors", chosen_drive.sector_count_28);
		return Err(0);
	}
    //selects master drive(using 0xE0 value) in primary bus (by writing to primary_bus_select-port 0x1F6)
    let master_select: u8 = drive | (0 << 4) | ((lba >> 24) & 0x0F) as u8;
    unsafe{
		
	PRIMARY_BUS_SELECT.lock().write(master_select);

	//number of consecutive sectors to read from, set at 1 
	SECTORCOUNT.lock().write(1);
    //lba is written to disk ports 
    LBALO.lock().write((lba)as u8);
    LBAMID.lock().write((lba>>8)as u8);
    LBAHI.lock().write((lba>>16)as u8);

    COMMAND_IO.lock().write(PIO_READ_COMMAND);
    }

	if COMMAND_IO.lock().read()%2 == 1{
		trace!("error bit set");
		return Err(0);
	}

	//data is ready to read from data_io port	
    read_primary_data_port()

}

//returns number of shorts written to disk or error, drive = 0xE0 for master drive, 0xF0 for slave drive
pub fn pio_write(drive:u8, lba:u32, arr: [u16;256])->Result<u16, u16>{
	let mut chosen_drive = &AtaIdentifyData{..Default::default()};

	if drive == 0xE0 {
		chosen_drive = &ATA_DEVICES.try().expect("ATA_DEVICES used before initialization").primary_master;
	}

	if drive == 0xF0{
		chosen_drive = &ATA_DEVICES.try().expect("ATA_DEVICES used before initialization").primary_slave;
	}
	trace!("{} number of sectors", chosen_drive.sector_count_28);
	if drive != 0xE0 && drive != 0xF0 {
		return Err(0);
	}
	if lba+1> chosen_drive.sector_count_28{
		trace!("{} number of sectors", chosen_drive.sector_count_28);
		return Err(0);
	}
	let master_select: u8 = drive | (0 << 4) | ((lba >> 24) & 0x0F) as u8;
    unsafe{	
	PRIMARY_BUS_SELECT.lock().write(master_select);

	//number of consecutive sectors to write to: set at one currently
	SECTORCOUNT.lock().write(1);
    //lba(address) is written to disk ports
    LBALO.lock().write((lba)as u8);
    LBAMID.lock().write((lba>>8)as u8);
    LBAHI.lock().write((lba>>16)as u8);

    COMMAND_IO.lock().write(PIO_WRITE_COMMAND);
    }

	//data is ready to be written to data_io port
	write_primary_data_port(arr)

}

//exists to handle interrupts from PCI
//could be used later to replace polling system with interrupt system for reading and writing
pub fn handle_primary_interrupt(){
    trace!("Got IRQ 14!");
}

//AtaIdentifyData struct and implemenations from Tifflin Kernel
#[repr(C,packed)]
pub struct AtaIdentifyData
{
	pub flags: u16,
	_unused1: [u16; 9],
	pub serial_number: [u8; 20],
	_unused2: [u16; 3],
	pub firmware_ver: [u8; 8],
	pub model_number: [u8; 40],
	/// Maximum number of blocks per transfer
	pub sect_per_int: u16,
	_unused3: u16,
	pub capabilities: [u16; 2],
	_unused4: [u16; 2],
	/// Bitset of translation fields (next five shorts)
	pub valid_ext_data: u16,
	_unused5: [u16; 5],
	pub size_of_rw_multiple: u16,
	/// LBA 28 sector count (if zero, use 48)
	pub sector_count_28: u32,
	_unused6: [u16; 100-62],
	/// LBA 48 sector count
	pub sector_count_48: u64,
	_unused7: [u16; 2],
	/// [0:3] Physical sector size (in logical sectors
	pub physical_sector_size: u16,
	_unused8: [u16; 9],
	/// Number of words per logical sector
	pub words_per_logical_sector: u32,
	_unusedz: [u16; 257-119],
}

impl Default for AtaIdentifyData {
	fn default() -> AtaIdentifyData {
		// SAFE: Plain old data
		unsafe { ::core::mem::zeroed() }
	}

}


impl AtaIdentifyData{

	//takes an array storing data from ATA IDENTIFY command and returns struct with the relevant information
	fn new(arr: [u16; 256])-> AtaIdentifyData{

		//transmutes the array of u16s from the ATA device into an ATAIdentifyData struct
		let mut identify_data: AtaIdentifyData =unsafe {::core::mem::transmute(arr)};
		flip_bytes(&mut identify_data.serial_number);
		flip_bytes(&mut identify_data.firmware_ver);
		flip_bytes(&mut identify_data.model_number);


		return identify_data

	}


}

//holds AtaIdentifyData for possible pci drives, zeroed if no ata device on that bus
pub struct AtaDevices
{
	pub primary_master: AtaIdentifyData,
	pub primary_slave: AtaIdentifyData,
	pub secondary_master: AtaIdentifyData,
	pub secondary_slave: AtaIdentifyData,

}
impl Default for AtaDevices{
	fn default()->AtaDevices{
		// SAFE: Plain old data
		unsafe{::core::mem::zeroed()}
	}

}


//used to print ATAIdentifyData information to console
impl ::core::fmt::Display for AtaIdentifyData {
	fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result
	{
		write!(f, "AtaIdentifyData {{");
		write!(f, " flags: {:#x}", self.flags);
		write!(f, " serial_number: {:?}", RawString(&self.serial_number));
		write!(f, " firmware_ver: {:?}", RawString(&self.firmware_ver));
		write!(f, " model_number: {:?}", RawString(&self.model_number));
		write!(f, " sect_per_int: {}", self.sect_per_int & 0xFF);
		write!(f, " capabilities: [{:#x},{:#x}]", self.capabilities[0], self.capabilities[1]);
		write!(f, " valid_ext_data: {}", self.valid_ext_data);
		write!(f, " size_of_rw_multiple: {}", self.size_of_rw_multiple);
		write!(f, " sector_count_28: {:#x}", self.sector_count_28);
		write!(f, " sector_count_48: {:#x}", self.sector_count_48);
		write!(f, " physical_sector_size: {}", self.physical_sector_size);
		write!(f, " words_per_logical_sector: {}", self.words_per_logical_sector);
		write!(f, "}}");
		Ok( () )
	}
}

//flips pairs of bytes, helpful for transfers between certain big-endian and little-endian interfaces 
fn flip_bytes(bytes: &mut [u8]) {
	for pair in bytes.chunks_mut(2) {
		pair.swap(0, 1);
	}
}

//prints basic ASCII characters to the console
pub struct RawString<'a>(pub &'a [u8]);
impl<'a> ::core::fmt::Debug for RawString<'a>
{
	fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result
	{
		try!(write!(f, "b\""));
		for &b in self.0
		{
			match b
			{
			b'\\' => try!(write!(f, "\\\\")),
			b'\n' => try!(write!(f, "\\n")),
			b'\r' => try!(write!(f, "\\r")),
			b'"' => try!(write!(f, "\\\"")),
			b'\0' => try!(write!(f, "\\0")),
			// ASCII printable characters
			32...127 => try!(write!(f, "{}", b as char)),
			_ => try!(write!(f, "\\x{:02x}", b)),
			}
		}
		try!(write!(f, "\""));
		::core::result::Result::Ok( () )
	}
}