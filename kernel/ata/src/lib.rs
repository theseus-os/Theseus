//! Basic driver for accessing ATA drives (IDE) as a storage device.
//! 
//! The primary struct of interest is [`AtaDrive`].
//! 
//! Support for DMA is not yet implemented, but the slower port-based I/O is fully supported.

#![no_std]
#![feature(abi_x86_interrupt)]

extern crate alloc;
#[macro_use] extern crate log;

use core::fmt;
use bitflags::bitflags;
use spin::Mutex;
use alloc::{
	boxed::Box, 
	format, 
	string::{String, ToString}, 
	sync::Arc
};
use port_io::{Port, PortReadOnly, PortWriteOnly};
use pci::PciDevice;
use storage_device::{StorageDevice, StorageDeviceRef, StorageController};
use io::{BlockIo, BlockReader, BlockWriter, IoError, KnownLength};
use x86_64::structures::idt::InterruptStackFrame;


const SECTOR_SIZE_IN_BYTES: usize = 512;

const DEFAULT_PRIMARY_CHANNEL_DATA_PORT:         u16 = 0x1F0;
const DEFAULT_PRIMARY_CHANNEL_CONTROL_PORT:      u16 = 0x3F6;
const DEFAULT_SECONDARY_CHANNEL_DATA_PORT:       u16 = 0x170;
const DEFAULT_SECONDARY_CHANNEL_CONTROL_PORT:    u16 = 0x376;

const MAX_LBA_28_VALUE: usize = (1 << 28) - 1;

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
		/// When set, the drive's port values are still changing, so ports shouldn't be accessed. 
		const BUSY                 = 0x80;
		/// When set, the drive is on. When cleared, the drive is sleeping or "spun down".
		const DRIVE_READY          = 0x40;
		const DRIVE_WRITE_FAULT    = 0x20;
		const DRIVE_SEEK_COMPLETE  = 0x10;
		/// When **cleared**, the drive is ready for data to be read/written. 
		/// When set, the drive is handling a data request and isn't ready for another command.
		const DATA_REQUEST_READY   = 0x08;
		const CORRECTED_DATA       = 0x04;
		const INDEX                = 0x02;
		const ERROR                = 0x01;
    }
}

bitflags! {
	/// The possible control values used in an ATA drive's status port.
    struct AtaControl: u8 {
		/// Set this to read back the High Order Byte of the last-written LBA48 value.
		const HOB   = 0x80;
		/// Software reset
		const SRST  = 0x04;
		/// No interrupt enable -- set this to disable interrupts from the device. 
		const NIEN  = 0x02;
		// all other bits are reserved
    }
}

#[allow(dead_code)]
/// The possible commands that can be issued to an ATA drive's command port. 
/// More esoteric commands (nearly a full list) are here: <https://wiki.osdev.org/ATA_Command_Matrix>.
#[repr(u8)]
enum AtaCommand {
	/// Read sectors using PIO (28-bit LBA)
	ReadPio         = 0x20,
	/// Read sectors using PIO (48-bit LBA)
	ReadPioExt      = 0x24,
	/// Read sectors using DMA (28-bit LBA)
	ReadDma         = 0xC8,
	/// Read sectors using DMA (48-bit LBA)
	ReadDmaExt      = 0x25,
	/// Write sectors using PIO (28-bit LBA)
	WritePio        = 0x30,
	/// Write sectors using PIO (48-bit LBA)
	WritePioExt     = 0x34,
	/// Write sectors using DMA (28-bit LBA)
	WriteDma        = 0xCA,
	/// Write sectors using DMA (48-bit LBA)
	WriteDmaExt     = 0x35,
	/// Flush the drive's bus cache (28-bit LBA).
	/// This is to be used after each write.
	CacheFlush      = 0xE7,
	/// Flush the drive's bus cache (48-bit LBA).
	/// This is to be used after each write.
	CacheFlushExt   = 0xEA,
	/// Sends a packet, for ATAPI devices using the packet interface (PI).
	Packet          = 0xA0,
	/// Get identifying details of an ATAPI drive.
	IdentifyPacket  = 0xA1,
	/// Get identifying details of an ATA drive.
	IdentifyDevice  = 0xEC,
}


/// The possible types of drive devices that can be attached to an IDE controller via ATA.
pub enum AtaDeviceType {
	/// A parallel ATA (PATA) drive, like a hard drive.
	/// This is the type previously known as just "ATA" before SATA existed.
	Pata,
	/// A parallel ATA (PATA) drive that uses the packet interface,
	/// like an optical CD-ROM drive.
	PataPi,
	/// A serial ATA (SATA) drive that is operating in legacy IDE emulation mode,
	/// **not the standard AHCI interface for SATA**.
	/// Some systems refer to this as a `SEMB` (SATA Enclosure Management Bridge) device,
	/// which may or may not be attached through a port multiplier.
	Sata,
	/// A serial ATA (SATA) drive that that is operating in legacy IDE emulation mode 
	/// and uses the packet interface.
	SataPi,
}
impl AtaDeviceType {
	/// Determines the ATA device type based on the values of the LBA mid and LBA high
	/// ports after an identify device command has been issued, but before the response has been read.
	fn from_lba(lba_mid: u8, lba_high: u8) -> Option<AtaDeviceType> {
		match (lba_mid, lba_high) {
			(0x00, 0x00) => Some(AtaDeviceType::Pata),
			(0x14, 0xEB) => Some(AtaDeviceType::PataPi),
			(0x3C, 0xC3) => Some(AtaDeviceType::Sata),
			(0x69, 0x96) => Some(AtaDeviceType::SataPi),
			_ => None,
		}
	}
}

