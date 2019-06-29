//! Support for accessing ATA disks (IDE).

#![no_std]

#[macro_use] extern crate log;
extern crate spin;
extern crate port_io;
extern crate pci;
#[macro_use] extern crate bitflags;

use core::fmt;
use spin::{Once, Mutex}; 
use port_io::{Port, PortReadOnly, PortWriteOnly};
use pci::PciDevice;



const DEFAULT_PRIMARY_CHANNEL_DATA_PORT:         u16 = 0x1F0;
const DEFAULT_PRIMARY_CHANNEL_CONTROL_PORT:      u16 = 0x3F6;
const DEFAULT_SECONDARY_CHANNEL_DATA_PORT:       u16 = 0x170;
const DEFAULT_SECONDARY_CHANNEL_CONTROL_PORT:    u16 = 0x376;

/// To use a BAR as a Port address, you must mask out the lowest 2 bits.
const PCI_BAR_PORT_MASK: u16 = 0xFFFC;




bitflags! {
	/// The possible error values found in an ATA drive's error port.
    pub struct AtaError: u8 {
		const BAD_BLOCK              = 0x80;
		const UNCORRECTABLE_DATA     = 0x40;
		const MEDIA_CHANGED          = 0x20;
		const ID_MARK_NOT_FOUND      = 0x10;
		const MEDIA_CHANGE_REQUEST   = 0x08;
		const COMMAND_ABORTED        = 0x04;
		const TRACK_0_NOT_FOUND      = 0x02;
		const ADDRESS_MARK_NOT_FOUND = 0x01;
    }
}

bitflags! {
	/// The possible status values found in an ATA drive's status port.
    pub struct AtaStatus: u8 {
		const BUSY                 = 0x80;
		const DRIVE_READY          = 0x40;
		const DRIVE_WRITE_FAULT    = 0x20;
		const DRIVE_SEEK_COMPLETE  = 0x10;
		const DATA_REQUEST_READY   = 0x08;
		const CORRECTED_DATA       = 0x04;
		const INDEX                = 0x02;
		const ERROR                = 0x01;
    }
}

bitflags! {
	/// The possible control values used in an ATA drive's status port.
    pub struct AtaControl: u8 {
		/// Set this to read back the High Order Byte of the last-written LBA48 value.
		const HOB   = 0x80;
		/// Software reset
		const SRST  = 0x04;
		/// No interrupt enable -- set this to disable interrupts from the device. 
		const NIEN  = 0x02;
		// all other bits are reserved
    }
}

/// The possible commands that can be issues to an ATA drive's command port. 
/// More esoteric commands (nearly a full list) are here: <https://wiki.osdev.org/ATA_Command_Matrix>.
#[repr(u8)]
pub enum AtaCommand {
	/// Read sectors with retry
	READ_PIO         = 0x20,
	READ_PIO_EXT     = 0x24,
	/// Read DMA with retry
	READ_DMA         = 0xC8,
	READ_DMA_EXT     = 0x25,
	/// Write sectors with retry
	WRITE_PIO        = 0x30,
	WRITE_PIO_EXT    = 0x34,
	/// Write DMA with retry
	WRITE_DMA        = 0xCA,
	WRITE_DMA_EXT    = 0x35,
	CACHE_FLUSH      = 0xE7,
	CACHE_FLUSH_EXT  = 0xEA,
	PACKET           = 0xA0,
	IDENTIFY_PACKET  = 0xA1,
	/// The primary command for getting identifying details of an ATA drive.
	IDENTIFY_DEVICE  = 0xEC,
}

/// The two types of ATA drives that may exist on one bus.
/// The value is the bitmask used to select either master or slave
/// in the ATA drive's `drive_select` port.
#[derive(Copy, Clone, Debug)]
enum BusDriveSelect {
	Master = 0 << 4,
	Slave  = 1 << 4,
}




