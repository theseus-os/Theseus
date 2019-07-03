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

const SECTOR_SIZE_IN_BYTES: usize = 512;

const DEFAULT_PRIMARY_CHANNEL_DATA_PORT:         u16 = 0x1F0;
const DEFAULT_PRIMARY_CHANNEL_CONTROL_PORT:      u16 = 0x3F6;
const DEFAULT_SECONDARY_CHANNEL_DATA_PORT:       u16 = 0x170;
const DEFAULT_SECONDARY_CHANNEL_CONTROL_PORT:    u16 = 0x376;

const MAX_LBA_28_VALUE: u64 = (1 << 28) - 1;

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

/// The possible commands that can be issued to an ATA drive's command port. 
/// More esoteric commands (nearly a full list) are here: <https://wiki.osdev.org/ATA_Command_Matrix>.
#[repr(u8)]
pub enum AtaCommand {
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
	/// Get identifying details of an ATA drive.
	IdentifyDevice  = 0xEC,
	/// Get identifying details of an ATAPI drive.
	IdentifyPacket  = 0xA1,
}


/// The possible types of drive devices that can be attached via ATA.
pub enum AtaDeviceType {
	/// A parallel ATA (PATA) drive, like a hard drive.
	/// This is the type previously known as just "ATA" before SATA existed.
	Pata,
	/// A parallel ATA (PATA) drive that uses the packet interface,
	/// like a CD-ROM drive.
	PataPi,
	/// A serial ATA (SATA) drive, like a hard drive (newer than PATA).
	Sata,
	/// A serial ATA (SATA) drive that uses the packet interface,
	/// like a hard drive (newer than PATA).
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
	lba_low: Port<u8>,
	/// The middle byte `[8:16)`of the linear block address (LBA) of the sector that we want to read or write. 
	/// Located at `BAR0 + 4`.
	lba_mid: Port<u8>,
	/// The high byte `[16:24)` of the linear block address (LBA) of the sector that we want to read or write. 
	/// Located at `BAR0 + 5`.
	lba_high: Port<u8>,
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
			lba_low: Port::new(data_bar + 3),
			lba_mid: Port::new(data_bar + 4),
			lba_high: Port::new(data_bar + 5),
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

		// Check to see that the drive supports LBA, since we don't support the ancient CHS addressing scheme.
		if drive.identify_data.capabilities & 0x200 == 0 {
			return Err("drive is an ancient CHS device that doesn't support LBA addressing mode, but we don't support CHS.");
		}