/// The two types of ATA drives that may exist on one bus.
/// The value is the bitmask used to select either master or slave
/// in the ATA drive's `drive_select` port.
#[derive(Copy, Clone, Debug)]
#[repr(u8)]
enum BusDriveSelect {
	Master = 0 << 4,
	Slave  = 1 << 4,
}


/// TODO: support DMA like so: <https://wiki.osdev.org/ATA/ATAPI_using_DMA#The_Bus_Master_Register>
/// There is one instance of this struct for each `AtaBus`.
/// 
/// Note: TODO: depending on whether BAR4 is a Port I/O address or MMIO address, this could also be mapped into memory.
///             We need to have an abstraction either above or beneath `Volatile` that allows reads/writes from port I/O and memory addresses similarly.
#[allow(unused)]
struct AtaBusMaster {
	/// For the primary bus, this exists at BAR4 + 0.
	/// For the secondary,   this exists at BAR4 + 8.
	command:      Port<u8>,
	/// For the primary bus, this exists at BAR4 + 2.
	/// For the secondary,   this exists at BAR4 + 10.
	status:       Port<u8>,
	/// For the primary bus, this exists at BAR4 + 4.
	/// For the secondary,   this exists at BAR4 + 12.
	prdt_address: Port<u32>,
}


/// There are two ATA buses on an IDE controller,
/// and each one can have two drives attached to it:
/// one master drive and one slave drive. 
/// 
/// Although a bus logically "contains" one to two drives,
/// we don't model the structs that way. 
/// Instead, each `AtaDrive` contains a shared reference 
/// to whatever bus it is connected to, 
/// which ensures that commands destined for two different drives 
/// on the same bus do not interfere or interleave with each other. 
#[derive(Debug)]
struct AtaBus {
	/// The port that holds the data to be written or the data from a read.
	/// Located at `BAR0 + 0`.
	data: Port<u16>,
	/// The error port, shared with the `features` port.
	/// Located at `BAR0 + 1`.
	error: PortReadOnly<u8>,
	/// The features port, shared with the `error` port.
	/// Located at `BAR0 + 1`.
	_features: PortWriteOnly<u8>,
	/// The number of sectors to read or write.
	/// Located at `BAR0 + 2`.
	sector_count: Port<u8>,
	/// The low byte `[0:8)` of the linear block address (LBA) of the sector that we want to read or write. 
	/// Located at `BAR0 + 3`.
	lba_low: Port<u8>,
	/// The middle byte `[8:16)`of the linear block address (LBA) of the sector that we want to read or write. 
	/// Located at `BAR0 + 4`.
	lba_mid: Port<u8>,
	/// The high byte `[16:24)` of the linear block address (LBA) of the sector that we want to read or write. 
	/// Located at `BAR0 + 5`.
	lba_high: Port<u8>,
	/// `HDDEVSEL`, used for selecting a drive in the bus.
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
	_drive_address: Port<u8>,
}

impl AtaBus {
	/// Creates and sets up a new ATA bus at the location specified by the given data and control BARs.
	fn new(data_bar: u16, control_bar: u16) -> AtaBus {
		let data_bar = data_bar & PCI_BAR_PORT_MASK;
		let control_bar = control_bar & PCI_BAR_PORT_MASK;
		AtaBus { 
			data: Port::new(data_bar),
			error: PortReadOnly::new(data_bar + 1),
			_features: PortWriteOnly::new(data_bar + 1),
			sector_count: Port::new(data_bar + 2),
			lba_low: Port::new(data_bar + 3),
			lba_mid: Port::new(data_bar + 4),
			lba_high: Port::new(data_bar + 5),
			drive_select: Port::new(data_bar + 6),
			command: PortWriteOnly::new(data_bar + 7),
			status: PortReadOnly::new(data_bar + 7),

			alternate_status: PortReadOnly::new(control_bar + 2),
			control: PortWriteOnly::new(control_bar + 2),
			_drive_address: Port::new(control_bar + 3),
		}
	}

	/// Issues the actual read PIO command on the ATA Bus without performing any bounds checks.
	/// 
	/// See `AtaDrive::read_pio()` (the caller of this function) for more documentation.
	fn read_pio(&mut self, 
		buffer: &mut [u8],
		which: BusDriveSelect,
		lba_start: usize,
		sector_count: usize
	) -> Result<usize, &'static str> {
		if sector_count == 0 {
			return Ok(0);
		}

		// Use 28-bit LBAs, unless the LBA is too large, then we use 48-bit LBAs
		let using_lba_28 = lba_start <= MAX_LBA_28_VALUE;

		self.wait_for_data_done().map_err(|_| "error before issuing read pio command")?;

		// Set up and issue the read command.
		if using_lba_28 {
			unsafe {
				// bits [24:28] of the LBA need to go into the lower 4 bits of the `drive_select` port.
				self.drive_select.write(0xE0 | (which as u8) | ((lba_start >> 24) as u8 & 0x0F));
				self.sector_count.write(sector_count as u8);
				self.lba_high.write((lba_start >> 16) as u8);
				self.lba_mid.write( (lba_start >>  8) as u8);
				self.lba_low.write(  lba_start        as u8);
				self.command.write(AtaCommand::ReadPio as u8);
			}
		} else {
			// When using 48-bit LBAs, the high bytes of the sector_count and LBA must be written *before* the low bytes.
			unsafe {
				self.drive_select.write(0x40 | (which as u8));
				// write the high bytes
				self.sector_count.write((sector_count >> 8) as u8);
				self.lba_high.write((lba_start >> 40) as u8);
				self.lba_mid.write( (lba_start >> 32) as u8);
				self.lba_low.write( (lba_start >> 24) as u8);
				// write the low bytes
				self.sector_count.write(sector_count as u8);
				self.lba_high.write((lba_start >> 16) as u8);
				self.lba_mid.write( (lba_start >>  8) as u8);
				self.lba_low.write(  lba_start        as u8);
				self.command.write(AtaCommand::ReadPioExt as u8);
			}
		} 