/// A single ATA drive, either a Master or a Slave, 
/// within a larger ATA controller.
#[derive(Debug)]
pub struct AtaDrive {
	/// The port that holds the data to be written or the data from a read.
	/// Located at `BAR0 + 0`.
	data: Port<u16>,
	/// The error port, shared with the `features` port.
	/// Located at `BAR0 + 1`.
	error: PortReadOnly<u8>,
	/// The features port, shared with the `error` port.
	/// Located at `BAR0 + 1`.
	features: PortWriteOnly<u8>,
	/// The number of sectors to read or write.
	/// Located at `BAR0 + 2`.
	sector_count: Port<u8>,
	/// The low byte `[0:8)` of the linear block address (LBA) of the sector that we want to read or write. 
	/// Located at `BAR0 + 3`.
	lba_0: Port<u8>,
	/// The middle byte `[8:16)`of the linear block address (LBA) of the sector that we want to read or write. 
	/// Located at `BAR0 + 4`.
	lba_1: Port<u8>,
	/// The high byte `[16:24)` of the linear block address (LBA) of the sector that we want to read or write. 
	/// Located at `BAR0 + 5`.
	lba_2: Port<u8>,
	/// `HDDEVSEL`, used for selecting a drive in the channel.
	/// The lower 4 bits of this port are used for the upper 4 bits of the 28-bit LBA.
	/// Located at `BAR0 + 6`.
	drive_select: Port<u8>,
	/// The command port, shared with the `status` port.
	/// Located at `BAR0 + 7`.
	command: PortWriteOnly<u8>,
	/// The status port, shared with the `command` port.
	/// Located at `BAR0 + 7`.
	status: PortReadOnly<u8>, //PortReadOnly<AtaStatus>,

	/// Another status port. 
	/// Has the same value as the `status` port, but reading this does not affect interrupts.
	/// This port is mostly used for a polling wait, as reading it takes approximately 100ns.
	/// Located at `BAR1 + 2`.
	alternate_status: PortReadOnly<u8>,
	/// The control port, shared with the `alternate_status` port.
	/// This should be set to 0 once during boot.
	/// Located at `BAR1 + 2`.
	control: PortWriteOnly<u8>,
	/// `DEVADDRESS`, located at `BAR1 + 3`. 
	/// Not sure what this is used for.
	drive_address: Port<u8>,

	/// Data that represents the characteristics of the drive. 
	identify_data: AtaIdentifyData,
	/// Whether this drive is a master or slave on the bus.
	master_slave: BusDriveSelect,
}

impl AtaDrive {
	/// Looks for an ATA drive at the location specified by the given data and control BARs,
	/// and if one is found, it probes and initializes that drive and returns an object representing it.
	/// 
	/// Since two drives (one master and one slave) may exist at the same data and control BAR,
	/// the caller may specify which one to search for, 
	/// or look for both by calling this twice: once with `which = Master` and once with `which = Slave`.
	fn new(data_bar: u16, control_bar: u16, which: BusDriveSelect) -> Result<AtaDrive, &'static str> {
		let data_bar = data_bar & PCI_BAR_PORT_MASK;
		let control_bar = control_bar & PCI_BAR_PORT_MASK;

		// First, we need to create a drive object and probe that drive to see if it exists.
		let mut drive = AtaDrive { 
			data: Port::new(data_bar + 0),
			error: PortReadOnly::new(data_bar + 1),
			features: PortWriteOnly::new(data_bar + 1),
			sector_count: Port::new(data_bar + 2),
			lba_0: Port::new(data_bar + 3),
			lba_1: Port::new(data_bar + 4),
			lba_2: Port::new(data_bar + 5),
			drive_select: Port::new(data_bar + 6),
			command: PortWriteOnly::new(data_bar + 7),
			status: PortReadOnly::new(data_bar + 7),

			alternate_status: PortReadOnly::new(control_bar + 2),
			control: PortWriteOnly::new(control_bar + 2),
			drive_address: Port::new(control_bar + 3),

			identify_data: AtaIdentifyData::default(), // fill this in later
			master_slave: which,
		};

		unsafe { drive.control.write(0) }; // clear out the control port before the first drive access

		// Then use an identify command to see if the drive exists.
		drive.identify_data = drive.identify_drive()?;