		Ok(drive)
	}

	/// Reads data from this drive and places it into the provided `buffer`.
    /// The length of the given `buffer` determines the maximum number of bytes to be read.
	/// 
	/// Returns the number of bytes that were successfully read from the drive
	/// and copied into the given `buffer`.
	/// 
	/// # Note
	/// This is slow, as it uses blocking port I/O instead of DMA. 
	pub fn read_pio(&mut self, buffer: &mut [u8], offset: usize) -> Result<usize, &'static str> {
		// Calculate LBA and sector count based on the offset and requested read length
		let (lba_start, lba_end, offset_remainder) = self.lba_bounds(offset, buffer.len())?;
		let sector_count = lba_end - lba_start;
		trace!("AtaDrive::read_pio(): lba_start: {}, lba_end: {}, sector_count: {}, offset_remainder: {}",
			lba_start, lba_end, sector_count, offset_remainder
		);
		if sector_count > (self.identify_data.max_blocks_per_transfer as u64) {
			error!("AtaDrive::read_pio(): cannot read {} sectors ({} bytes), drive has a max of {} sectors per transfer.", 
				sector_count, buffer.len(), self.identify_data.max_blocks_per_transfer
			);
			return Err("AtaDrive::read_pio(): cannot read more sectors than the drive's max");
		}

		// Set up and issue the read command.
		if lba_start > MAX_LBA_28_VALUE {
			// Using 48-bit LBA. 
			// The high bytes of the sector_count and LBA must be written *before* the low bytes.
			unsafe {
				self.drive_select.write(0x40 | (self.master_slave as u8));
				// write the high bytes
				self.sector_count.write((sector_count >> 8) as u8);
				self.lba_high.write((lba_start >> 40) as u8);
				self.lba_mid.write( (lba_start >> 32) as u8);
				self.lba_low.write( (lba_start >> 24) as u8);
				// write the low bytes
				self.sector_count.write(sector_count as u8);
				self.lba_high.write((lba_start >> 16) as u8);
				self.lba_mid.write( (lba_start >>  8) as u8);
				self.lba_low.write( (lba_start >>  0) as u8);
				self.command.write(AtaCommand::ReadPioExt as u8);
			}
		} else {
			// Using 28-bit LBA.
			unsafe {
				// bits [24:28] of the LBA need to go into the lower 4 bits of the `drive_select` port.
				self.drive_select.write(0xE0 | (self.master_slave as u8) | ((lba_start >> 24) as u8 & 0x0F));
				self.sector_count.write(sector_count as u8);
				self.lba_high.write((lba_start >> 16) as u8);
				self.lba_mid.write( (lba_start >>  8) as u8);
				self.lba_low.write( (lba_start >>  0) as u8);
				self.command.write(AtaCommand::ReadPio as u8);
			}
		}

		self.wait_for_ready().map_err(|_| "error before data read")?;

		// Read the actual data, one sector at a time.
		let mut src_offset = offset_remainder; 
		let mut dest_offset = 0;
		for _lba in lba_start..lba_end {
			let sector = self.internal_read_sector()?;
			// don't copy past the end of `buffer`
			let bytes_to_copy = core::cmp::min(SECTOR_SIZE_IN_BYTES - src_offset, buffer.len() - dest_offset);
			buffer[dest_offset .. (dest_offset + bytes_to_copy)].copy_from_slice(&sector[src_offset .. (src_offset + bytes_to_copy)]);
			trace!("LBA {}: copied bytes into buffer[{}..{}] from sector[{}..{}]",
				_lba, dest_offset, dest_offset + bytes_to_copy, src_offset, src_offset + bytes_to_copy,
			);
			dest_offset += bytes_to_copy;
			src_offset = 0;
		}

		trace!("read_pio(): status after data read: {:?}", self.status());
		self.wait_for_ready().map_err(|_| "error after data read")?;
		Ok(dest_offset)
	}

	/// Writes data from the provided `buffer` to this drive, starting at the given `offset` into the drive.
    /// The length of the given `buffer` determines the number of bytes to be written.
	/// 
	/// As content is written to the drive at sector granularity, 
	/// both the offset and the buffer length must be a multiple of the sector size (512 bytes). 
	/// 
	/// Returns the number of bytes that were successfully written to the drive.
	/// 
	/// # Note
	/// This is slow, as it uses blocking port I/O instead of DMA. 
	pub fn write_pio(&mut self, buffer: &[u8], offset: usize) -> Result<usize, &'static str> {
		if buffer.len() % SECTOR_SIZE_IN_BYTES != 0 {
			return Err("The buffer length must be a multiple of sector size (512) bytes. ATA drives can only write at sector granularity.");
		}
		if offset % SECTOR_SIZE_IN_BYTES != 0 {
			return Err("The offset must be a multiple of sector size (512) bytes. ATA drives can only write at sector granularity.");
		}

		// Calculate LBA and sector count based on the offset and requested length
		let (lba_start, lba_end, offset_remainder) = self.lba_bounds(offset, buffer.len())?;
		let sector_count = lba_end - lba_start;
		trace!("AtaDrive::write_pio(): lba_start: {}, lba_end: {}, sector_count: {}, offset_remainder: {}",
			lba_start, lba_end, sector_count, offset_remainder
		);
		if sector_count > (self.identify_data.max_blocks_per_transfer as u64) {
			error!("AtaDrive::write_pio(): cannot write {} sectors ({} bytes), drive has a max of {} sectors per transfer.", 
				sector_count, buffer.len(), self.identify_data.max_blocks_per_transfer
			);
			return Err("AtaDrive::write_pio(): cannot read more sectors than the drive's max");
		}

		// Use 28-bit LBAs, unless the LBA is too large, then we use 48-bit LBAs
		let using_lba_28 = lba_start <= MAX_LBA_28_VALUE;

		self.wait_for_ready().map_err(|_| "error before issuing write command")?;

		// Set up and issue the write command.
		if using_lba_28 {
			unsafe {
				// bits [24:28] of the LBA need to go into the lower 4 bits of the `drive_select` port.
				self.drive_select.write(0xE0 | (self.master_slave as u8) | ((lba_start >> 24) as u8 & 0x0F));
				self.sector_count.write(sector_count as u8);
				self.lba_high.write((lba_start >> 16) as u8);
				self.lba_mid.write( (lba_start >>  8) as u8);
				self.lba_low.write( (lba_start >>  0) as u8);
				self.command.write(AtaCommand::WritePio as u8);
			}
		} else {
			// When using 48-bit LBAs, the high bytes of the sector_count and LBA must be written *before* the low bytes.
			unsafe {
				self.drive_select.write(0x40 | (self.master_slave as u8));
				// write the high bytes
				self.sector_count.write((sector_count >> 8) as u8);
				self.lba_high.write((lba_start >> 40) as u8);
				self.lba_mid.write( (lba_start >> 32) as u8);
				self.lba_low.write( (lba_start >> 24) as u8);
				// write the low bytes
				self.sector_count.write(sector_count as u8);
				self.lba_high.write((lba_start >> 16) as u8);
				self.lba_mid.write( (lba_start >>  8) as u8);
				self.lba_low.write( (lba_start >>  0) as u8);
				self.command.write(AtaCommand::WritePioExt as u8);
			}
		}

		self.wait_for_ready().map_err(|_| "error before data write")?;

		// Write the actual data.
		// ATA PIO works by writing one 16-bit word at a time, 
		// so one write covers two bytes of the buffer.
		let mut bytes_written = 0;
		for chunk in buffer.chunks_exact(2) {
			self.wait_for_ready().map_err(|_| "error during data write")?;
			let word = (chunk[1] as u16) << 8 | (chunk[0] as u16);
			unsafe { self.data.write(word); }
			bytes_written += 2;
		}

		self.wait_for_ready().map_err(|_| "error after data write")?;

		// Flush the drive's cache after each write command
		let cache_flush_cmd = if using_lba_28 { AtaCommand::CacheFlush } else { AtaCommand::CacheFlushExt };
		unsafe { self.command.write(cache_flush_cmd as u8) };

		self.wait_for_ready().map_err(|_| "error after cache flush after data write")?;
		Ok(bytes_written)
	}


	/// Translates bounds info into an LBA, sector count, and first sector offset.
	/// 
	/// # Arguments
	/// * `offset`: the absolute byte offset from the beginning of the drive at which the read/write starts.
	/// * `length`: the number of bytes to be read/written.
	/// 
	/// # Return
	/// Returns a tuple of the following information:
	/// * the first LBA (sector number), i.e., where the transfer should start,
	/// * the last LBA (sector number), i.e., the LBA where the transfer should end (exclusive bound),
	/// * the offset remainder, which is the offset into the first sector where the read should start.
	/// 
	/// The number of sectors to be transferred is `num_sectors = last LBA - first LBA`.
	/// 
	/// Returns an error if the `offset + length` extends past the bounds of this drive.
	fn lba_bounds(&self, offset: usize, length: usize) -> Result<(u64, u64, usize), &'static str> {
		if offset > self.size_in_bytes() {
			return Err("offset was out of bounds");
		}
		let starting_lba = (offset / SECTOR_SIZE_IN_BYTES) as u64;
		let offset_remainder = (offset % SECTOR_SIZE_IN_BYTES) as usize;
		let ending_lba = core::cmp::min(
			self.size_in_sectors() as u64,
			((offset + length + SECTOR_SIZE_IN_BYTES - 1) / SECTOR_SIZE_IN_BYTES) as u64, // round up to next sector
		);
		trace!("lba_bounds: offset: {}, length: {}, starting_lba: {}, ending_lba: {}", offset, length, starting_lba, ending_lba);
		Ok((starting_lba, ending_lba, offset_remainder))
	}


	/// Returns the number of sectors in this drive.
	pub fn size_in_sectors(&self) -> usize {
		if self.identify_data.user_addressable_sectors != 0 {
			self.identify_data.user_addressable_sectors as usize
		} else {
			self.identify_data.max_48_bit_lba as usize
		}
	}

	/// Returns the size of this drive in bytes,
	/// rounded up to the nearest sector size.
	pub fn size_in_bytes(&self) -> usize {
		self.size_in_sectors() * SECTOR_SIZE_IN_BYTES
	}


	/// Issues an ATA identify command to probe the drive
	/// and query its characteristics. 
	/// 
	/// See this link: <https://wiki.osdev.org/ATA_PIO_Mode#IDENTIFY_command>
	fn identify_drive(&mut self) -> Result<AtaIdentifyData, &'static str> {
		unsafe {
			self.drive_select.write(0xA0 | self.master_slave as u8);
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
			Some(AtaDeviceType::SataPi) => return Err("drive was an unsupported PATAPI device"),
			_                           => return Err("drive was an unknown device type"),
		};

		self.wait_for_ready().map_err(|_| "error reading identify data from the drive")?;

		// we're ready to read the actual data
		let arr = self.internal_read_sector()?;
		Ok(AtaIdentifyData::new(arr))
    }

	/// Performs the actual read operation once the `LBA` and other ports have been set up. 
	///
	/// This function reads one sector of data from the drive and returns it.
	fn internal_read_sector(&mut self) -> Result<[u8; SECTOR_SIZE_IN_BYTES], &'static str> {
		// ATA PIO works by reading one 16-bit word at a time, 
		// so one read covers two bytes of the buffer.
		// Also, we *MUST* read a full sector for the drive to continue working properly,
		// even if we don't need that many bytes to fill the given `buffer`.
		let mut data = [0u8; SECTOR_SIZE_IN_BYTES];
		for chunk in data.chunks_exact_mut(2) {
			self.wait_for_ready().map_err(|_| "error during data read")?;
			let word: u16 = self.data.read();
			chunk[0] = word as u8;
			chunk[1] = (word >> 8) as u8;
		}
		if self.status().intersects(AtaStatus::ERROR) {
			error!("internal_read_sector: status: {:?}, error: {:?}", self.status(), self.error());
			return Err("error after data read");
		}
		Ok(data)
	}

	/// Performs a blocking poll that reads the drive's status 
	/// until it is no longer busy and it is ready
	/// (`AtaStatus::BUSY` is `0` and `AtaStatus::DRIVE_READY` is `1`).
	/// 
	/// Returns an error if the `status` port indicates an error. 
	/// The `error` port can then be read to obtain more details on what kind of error occurred.
	fn wait_for_ready(&self) -> Result<(), ()> {
		let mut loop_counter = 0;
		loop {
			let status = self.status();
			loop_counter += 1;
			if status.intersects(AtaStatus::ERROR | AtaStatus::DRIVE_WRITE_FAULT) {
				return Err(());
			}
			if status.intersects(AtaStatus::BUSY) { 
				// if loop_counter % 100 == 0 {
					warn!("AtaDrive::status() has been busy waiting for a long time... is there a drive problem? (status: {:?})", status);
				// }
				continue;
			}
			if status.intersects(AtaStatus::DRIVE_READY) {
				return Ok(()); // ready to go!
			}
		}
	}


	/// Issues a software reset for this drive.
	pub fn software_reset(&mut self) {
		// The procedure is to first set the SRST bit, 
		// then to wait 5 microseconds,
		// the to clear the SRST bit.
		unimplemented!()
	}

	
	/// Reads the `status` port and returns the value as an `AtaStatus` bitfield. 
	/// Because some disk drives operate (change wire values) very slowly,
	/// this undergoes the standard procedure of reading the port value 
	/// and discarding it 4 times before reading the real value. 
	/// Each read is a 100ns delay, so the total delay of 400ns is proper.
	pub fn status(&self) -> AtaStatus {
		let _ = self.status.read();
		let _ = self.status.read();
		let _ = self.status.read();
		let _ = self.status.read();
		AtaStatus::from_bits_truncate(self.status.read())
	}


	/// Reads the `error` port and returns the value as an `AtaError` bitfield.
	pub fn error(&self) -> AtaError {
		AtaError::from_bits_truncate(self.error.read())
	}
}