		// Read the actual data, one sector at a time.
		let mut buffer_offset = 0;
		for _lba in lba_start .. (lba_start + sector_count) {
			// Before transferring each sector, we have to wait for the drive to be ready for data
			self.wait_for_data_ready().map_err(|_| "error during data read")?;

			for chunk in buffer[buffer_offset .. (buffer_offset + SECTOR_SIZE_IN_BYTES)].chunks_exact_mut(2) {
				// ATA PIO works by reading one 16-bit word at a time, 
				// so one read covers two bytes of the buffer.
				let word: u16 = self.data.read();
				chunk[0] = word as u8;
				chunk[1] = (word >> 8) as u8;
			}
			buffer_offset += SECTOR_SIZE_IN_BYTES;
		}
		self.wait_for_data_done().map_err(|_| "error after data read")?;
		Ok(sector_count)
	}

	// TODO: refactor read and write into a single internal "I/O" function, since they're mostly identical.

	/// Issues the actual write PIO command on the ATA Bus without performing any bounds checks.
	/// 
	/// See `AtaDrive::write_pio()` (the caller of this function) for more documentation.
	fn write_pio(&mut self, 
		buffer: &[u8],
		which: BusDriveSelect,
		lba_start: usize,
		sector_count: usize
	) -> Result<usize, &'static str> {
		if sector_count == 0 {
			return Ok(0);
		}

		// Use 28-bit LBAs, unless the LBA is too large, then we use 48-bit LBAs
		let using_lba_28 = lba_start <= MAX_LBA_28_VALUE;

		self.wait_for_data_done().map_err(|_| "error before issuing write command")?;

		// Set up and issue the write command.
		if using_lba_28 {
			unsafe {
				// bits [24:28] of the LBA need to go into the lower 4 bits of the `drive_select` port.
				self.drive_select.write(0xE0 | (which as u8) | ((lba_start >> 24) as u8 & 0x0F));
				self.sector_count.write(sector_count as u8);
				self.lba_high.write((lba_start >> 16) as u8);
				self.lba_mid.write( (lba_start >>  8) as u8);
				self.lba_low.write(  lba_start        as u8);
				self.command.write(AtaCommand::WritePio as u8);
			}
		} else {
			// When using 48-bit LBAs, the high bytes of the sector_count and LBA must be written *before* the low bytes.
			unsafe {
				self.drive_select.write(0x40 | (which as u8));
				// write the high bytes
				self.sector_count.write((sector_count >> 8) as u8);
				self.lba_high.write((lba_start >> 40) as u8);
				self.lba_mid.write( (lba_start >> 32) as u8);
				self.lba_low.write( (lba_start >> 24) as u8);
				// write the low bytes
				self.sector_count.write(sector_count as u8);
				self.lba_high.write((lba_start >> 16) as u8);
				self.lba_mid.write( (lba_start >>  8) as u8);
				self.lba_low.write(  lba_start        as u8);
				self.command.write(AtaCommand::WritePioExt as u8);
			}
		}

		// Write the actual data, one sector at a time. 
		let mut buffer_offset = 0;
		for _lba in lba_start .. (lba_start + sector_count) {
			// Before transferring each sector, we have to wait for the drive to be ready for data
			self.wait_for_data_ready().map_err(|_| "error during data write")?;

			for chunk in buffer[buffer_offset .. (buffer_offset + SECTOR_SIZE_IN_BYTES)].chunks_exact(2) {
				// ATA PIO works by writing one 16-bit word at a time, 
				// so one 16-bit write covers two bytes of the buffer.
				let word = (chunk[1] as u16) << 8 | (chunk[0] as u16);
				unsafe { self.data.write(word); }
			}
			buffer_offset += SECTOR_SIZE_IN_BYTES;
		}
		self.wait_for_data_done().map_err(|_| "error after data write")?;

		// Flush the drive's cache after each write command
		let cache_flush_cmd = if using_lba_28 { AtaCommand::CacheFlush } else { AtaCommand::CacheFlushExt };
		unsafe { self.command.write(cache_flush_cmd as u8) };

		self.wait_for_data_done().map_err(|_| "error after cache flush after data write")?;
		Ok(sector_count)
	}

	/// Issues an ATA identify command to probe the drive
	/// and query its characteristics. 
	/// 
	/// See this link: <https://wiki.osdev.org/ATA_PIO_Mode#IDENTIFY_command>
	fn identify_drive(&mut self, which: BusDriveSelect) -> Result<AtaIdentifyData, &'static str> {
		self.wait_for_data_done().map_err(|_| "error before issuing identify command")?;

		unsafe {
			self.drive_select.write(0xA0 | which as u8);
			self.sector_count.write(0);
			self.lba_high.write(0);
			self.lba_mid.write(0);
			self.lba_low.write(0);
			// issue the actual commannd
			self.command.write(AtaCommand::IdentifyDevice as u8);
		}

		// a status of 0 means that a drive was not attached
		if self.status().is_empty() {
			return Err("drive did not exist");
		}

		// wait until the BUSY status bit is cleared
		while self.status().intersects(AtaStatus::BUSY) {
			// check for a non-ATA drive
			if self.lba_mid.read() != 0 || self.lba_high.read() != 0 {
				return Err("drive was not ATA");
			}
		}

		match AtaDeviceType::from_lba(self.lba_mid.read(), self.lba_high.read()) {
			Some(AtaDeviceType::Pata)   => { }, // we support this device type
			Some(AtaDeviceType::PataPi) => return Err("drive was an unsupported PATAPI device"),
			Some(AtaDeviceType::Sata)   => return Err("drive was an unsupported SATA device"),
			Some(AtaDeviceType::SataPi) => return Err("drive was an unsupported SATAPI device"),
			_                           => return Err("drive was an unknown device type"),
		};

		// we're ready to read the actual identify data, it's just one sector.
		let mut buffer: [u8; SECTOR_SIZE_IN_BYTES] = [0; SECTOR_SIZE_IN_BYTES];
		self.wait_for_data_ready().map_err(|_| "error before identify data read")?;
		for chunk in buffer.chunks_exact_mut(2) {
			// ATA PIO works by reading one 16-bit word at a time, 
			// so one read covers two bytes of the buffer.
			let word: u16 = self.data.read();
			chunk[0] = word as u8;
			chunk[1] = (word >> 8) as u8;
		}
		self.wait_for_data_done().map_err(|_| "error after identify data read")?;
		Ok(AtaIdentifyData::new(buffer))
    }
	
	/// Waits until the bus is ready to transfer data (either read or write).
	/// This is intended to be used **after** commands have been issued.
	/// 
	/// This performs a blocking poll that reads the bus's status 
	/// until it is no longer busy and data is ready to be transferred
	/// (`AtaStatus::BUSY` is `0` and `AtaStatus::DATA_REQUEST_READY` is `1`).
	/// 
	/// Returns an error if the `status` port indicates an error. 
	/// Invoke [`error()`](#method.error) to obtain more details on what kind of error occurred.
	fn wait_for_data_ready(&self) -> Result<(), ()> {
		let mut _loop_counter = 0;
		loop {
			let status = self.status();
			_loop_counter += 1;
			if status.intersects(AtaStatus::ERROR | AtaStatus::DRIVE_WRITE_FAULT) {
				return Err(());
			}
			if status.intersects(AtaStatus::BUSY) { 
				if _loop_counter % 1_000_000 == 0 {
					warn!("AtaBus::wait_for_data_ready() has been busy waiting for a long time... is there a device/driver problem? (status: {:?})", status);
				}
				continue;
			}
			if status.intersects(AtaStatus::DATA_REQUEST_READY) {
				return Ok(()); // ready to go!
			}
		}
	}

	/// Waits until this bus is finished transferring data (either read or write),
	/// in other words, until there is no pending data to be transferred.
	/// This is intended to be used **before** commands are issued.
	/// 
	/// Performs a blocking poll that reads the bus's status 
	/// until it is no longer busy and there is no data waiting to be transferred
	/// (`AtaStatus::BUSY` is `0` and `AtaStatus::DATA_REQUEST_READY` is `0`).
	/// 
	/// Returns an error if the `status` port indicates an error. 
	/// Invoke [`error()`](#method.error) to obtain more details on what kind of error occurred.
	fn wait_for_data_done(&self) -> Result<(), ()> {
		let mut _loop_counter = 0;
		loop {
			let status = self.status();
			_loop_counter += 1;
			if status.intersects(AtaStatus::ERROR | AtaStatus::DRIVE_WRITE_FAULT) {
				return Err(());
			}
			if status.intersects(AtaStatus::BUSY) { 
				if _loop_counter % 1_000_000 == 0 {
					warn!("AtaBus::wait_for_data_done() has been busy waiting for a long time... is there a device/driver problem? (status: {:?})", status);
				}
				continue;
			}
			if !status.intersects(AtaStatus::DATA_REQUEST_READY) {
				return Ok(()); // ready to go!
			}
		}
	}

	
	/// Reads the `status` port and returns the value as an `AtaStatus` bitfield. 
	/// Because some buses operate (change wire values) very slowly,
	/// this undergoes the standard procedure of reading the alternate status port 
	/// and discarding it 4 times before reading the real status port value. 
	/// Each read is a 100ns delay, so the total delay of 400ns is proper.
	fn status(&self) -> AtaStatus {
		self.alternate_status.read();
		self.alternate_status.read();
		self.alternate_status.read();
		self.alternate_status.read();
		AtaStatus::from_bits_truncate(self.status.read())
	}


	/// Reads the `error` port and returns the value as an `AtaError` bitfield.
	#[allow(dead_code)]
	fn error(&self) -> AtaError {
		AtaError::from_bits_truncate(self.error.read())
	}

	/// Issues a software reset to this bus, which resets *both* ATA drives on this bus,
	/// the master AND the slave. A reset *cannot* be issued to only a single drive on the bus.
	/// 
	/// This should only be used to clear leftover error values before identifying the drive,
	/// or when the drive is stuck in the BUSY status.
	///
	/// # Warning
	/// This resets BOTH (master and slave) drives on this bus, so do not call this
	/// unless you are certain the other drive has no in-progress transfers.
	fn software_reset(&mut self) {
		// Procedure is (1) set the SRST bit, (2) wait 5us, (3) clear the SRST bit.
		unsafe { self.control.write(AtaControl::SRST.bits()); }
		// We wait 5us by reading the status port 50 times (each read takes 100ns)
		for _ in 0..10 {
			self.status(); // reads status port 5 times.
		}
		unsafe { self.control.write(0); }
	}
}


