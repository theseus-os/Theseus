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

	/// Reads 256 16-bit words (`u16`s) from this drive 
	/// and places them into the provided `buffer`. 
	/// 
	/// #Note
	/// This is slow, as it uses blocking port I/O instead of DMA. 
	pub fn read_pio(&self, buffer: &mut [u8; 256]) -> Result<usize, &'static str> {
		unimplemented!()
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


	
	/// Reads the `status` port and returns the value. 
	/// Because some disk drives operate (change wire values) very slowly,
	/// this undergoes the standard procedure of reading the port value 
	/// and discarding it 4 times before reading the real value. 
	/// Each read is a 100ns delay, so the total delay of 400ns is proper.
	pub fn status(&mut self) -> AtaStatus {
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


/// The format of the
/// 
/// Fuller documentation is available here:
/// <https://docs.microsoft.com/en-us/windows-hardware/drivers/ddi/content/ata/ns-ata-_identify_device_data#members
#[derive(Copy, Clone)]
#[repr(packed)]
pub struct AtaIdentifyData {
	general_configuration: u16,
	num_cylinders: u16,
	specific_configuration: u16,
	num_heads: u16,
	_reserved1: [u16; 2],
	num_sectors_per_track: u16,
	vendor_unique1: [u16; 3],
	serial_number: [u8; 20],
	_reserved2: [u16; 3],
	firmware_version: [u8; 8],
	model_number: [u8; 40],
	/// Maximum number of blocks per transfer.
	/// Sometimes referred to as "sectors per int".
	max_blocks_per_transfer: u8,
	vendor_unique2: u8,
	trusted_computing: u16,
	capabilities: u16,
	_reserved3: u16, // reserved word 50
	_reserved4: [u16; 2],
	/// A bitmask of translation fields valid and free fall control sensitivity
	translation_fields_valid: u8,
	free_fall_control_sensitivity: u8,
	num_current_cylinders: u16,
	num_current_heads: u16,
	current_sectors_per_track: u16,
	current_sector_capacity: u32, 
	current_multi_sector_setting: u8,
	/// MultiSectorSettingValid : 1;
	/// ReservedByte59 : 3;
	/// SanitizeFeatureSupported : 1;
	/// CryptoScrambleExtCommandSupported : 1;
	/// OverwriteExtCommandSupported : 1;
	/// BlockEraseExtCommandSupported : 1;
	ext_command_supported: u8,
	/// LBA 28 sector count (if zero, use 48)
	user_addressable_sectors: u32,
	_reserved5: u16,
	multiword_dma_support: u8,
	multiword_dma_active: u8,
	advanced_pio_modes: u8,
	_reserved6: u8,
	minimum_mw_transfer_cycle_time: u16,
	recommended_mw_transfer_cycle_time: u16,
	minimum_pio_cycle_time: u16,
	minimum_pio_cycle_time_io_ready: u16,
	additional_supported: u16,
	_reserved7: [u16; 5],
	/// only the first 5 bits are used, others are reserved
	queue_depth: u16,
	serial_ata_capabilities: u32,
	serial_ata_features_supported: u16,
	serial_ata_features_enabled: u16,
	major_revision: u16,
	minor_revision: u16,
	command_set_support: [u16; 3],
	command_set_active: [u16; 3],
	ultra_dma_support: u8,
	ultra_dma_active: u8,
	normal_security_erase_unit: u16,
	enhanced_security_erase_unit: u16,
	current_apm_level: u8,
	_reserved8: u8,
	master_password_id: u16,
	hardware_reset_result: u16,
	current_acoustic_value: u8,
	recommended_acoustic_value: u8,
	stream_min_request_size: u16,
	streaming_transfer_time_dma: u16,
	streaming_access_latency_dma_pio: u16,
	streaming_perf_granularity: u32, 
	/// The max user LBA when using 48-bit LBA
	max_48_bit_lba: u64,
	streaming_transfer_time: u16,
	dsm_cap: u16,
	/// [0:3] Physical sector size (in logical sectors)
	physical_logical_sector_size: u16, 
	inter_seek_delay: u16,
	world_wide_name: [u16; 4],
	reserved_for_world_wide_name_128: [u16; 4],
	reserved_for_tlc_technical_report: u16,
	words_per_logical_sector: u32,
	command_set_support_ext: u16,
	command_set_active_ext: u16,
	reserved_for_expanded_support_and_active: [u16; 6],
	msn_support: u16,
	security_status: u16,
	_reserved9: [u16; 31],
	cfa_power_mode1: u16,
	_reserved10: [u16; 7],
	nominal_form_factor: u16, 
	data_set_management_feature: u16, 
	additional_product_id: [u16; 4],
	_reserved11: [u16; 2],
	current_media_serial_number: [u16; 30],
	sct_command_transport: u16,
	_reserved12: [u16; 2],
	block_alignment: u16, 
	write_read_verify_sector_count_mode_3_only: [u16; 2],
	write_read_verify_sector_count_mode_2_only: [u16; 2],
	nv_cache_capabilities: u16,
	nv_cache_size_lsw: u16,
	nv_cache_size_msw: u16,
	nominal_media_rotation_rate: u16,
	_reserved13: u16, 
	nv_cache_time_to_spin_up_in_seconds: u8,
	_reserved14: u8,
	write_read_verify_sector_count_mode: u8,
	_reserved15: u8,
	_reserved16: u16,
	transport_major_version: u16,
	transport_minor_version: u16,
	_reserved17: [u16; 6],
	extended_num_of_user_addressable_sectors: u64,
	min_blocks_per_download_microcode: u16,
	max_blocks_per_download_microcode: u16,
	_reserved18: [u16; 19],
	signature: u8,
	checksum: u8,
}

impl AtaIdentifyData {
	/// Converts the given array of `u16` words, which should be the result of an ATA identify command,
	/// into the appropriate detailed struct.
	fn new(arr: [u16; 256])-> AtaIdentifyData{
		let mut identify_data: AtaIdentifyData =unsafe {::core::mem::transmute(arr)};
		flip_bytes(&mut identify_data.serial_number);
		flip_bytes(&mut identify_data.firmware_version);
		flip_bytes(&mut identify_data.model_number);
		return identify_data
	}
}

impl fmt::Debug for AtaIdentifyData {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "AtaIdentifyData {{ \
			general_configuration: {:#X}, \
			serial_number: {:?}, \
			firmware_version: {:?}, \
			model_number: {:?}, \
			max_blocks_per_transfer: {}, \
			capabilities: [{:#X}, {:#X}], \
			valid_ext_data: {}, \
			size_of_rw_multiple: {}, \
			sector_count_28: {}, \
			sector_count_48: {}, \
			physical_sector_size: {}, \
			words_per_logical_sector: {}, \
			}}",
			self.general_configuration,
			RawString(&self.serial_number),
			RawString(&self.firmware_version),
			RawString(&self.model_number),
			self.max_blocks_per_transfer & 0xFF,
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