/// A single ATA controller has two buses with up to two drives attached to each bus,
/// for a total of up to four drives. 
pub struct AtaController {
	pub primary_master:    Option<AtaDrive>,
	pub primary_slave:     Option<AtaDrive>,
	pub secondary_master:  Option<AtaDrive>,
	pub secondary_slave:   Option<AtaDrive>,
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

		// TODO: use the BAR4 for DMA in the future
		let _bus_master_base = pci_device.bars[4]; 

		let primary_master   = AtaDrive::new(primary_channel_data_port, primary_channel_control_port, BusDriveSelect::Master);
		let primary_slave    = AtaDrive::new(primary_channel_data_port, primary_channel_control_port, BusDriveSelect::Slave);
		let secondary_master = AtaDrive::new(secondary_channel_data_port, secondary_channel_control_port, BusDriveSelect::Master);
		let secondary_slave  = AtaDrive::new(secondary_channel_data_port, secondary_channel_control_port, BusDriveSelect::Slave);
		
		debug!("Primary master: {:#X?}", primary_master);
		debug!("Primary slave: {:#X?}", primary_slave);
		debug!("Secondary master: {:#X?}", secondary_master);
		debug!("Secondary slave: {:#X?}", secondary_slave);

		Ok( AtaController {
			primary_master: primary_master.ok(),
			primary_slave: primary_slave.ok(),
			secondary_master: secondary_master.ok(),
			secondary_slave: secondary_slave.ok(),
		})
	}
}