/// A single ATA drive, either a master or a slave, 
/// which sits on one of two buses within a larger IDE controller.
#[derive(Debug)]
pub struct AtaDrive {
	/// A reference to the bus that this drive sits on,
	/// shared with the other AtaDrive that also sits on this bus.
	bus: Arc<Mutex<AtaBus>>,
	/// Data that represents the characteristics of the drive. 
	identify_data: AtaIdentifyData,
	/// Whether this drive is a master or slave on the bus.
	master_slave: BusDriveSelect,
}

impl AtaDrive {
	/// Looks for an ATA drive on the given `bus`, and if found,
	/// probes and initializes that drive and returns an object representing it.
	/// 
	/// Since two drives (one master and one slave) may exist on one IDE bus (sharing the same data and control BAR),
	/// the caller must specify *which* one to search for. 
	/// The caller can look for both by calling this twice: once with `which = Master` and once with `which = Slave`.
	fn new(bus: Arc<Mutex<AtaBus>>, which: BusDriveSelect) -> Result<AtaDrive, &'static str> {
		// Issue a preliminary software reset of the bus to clear out lingering errors.
		bus.lock().software_reset(); 
		// Then use an identify command to see if the drive exists.
		let identify_data = bus.lock().identify_drive(which)?;

		// Check to see that the drive supports LBA,
		// because we don't support the ancient CHS (cylinder-head-sector) addressing scheme.
		if identify_data.capabilities & 0x200 == 0 {
			return Err("drive is an ancient CHS device that doesn't support LBA addressing mode, but we don't support CHS.");
		}