		Ok(drive)
	}


	/// Issues an ATA identify command to probe the drive
	/// and query its characteristics. 
	/// 
	/// See this link: <https://wiki.osdev.org/ATA_PIO_Mode#IDENTIFY_command>
	fn identify_drive(&mut self) -> Result<AtaIdentifyData, &'static str> {
		unsafe {
			self.drive_select.write(0xA0 | self.master_slave as u8);
			self.sector_count.write(0);
			self.lba_0.write(0);
			self.lba_1.write(0);
			self.lba_2.write(0);
			// issue the actual commannd
			self.command.write(AtaCommand::IDENTIFY_DEVICE as u8);
		}

		if self.status().is_empty() {
			return Err("drive did not exist");
		}

		// wait until the BUSY status bit is cleared
		while self.status().intersects(AtaStatus::BUSY) {
			// check for a non-ATA drive
			if self.lba_1.read() != 0 || self.lba_2.read() != 0 {
				return Err("drive was not ATA");
			}
		}

		match (self.lba_1.read(), self.lba_2.read()) {
			(0x14, 0xEB) => {
				// PATAPI device
				return Err("drive was an unsupported PATAPI device");
			}
			(0x69, 0x96) => {
				// SATAPI device
				return Err("drive was an unsupported SATAPI device");
			}
			(0x00, 0x00) => {
				// PATA device, the one we currently support
			}
			(0x3C, 0xC3) => {
				// SATA device
				return Err("drive was an unsupported SATA device");
			}
			_ => {
				return Err("drive was an unknown device type");				
			}
		}

		// wait until the drive is ready to read data from, or an error has occurred
		loop {
			let status = self.status();
			if status.intersects(AtaStatus::DATA_REQUEST_READY) {
				break; // ready to go!
			}
			if status.intersects(AtaStatus::ERROR) {
				return Err("error reading identify data from drive");
			}
		}
		
		if self.status().intersects(AtaStatus::ERROR) {
			return Err("error reading identify data from drive");
		}

		// time to read the actual data
		let mut arr: [u16; 256] = [0; 256];
		for i in 0..256 {
			arr[i] = self.data.read();
		}

		Ok(AtaIdentifyData::new(arr))
    }


	/// Issues a software reset for this drive.
	pub fn software_reset(&mut self) {
		// The procedure is to first set the SRST bit, 
		// then to wait 5 microseconds,
		// the to clear the SRST bit.
		unimplemented!()
	}



	pub fn status(&mut self) -> AtaStatus {
		// Read the status port 4 times: a 400ns delay to accommodate slow drives.
		for _ in 0..4 {
			self.status.read();
		}
		AtaStatus::from_bits_truncate(self.status.read())
	}
}



/// A single ATA controller has two buses with up to two drives attached to each bus,
/// for a total of up to four drives. 
pub struct AtaController {
	primary_master:    Option<AtaDrive>,
	primary_slave:     Option<AtaDrive>,
	secondary_master:  Option<AtaDrive>,
	secondary_slave:   Option<AtaDrive>,
}

impl AtaController {
	/// Creates a new instance of an ATA disk controller based on the given PCI device.
	pub fn new(pci_device: &PciDevice) -> Result<AtaController, &'static str> {
		let primary_channel_data_port = match pci_device.bars[0] {
			0x0 | 0x1 => DEFAULT_PRIMARY_CHANNEL_DATA_PORT,
			other => {
				warn!("Untested rare condition: ATA drive PCI BAR0 was special address value: {:#X}", other);
				other as u16
			}
		};
		let primary_channel_control_port = match pci_device.bars[1] {
			0x0 | 0x1 => DEFAULT_PRIMARY_CHANNEL_CONTROL_PORT,
			other => {
				warn!("Untested rare condition: ATA drive PCI BAR1 was special address value: {:#X}", other);
				other as u16
			}
		};
		let secondary_channel_data_port = match pci_device.bars[2] {
			0x0 | 0x1 => DEFAULT_SECONDARY_CHANNEL_DATA_PORT,
			other => {
				warn!("Untested rare condition: ATA drive PCI BAR2 was special address value: {:#X}", other);
				other as u16
			}
		};
		let secondary_channel_control_port = match pci_device.bars[3] {
			0x0 | 0x1 => DEFAULT_SECONDARY_CHANNEL_CONTROL_PORT,
			other => {
				warn!("Untested rare condition: ATA drive PCI BAR3 was special address value: {:#X}", other);
				other as u16
			}
		};

		let bus_master_base = pci_device.bars[4];