/// Information that describes an ATA drive, 
/// obtained from the response to an identify command.
/// 
/// Fuller documentation is available here:
/// <https://docs.microsoft.com/en-us/windows-hardware/drivers/ddi/content/ata/ns-ata-_identify_device_data#members
#[derive(Copy, Clone, Debug, Default)]
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
	model_number: AtaModelNumber,
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
	/// Number of sectors in the disk, if using 28-bit LBA. 
	/// This can be used to calculate the size of the disk.
	/// If zero, we're using 48-bit LBA, so you should use `max_48_bit_lba`.
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
	/// Number of sectors in the disk, if using 48-bit LBA. 
	/// This can be used to calculate the size of the disk.
	max_48_bit_lba: u64,
	streaming_transfer_time: u16,
	dsm_cap: u16,
	/// `[0:3]` Physical sector size (in logical sectors)
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
	/// Converts the given byte array, which should be the result of an ATA identify command,
	/// into a struct that contains the identified details of an ATA drive.
	fn new(arr: [u8; SECTOR_SIZE_IN_BYTES])-> AtaIdentifyData {
		let mut identify_data: AtaIdentifyData = unsafe { core::mem::transmute(arr) };
		Self::flip_bytes(&mut identify_data.serial_number);
		Self::flip_bytes(&mut identify_data.firmware_version);
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

#[derive(Copy, Clone)]
#[repr(packed)]
pub struct AtaModelNumber([u8; 40]);
impl Default for AtaModelNumber {
	fn default() -> Self { 
		AtaModelNumber([0; 40])
	}
}
impl fmt::Debug for AtaModelNumber {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		core::str::from_utf8(&self.0)
			.map_err(|_| fmt::Error)
			.and_then(|s| core::fmt::Write::write_str(f, s))
	}
}