		Ok(AtaDrive {
			bus, 
			identify_data,
			master_slave: which,
		})
	}

	/// Reads data from this drive starting at the given `offset_in_sectors` into the provided `buffer`.
	/// The length of the given `buffer` determines the number of bytes to be written.
	/// 
	/// As content is read from the drive at sector granularity, 
	/// the buffer length must be a multiple of the sector size (512 bytes),
	/// and the offset is specified in number of sectors (not number of bytes) from the beginning of the drive.
	/// 
	/// Returns the number of sectors (*not bytes*) that were successfully written to the drive.
	/// 
	/// # Note
	/// This is slow, as it uses blocking port I/O instead of DMA. 
	pub fn read_pio(&mut self, buffer: &mut [u8], offset_in_sectors: usize) -> Result<usize, &'static str> {
		if offset_in_sectors > self.size_in_blocks() {
			return Err("offset_in_sectors was out of bounds");
		}
		let length_in_bytes = buffer.len();
		if length_in_bytes % SECTOR_SIZE_IN_BYTES != 0 {
			return Err("The buffer length must be a multiple of sector size (512) bytes. ATA drives can only read at sector granularity.");
		}

		let lba_start = offset_in_sectors;
		let lba_end = lba_start + (length_in_bytes / SECTOR_SIZE_IN_BYTES);
		let sector_count = lba_end - lba_start;
		// trace!("AtaDrive::read_pio(): lba_start: {}, lba_end: {}, sector_count: {}",
		// 	lba_start, lba_end, sector_count,
		// );
		if sector_count > (self.identify_data.max_blocks_per_transfer as usize) {
			error!("AtaDrive::read_pio(): cannot read {} sectors, drive has a max of {} sectors per transfer.", 
				sector_count, self.identify_data.max_blocks_per_transfer
			);
			return Err("AtaDrive::read_pio(): cannot read more sectors than the drive's max");
		}
		
		self.bus.lock().read_pio(buffer, self.master_slave, lba_start, sector_count)
	}

	/// Writes data from the provided `buffer` to this drive, starting at the given `offset_in_sectors` into the drive.
	/// The length of the given `buffer` determines the number of bytes to be written.
	/// 
	/// As content is written to the drive at sector granularity, 
	/// the buffer length must be a multiple of the sector size (512 bytes),
	/// and the offset is specified in number of sectors (not number of bytes) from the beginning of the drive.
	/// 
	/// Returns the number of sectors (*not bytes*) that were successfully written to the drive.
	/// 
	/// # Note
	/// This is slow, as it uses blocking port I/O instead of DMA. 
	pub fn write_pio(&mut self, buffer: &[u8], offset_in_sectors: usize) -> Result<usize, &'static str> {
		if offset_in_sectors > self.size_in_blocks() {
			return Err("offset_in_sectors was out of bounds");
		}
		let length_in_bytes = buffer.len();
		if length_in_bytes % SECTOR_SIZE_IN_BYTES != 0 {
			return Err("The buffer length must be a multiple of sector size (512) bytes. ATA drives can only write at sector granularity.");
		}

		let lba_start = offset_in_sectors;
		let lba_end = lba_start + (length_in_bytes / SECTOR_SIZE_IN_BYTES);
		let sector_count = lba_end - lba_start;
		// trace!("AtaDrive::write_pio(): lba_start: {}, lba_end: {}, sector_count: {}",
		// 	lba_start, lba_end, sector_count,
		// );
		if sector_count > (self.identify_data.max_blocks_per_transfer as usize) {
			error!("AtaDrive::write_pio(): cannot write {} sectors, drive has a max of {} sectors per transfer.", 
				sector_count, self.identify_data.max_blocks_per_transfer
			);
			return Err("AtaDrive::write_pio(): cannot write more sectors than the drive's max");
		}

		self.bus.lock().write_pio(buffer, self.master_slave, lba_start, sector_count)
	}


	/// Returns `true` if this drive is the master, or `false` if it is the slave 
	/// on the IDE controller bus.
	pub fn is_master(&self) -> bool {
		match self.master_slave {
			BusDriveSelect::Master => true,
			BusDriveSelect::Slave => false,
		}
	}
}

impl StorageDevice for AtaDrive {
	fn size_in_blocks(&self) -> usize {
		if self.identify_data.user_addressable_sectors != 0 {
			self.identify_data.user_addressable_sectors as usize
		} else {
			self.identify_data.max_48_bit_lba as usize
		}
	}
}
impl BlockIo for AtaDrive {
	fn block_size(&self) -> usize { SECTOR_SIZE_IN_BYTES }
}
impl KnownLength for AtaDrive {
	fn len(&self) -> usize { self.block_size() * self.size_in_blocks() }
}
impl BlockReader for AtaDrive {
	fn read_blocks(&mut self, buffer: &mut [u8], block_offset: usize) -> Result<usize, IoError> {
		// TODO: emit a more specific IoError from the read_pio function itself instead of a blind conversion here
		self.read_pio(buffer, block_offset).map_err(|_e| IoError::InvalidInput)
	}
}
impl BlockWriter for AtaDrive {
	fn write_blocks(&mut self, buffer: &[u8], block_offset: usize) -> Result<usize, IoError> {
		// TODO: emit a more specific IoError from the read_pio function itself instead of a blind conversion here
		self.write_pio(buffer, block_offset).map_err(|_e| IoError::InvalidInput)
	}

