//! Support for accessing ATA disks (IDE).

#![no_std]

#[macro_use] extern crate log;
extern crate spin;
extern crate port_io;
extern crate pci;


use spin::{Once, Mutex}; 
use port_io::Port;
use pci::PciDevice;



const DEFAULT_PRIMARY_CHANNEL_DATA_PORT:         u16 = 0x1F0;
const DEFAULT_PRIMARY_CHANNEL_CONTROL_PORT:      u16 = 0x3F6;
const DEFAULT_SECONDARY_CHANNEL_DATA_PORT:       u16 = 0x170;
const DEFAULT_SECONDARY_CHANNEL_CONTROL_PORT:    u16 = 0x376;



/// 
pub struct AtaDrive {
	data_port: Port<u16>,
	control_port: Port<u8>,
}


pub fn init_drive(pci_device: &PciDevice) -> AtaDrive {
	let primary_channel_data_port = match pci_device.bars[0] {
		0x0 | 0x1 => DEFAULT_PRIMARY_CHANNEL_DATA_PORT,
		other => {
			warn!("Untested: ATA drive PCI BAR0 was special address value: {:#X}", other);
			other as u16
		}
	};
	let primary_channel_control_port = match pci_device.bars[1] {
		0x0 | 0x1 => DEFAULT_PRIMARY_CHANNEL_CONTROL_PORT,
		other => {
			warn!("Untested: ATA drive PCI BAR1 was special address value: {:#X}", other);
			other as u16
		}
	};
	let secondary_channel_data_port = match pci_device.bars[2] {
		0x0 | 0x1 => DEFAULT_SECONDARY_CHANNEL_DATA_PORT,
		other => {
			warn!("Untested: ATA drive PCI BAR0 was special address value: {:#X}", other);
			other as u16
		}
	};
	let secondary_channel_control_port = match pci_device.bars[3] {
		0x0 | 0x1 => DEFAULT_SECONDARY_CHANNEL_CONTROL_PORT,
		other => {
			warn!("Untested: ATA drive PCI BAR1 was special address value: {:#X}", other);
			other as u16
		}
	};

	let bus_master_base = pci_device.bars[4];

	unimplemented!()
}


