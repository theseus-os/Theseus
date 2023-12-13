//! This application lists currently connected PCI devices.

#![no_std]

extern crate alloc;
#[macro_use] extern crate app_io;
extern crate getopts;

use alloc::vec::Vec;
use alloc::string::String;
use getopts::Options;
use pci::pci_device_iter;
use memory::PhysicalAddress;

pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");

    let matches = match opts.parse(args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(opts);
            return -1; 
        }
    };

    if matches.opt_present("h") {
        print_usage(opts);
        return 0;
    }

    if let Err(msg) = list_pci_devices() {
        println!("Error: {}", msg);
    }

    0
}

fn list_pci_devices() -> Result<(), &'static str> {
    for dev in pci_device_iter()? {
        println!("{} -- {:04x}:{:04x}", dev.location, dev.vendor_id, dev.device_id);
        println!("- class, subclass, prog_if: {:x}, {:x}, {:x}", dev.class, dev.subclass, dev.prog_if);

        for bar_idx in 0..6 {
            let base = dev.determine_mem_base(bar_idx)?;
            if base != PhysicalAddress::zero() {
                let size = dev.determine_mem_size(bar_idx);
                println!("- BAR {}: base = 0x{:x}, size = 0x{:x}", bar_idx, base, size);
            }
        }

        let (msi, msix) = dev.modern_interrupt_support();
        let supports = |b| match b {
            true => "supported",
            false => "not supported",
        };

        println!("- MSI interrupts: {}", supports(msi));
        println!("- MSI-X interrupts: {}", supports(msix));
        println!("- INTx enabled: {}", dev.pci_intx_enabled());
        println!("- INTx status: {}", dev.pci_get_intx_status(false));
    }

    Ok(())
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &str = "Usage: lspci
An application which lists currently connected PCI devices.";