		trace!("Probing primary master...");
		let primary_master   = AtaDrive::new(primary_channel_data_port, primary_channel_control_port, BusDriveSelect::Master);
		trace!("Probing primary slave...");
		let primary_slave    = AtaDrive::new(primary_channel_data_port, primary_channel_control_port, BusDriveSelect::Slave);
		trace!("Probing secondary master...");
		let secondary_master = AtaDrive::new(secondary_channel_data_port, secondary_channel_control_port, BusDriveSelect::Master);
		trace!("Probing secondary slace...");
		let secondary_slave  = AtaDrive::new(secondary_channel_data_port, secondary_channel_control_port, BusDriveSelect::Slave);
		
		debug!("Primary master: {:?}", primary_master);
		debug!("Primary slave: {:?}", primary_slave);
		debug!("Secondary master: {:?}", secondary_master);
		debug!("Secondary slave: {:?}", secondary_slave);

		
		Err("unfinished")
	}


}



#[derive(Copy, Clone)]
#[repr(packed)]
pub struct AtaIdentifyData
{
	flags: u16,
	_unused1: [u16; 9],
	serial_number: [u8; 20],
	_unused2: [u16; 3],
	firmware_ver: [u8; 8],
	model_number: [u8; 40],
	/// Maximum number of blocks per transfer
	sect_per_int: u16,
	_unused3: u16,
	capabilities: [u16; 2],
	_unused4: [u16; 2],
	/// Bitset of translation fields (next five shorts)
	valid_ext_data: u16,
	_unused5: [u16; 5],
	size_of_rw_multiple: u16,
	/// LBA 28 sector count (if zero, use 48)
	sector_count_28: u32,
	_unused6: [u16; 100-62],
	/// LBA 48 sector count
	sector_count_48: u64,
	_unused7: [u16; 2],
	/// [0:3] Physical sector size (in logical sectors)
	physical_sector_size: u16,
	_unused8: [u16; 9],
	/// Number of words per logical sector
	words_per_logical_sector: u32,
	_unusedz: [u16; 257-119],
}

impl Default for AtaIdentifyData {
	fn default() -> AtaIdentifyData {
		AtaIdentifyData {
			flags: 0,
			_unused1: [0; 9],
			serial_number: [0; 20],
			_unused2: [0; 3],
			firmware_ver: [0; 8],
			model_number: [0; 40],
			sect_per_int: 0,
			_unused3: 0,
			capabilities: [0; 2],
			_unused4: [0; 2],
			valid_ext_data: 0,
			_unused5: [0; 5],
			size_of_rw_multiple: 0,
			sector_count_28: 0,
			_unused6: [0; 100-62],
			sector_count_48: 0,
			_unused7: [0; 2],
			physical_sector_size: 0,
			_unused8: [0; 9],
			words_per_logical_sector: 0,
			_unusedz: [0; 257-119],
		}
	}
}
impl AtaIdentifyData {
	/// Converts the given array of `u16` words, which should be the result of an ATA identify command,
	/// into the appropriate detailed struct.
	fn new(arr: [u16; 256])-> AtaIdentifyData{
		let mut identify_data: AtaIdentifyData =unsafe {::core::mem::transmute(arr)};
		flip_bytes(&mut identify_data.serial_number);
		flip_bytes(&mut identify_data.firmware_ver);
		flip_bytes(&mut identify_data.model_number);
		return identify_data
	}
}

impl fmt::Debug for AtaIdentifyData {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "AtaIdentifyData {{ \
			flags: {:#X}, \
			serial_number: {:?}, \
			firmware_ver: {:?}, \
			model_number: {:?}, \
			sect_per_int: {}, \
			capabilities: [{:#X}, {:#X}], \
			valid_ext_data: {}, \
			size_of_rw_multiple: {}, \
			sector_count_28: {}, \
			sector_count_48: {}, \
			physical_sector_size: {}, \
			words_per_logical_sector: {}, \
			}}",
			self.flags,
			RawString(&self.serial_number),
			RawString(&self.firmware_ver),
			RawString(&self.model_number),
			self.sect_per_int & 0xFF,
			self.capabilities[0], self.capabilities[1],
			self.valid_ext_data,
			self.size_of_rw_multiple,
			self.sector_count_28,
			self.sector_count_48,
			self.physical_sector_size,
			self.words_per_logical_sector,
		)
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