	fn flush(&mut self) -> Result<(), IoError> { Ok(()) }
}

pub type AtaDriveRef = Arc<Mutex<AtaDrive>>;


/// A single IDE controller has two buses with up to two drives attached to each bus,
/// for a total of up to four drives. 
#[derive(Debug)]
pub struct IdeController {
	pub primary_master:   Option<AtaDriveRef>,
	pub primary_slave:    Option<AtaDriveRef>,
	pub secondary_master: Option<AtaDriveRef>,
	pub secondary_slave:  Option<AtaDriveRef>,
}

impl IdeController {
	/// Creates a new instance of an IDE controller based on the given PCI device.
	pub fn new(pci_device: &PciDevice) -> Result<IdeController, &'static str> {
		let primary_bus_data_port = match pci_device.bars[0] {
			0x0 | 0x1 => DEFAULT_PRIMARY_CHANNEL_DATA_PORT,
			other => {
				warn!("Untested rare condition: ATA drive PCI BAR0 was special address value: {:#X}", other);
				other as u16
			}
		};
		let primary_bus_control_port = match pci_device.bars[1] {
			0x0 | 0x1 => DEFAULT_PRIMARY_CHANNEL_CONTROL_PORT,
			other => {
				warn!("Untested rare condition: ATA drive PCI BAR1 was special address value: {:#X}", other);
				other as u16
			}
		};
		let secondary_bus_data_port = match pci_device.bars[2] {
			0x0 | 0x1 => DEFAULT_SECONDARY_CHANNEL_DATA_PORT,
			other => {
				warn!("Untested rare condition: ATA drive PCI BAR2 was special address value: {:#X}", other);
				other as u16
			}
		};
		let secondary_bus_control_port = match pci_device.bars[3] {
			0x0 | 0x1 => DEFAULT_SECONDARY_CHANNEL_CONTROL_PORT,
			other => {
				warn!("Untested rare condition: ATA drive PCI BAR3 was special address value: {:#X}", other);
				other as u16
			}
		};

		// TODO: use the BAR4 for DMA in the future
		let _bus_master_base = pci_device.bars[4]; 

		// Register interrupt handlers for the primary and secondary ATA buses.
		// They're not yet used for anything but will determine when a DMA transfer has completed.
		interrupts::register_interrupt(ATA_PRIMARY_IRQ, primary_ata_handler).map_err(|e| {
			error!("ATA Primary Bus IRQ {:#X} was already in use by handler {:#X}! Sharing IRQs is currently unsupported.", 
				ATA_PRIMARY_IRQ, e,
			);
			"ATA Primary Bus IRQ was already in use! Sharing IRQs is currently unsupported."
		})?;
		interrupts::register_interrupt(ATA_SECONDARY_IRQ, secondary_ata_handler).map_err(|e| {
			error!("ATA Secondary Bus IRQ {:#X} was already in use by handler {:#X}! Sharing IRQs is currently unsupported.", 
				ATA_SECONDARY_IRQ, e,
			);
			"ATA Secondary Bus IRQ was already in use! Sharing IRQs is currently unsupported."
		})?;

		let primary_bus = Arc::new(Mutex::new(AtaBus::new(primary_bus_data_port, primary_bus_control_port)));
		let secondary_bus = Arc::new(Mutex::new(AtaBus::new(secondary_bus_data_port, secondary_bus_control_port)));

		let primary_master   = AtaDrive::new(Arc::clone(&primary_bus), BusDriveSelect::Master);
		let primary_slave    = AtaDrive::new(primary_bus, BusDriveSelect::Slave);
		let secondary_master = AtaDrive::new(Arc::clone(&secondary_bus), BusDriveSelect::Master);
		let secondary_slave  = AtaDrive::new(secondary_bus, BusDriveSelect::Slave);
		
		let drive_fmt = |drive: &Result<AtaDrive, &str>| -> String {
			match drive {
				Ok(d)  => format!("drive initialized, size: {} sectors", d.size_in_blocks()),
				Err(e) => e.to_string(),
			}
		};

		info!("ATA drive controller at {}: \n\
			--> primary master:   {} \n\
			--> primary slave:    {} \n\
			--> secondary master: {} \n\
			--> secondary slave:  {}",
			pci_device.location,
			drive_fmt(&primary_master),
			drive_fmt(&primary_slave),
			drive_fmt(&secondary_master),
			drive_fmt(&secondary_slave),
		);

		Ok( IdeController {
			primary_master:   primary_master.ok().map(|d| Arc::new(Mutex::new(d))),
			primary_slave:    primary_slave.ok().map(|d| Arc::new(Mutex::new(d))),
			secondary_master: secondary_master.ok().map(|d| Arc::new(Mutex::new(d))),
			secondary_slave:  secondary_slave.ok().map(|d| Arc::new(Mutex::new(d))),
		})
	}

	/// Returns an `Iterator` over all of the `AtaDrive`s 
	/// that exist (and are supported) in this `IdeController`.
	/// The order of iteration is: 
	/// primary master, primary slave, secondary master, and secondary slave;
	/// any devices that do not exist are skipped.
	pub fn iter(&self) -> IdeControllerIter {
		IdeControllerIter {
			next: NextDrive::PrimaryMaster,
			controller: self,
		}
	}
}

impl StorageController for IdeController {
    fn devices<'c>(&'c self) -> Box<(dyn Iterator<Item = StorageDeviceRef> + 'c)> {
		Box::new(
			self.iter().map(|ata_drive_ref| Arc::clone(ata_drive_ref) as StorageDeviceRef)
		)
	}
}

/// The order in which `AtaDrive`s in an `IdeController` are iterated over.
#[derive(Clone)]
enum NextDrive {
	PrimaryMaster,
	PrimarySlave,
	SecondaryMaster,
	SecondarySlave,
}

/// Provides an iterator over all `AtaDrive`s in an `IdeController`.
/// See the [`IdeController::iter()`](struct.IdeController.html#method.iter) method.
#[derive(Clone)]
pub struct IdeControllerIter<'c> {
	/// which drive will be returned on the next call to `next()`
	next: NextDrive,
	controller: &'c IdeController,
}
impl<'c> Iterator for IdeControllerIter<'c> {
	type Item = &'c AtaDriveRef;


    fn next(&mut self) -> Option<Self::Item> {
		match self.next {
			NextDrive::PrimaryMaster => {
				self.next = NextDrive::PrimarySlave;
				if self.controller.primary_master.is_some() {
					self.controller.primary_master.as_ref()
				} else {
					self.next()
				}
			}
			NextDrive::PrimarySlave => {
				self.next = NextDrive::SecondaryMaster;
				if self.controller.primary_slave.is_some() {
					self.controller.primary_slave.as_ref()
				} else {
					self.next()
				}
			}
			NextDrive::SecondaryMaster => {
				self.next = NextDrive::SecondarySlave;
				if self.controller.secondary_master.is_some() {
					self.controller.secondary_master.as_ref()
				} else {
					self.next()
				}
			}
			NextDrive::SecondarySlave => {
				if self.controller.secondary_slave.is_some() {
					self.controller.secondary_slave.as_ref()
				} else {
					None
				}
			}
		}
	}
}


/// The primary ATA interrupt is connected to IRQ 0xE by default.
/// Because we perform the typical PIC remapping, the remapped IRQ vector number is 0x2E.
const ATA_PRIMARY_IRQ:   u8 = interrupts::IRQ_BASE_OFFSET + 0xE;
/// The secondary ATA interrupt is connected to IRQ 0xF by default.
/// Because we perform the typical PIC remapping, the remapped IRQ vector number is 0x2F.
const ATA_SECONDARY_IRQ: u8 = interrupts::IRQ_BASE_OFFSET + 0xF;

/// The primary ATA interrupt handler. Not yet used for anything, but useful for DMA.
extern "x86-interrupt" fn primary_ata_handler(_stack_frame: InterruptStackFrame ) {
    info!("Primary ATA Interrupt ({:#X})", ATA_PRIMARY_IRQ);
    interrupts::eoi(Some(ATA_PRIMARY_IRQ));
}

/// The primary ATA interrupt handler. Not yet used for anything, but useful for DMA.
extern "x86-interrupt" fn secondary_ata_handler(_stack_frame: InterruptStackFrame ) {
    info!("Secondary ATA Interrupt ({:#X})", ATA_SECONDARY_IRQ);
    interrupts::eoi(Some(ATA_SECONDARY_IRQ));
}


/// Information that describes an ATA drive, 
/// obtained from the response to an identify command.
/// 
/// Fuller documentation is available here:
/// <https://docs.microsoft.com/en-us/windows-hardware/drivers/ddi/content/ata/ns-ata-_identify_device_data#members>
#[derive(Copy, Clone, Debug, Default)]
#[repr(packed)]
pub struct AtaIdentifyData {
	pub general_configuration: u16,
	pub num_cylinders: u16,
	pub specific_configuration: u16,
	pub num_heads: u16,
	_reserved1: [u16; 2],
	pub num_sectors_per_track: u16,
	pub vendor_unique1: [u16; 3],
	pub serial_number: AtaSerialNumber,
	_reserved2: [u16; 3],
	pub firmware_version: AtaFirmwareVersion,
	pub model_number: AtaModelNumber,
	/// Maximum number of blocks per transfer.
	/// Sometimes referred to as "sectors per int".
	pub max_blocks_per_transfer: u8,
	pub vendor_unique2: u8,
	pub trusted_computing: u16,
	pub capabilities: u16,
	_reserved3: u16, // reserved word 50
	_reserved4: [u16; 2],
	/// A bitmask of translation fields valid and free fall control sensitivity
	pub translation_fields_valid: u8,
	pub free_fall_control_sensitivity: u8,
	pub num_current_cylinders: u16,
	pub num_current_heads: u16,
	pub current_sectors_per_track: u16,
	pub current_sector_capacity: u32, 
	pub current_multi_sector_setting: u8,
	/// MultiSectorSettingValid : 1;
	/// ReservedByte59 : 3;
	/// SanitizeFeatureSupported : 1;
	/// CryptoScrambleExtCommandSupported : 1;
	/// OverwriteExtCommandSupported : 1;
	/// BlockEraseExtCommandSupported : 1;
	pub ext_command_supported: u8,
	/// Number of sectors in the drive, if using 28-bit LBA. 
	/// This can be used to calculate the size of the drive.
	/// If zero, we're using 48-bit LBA, so you should use `max_48_bit_lba`.
	pub user_addressable_sectors: u32,
	_reserved5: u16,
	pub multiword_dma_support: u8,
	pub multiword_dma_active: u8,
	pub advanced_pio_modes: u8,
	_reserved6: u8,
	pub minimum_mw_transfer_cycle_time: u16,
	pub recommended_mw_transfer_cycle_time: u16,
	pub minimum_pio_cycle_time: u16,
	pub minimum_pio_cycle_time_io_ready: u16,
	pub additional_supported: u16,
	_reserved7: [u16; 5],
	/// only the first 5 bits are used, others are reserved
	pub queue_depth: u16,
	pub serial_ata_capabilities: u32,
	pub serial_ata_features_supported: u16,
	pub serial_ata_features_enabled: u16,
	pub major_revision: u16,
	pub minor_revision: u16,
	pub command_set_support: [u16; 3],
	pub command_set_active: [u16; 3],
	pub ultra_dma_support: u8,
	pub ultra_dma_active: u8,
	pub normal_security_erase_unit: u16,
	pub enhanced_security_erase_unit: u16,
	pub current_apm_level: u8,
	_reserved8: u8,
	pub master_password_id: u16,
	pub hardware_reset_result: u16,
	pub current_acoustic_value: u8,
	pub recommended_acoustic_value: u8,
	pub stream_min_request_size: u16,
	pub streaming_transfer_time_dma: u16,
	pub streaming_access_latency_dma_pio: u16,
	pub streaming_perf_granularity: u32, 
	/// Number of sectors in the drive, if using 48-bit LBA. 
	/// This can be used to calculate the size of the drive.
	pub max_48_bit_lba: u64,
	pub streaming_transfer_time: u16,
	pub dsm_cap: u16,
	/// `[0:3]` Physical sector size (in logical sectors)
	pub physical_logical_sector_size: u16, 
	pub inter_seek_delay: u16,
	pub world_wide_name: [u16; 4],
	pub reserved_for_world_wide_name_128: [u16; 4],
	pub reserved_for_tlc_technical_report: u16,
	pub words_per_logical_sector: u32,
	pub command_set_support_ext: u16,
	pub command_set_active_ext: u16,
	pub reserved_for_expanded_support_and_active: [u16; 6],
	pub msn_support: u16,
	pub security_status: u16,
	_reserved9: [u16; 31],
	pub cfa_power_mode1: u16,
	_reserved10: [u16; 7],
	pub nominal_form_factor: u16, 
	pub data_set_management_feature: u16, 
	pub additional_product_id: [u16; 4],
	_reserved11: [u16; 2],
	pub current_media_serial_number: [u16; 30],
	pub sct_command_transport: u16,
	_reserved12: [u16; 2],
	pub block_alignment: u16, 
	pub write_read_verify_sector_count_mode_3_only: [u16; 2],
	pub write_read_verify_sector_count_mode_2_only: [u16; 2],
	pub nv_cache_capabilities: u16,
	pub nv_cache_size_lsw: u16,
	pub nv_cache_size_msw: u16,
	pub nominal_media_rotation_rate: u16,
	_reserved13: u16, 
	pub nv_cache_time_to_spin_up_in_seconds: u8,
	_reserved14: u8,
	pub write_read_verify_sector_count_mode: u8,
	_reserved15: u8,
	_reserved16: u16,
	pub transport_major_version: u16,
	pub transport_minor_version: u16,
	_reserved17: [u16; 6],
	pub extended_num_of_user_addressable_sectors: u64,
	pub min_blocks_per_download_microcode: u16,
	pub max_blocks_per_download_microcode: u16,
	_reserved18: [u16; 19],
	pub signature: u8,
	pub checksum: u8,
}

impl AtaIdentifyData {
	/// Converts the given byte array, which should be the result of an ATA identify command,
	/// into a struct that contains the identified details of an ATA drive.
	fn new(arr: [u8; SECTOR_SIZE_IN_BYTES])-> AtaIdentifyData {
		let mut identify_data: AtaIdentifyData = unsafe { core::mem::transmute(arr) };
		Self::flip_bytes(&mut identify_data.serial_number.0);
		Self::flip_bytes(&mut identify_data.firmware_version.0);
		Self::flip_bytes(&mut identify_data.model_number.0);
		identify_data
	}

	/// Flips pairs of bytes to rectify quasi-endianness issues in the ATA identify response.
	fn flip_bytes(bytes: &mut [u8]) {
		for pair in bytes.chunks_mut(2) {
			pair.swap(0, 1);
		}
	}
}


/// An ATA drive's serial number is a 20-byte string.
/// 
/// This is a wrapper around a byte string `[u8; 20]`, because Rust only supports deriving traits
/// like `Debug` and `Default` for arrays up to 32 elements.
#[derive(Copy, Clone, Default)]
#[repr(packed)]
pub struct AtaSerialNumber([u8; 20]);

impl fmt::Display for AtaSerialNumber {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		core::str::from_utf8(&self.0)
			.map_err(|_| fmt::Error)
			.and_then(|s| write!(f, "{s}"))
	}
}
impl fmt::Debug for AtaSerialNumber {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "\"{self}\"")
	}
}


/// An ATA drive's model number is a 40-byte string.
///
/// A wrapper around a byte string `[u8; 40]`, because Rust only supports deriving traits
/// like `Debug` and `Default` for arrays up to 32 elements.
#[derive(Copy, Clone)]
#[repr(packed)]
pub struct AtaModelNumber([u8; 40]);
impl Default for AtaModelNumber {
	fn default() -> Self { 
		AtaModelNumber([0; 40])
	}
}
impl fmt::Display for AtaModelNumber {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		core::str::from_utf8(&self.0)
			.map_err(|_| fmt::Error)
			.and_then(|s| write!(f, "{s}"))
	}
}
impl fmt::Debug for AtaModelNumber {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "\"{self}\"")
	}
}


/// An ATA drive's firmware version is an 8-byte string.
///
/// A wrapper around a byte string `[u8; 8]` to allow it to be printed.
#[derive(Copy, Clone, Default)]
#[repr(packed)]
pub struct AtaFirmwareVersion([u8; 8]);

impl fmt::Display for AtaFirmwareVersion {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		core::str::from_utf8(&self.0)
			.map_err(|_| fmt::Error)
			.and_then(|s| write!(f, "{s}"))
	}
}
impl fmt::Debug for AtaFirmwareVersion {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "\"{self}\"")
	}
}
