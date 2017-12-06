// modified from the toyos pic8259_simple crate

//! Support for the 8259 Programmable Interrupt Controller, which handles
//! basic I/O interrupts.  In multicore mode, we would apparently need to
//! replace this with an APIC interface.
//!
//! The basic idea here is that we have two PIC chips, PIC1 and PIC2, and
//! that PIC2 is slaved to interrupt 2 on PIC 1.  You can find the whole
//! story at http://wiki.osdev.org/PIC (as usual).  Basically, our
//! immensely sophisticated modern chipset is engaging in early-80s
//! cosplay, and our goal is to do the bare minimum required to get
//! reasonable interrupts.
//!
//! The most important thing we need to do here is set the base "offset"
//! for each of our two PICs, because by default, PIC1 has an offset of
//! 0x8, which means that the I/O interrupts from PIC1 will overlap
//! processor interrupts for things like "General Protection Fault".  Since
//! interrupts 0x00 through 0x1F are reserved by the processor, we move the
//! PIC1 interrupts to 0x20-0x27 and the PIC2 interrupts to 0x28-0x2F.  If
//! we wanted to write a DOS emulator, we'd presumably need to choose
//! different base interrupts, because DOS used interrupt 0x21 for system
//! calls.

#![feature(const_fn)]
#![no_std]

extern crate port_io;

use core::fmt;


/// Command sent to read the Interrupt Request Register.
const CMD_IRR: u8 = 0x0A;

/// Command sent to read the In-Service Register.
const CMD_ISR: u8 = 0x0B;

/// Command sent to begin PIC initialization.
const CMD_INIT: u8 = 0x11;

/// Command sent to acknowledge an interrupt.
const CMD_END_OF_INTERRUPT: u8 = 0x20;

// The mode in which we want to run our PICs.
const MODE_8086: u8 = 0x01;


/// Each PIC chip has two interrupt status registers: 
/// the In-Service Register (ISR) and the Interrupt Request Register (IRR). 
/// The ISR tells us which interrupts are currently being serviced, meaning IRQs sent to the CPU. 
/// The IRR tells us which interrupts have been raised, but not necessarily serviced yet. 
/// Based on the interrupt mask, the PIC will send interrupts from the IRR to the CPU, 
/// at which point they are marked in the ISR.
/// see http://wiki.osdev.org/8259_PIC#ISR_and_IRR
#[derive(Debug)]
pub struct IrqStatusRegisters {
    pub master_isr: u8,
    pub master_irr: u8,
    pub slave_isr: u8,
    pub slave_irr: u8,
}
impl fmt::Display for IrqStatusRegisters {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Master ISR: {:#b}, Master IRR:{:#b}, Slave ISR: {:#b}, Slave IRR: {:#b}", 
                self.master_isr, self.master_irr, self.slave_isr, self.slave_irr)
    }
}


/// An individual PIC chip.  This is not exported, because we always access
/// it through `Pics` below.
struct Pic {
    /// The base offset to which our interrupts are mapped.
    offset: u8,

    /// The processor I/O port on which we send commands.
    command: port_io::Port<u8>,

    /// The processor I/O port on which we send and receive data.
    data: port_io::Port<u8>,
}

impl Pic {
    /// Are we in change of handling the specified interrupt?
    /// (Each PIC handles 8 interrupts.)
    fn handles_interrupt(&self, interupt_id: u8) -> bool {
        self.offset <= interupt_id && interupt_id < self.offset + 8
    }

    /// Notify us that an interrupt has been handled and that we're ready
    /// for more.
    fn end_of_interrupt(&mut self) {
        // SAFE because 
        unsafe {
            self.command.write(CMD_END_OF_INTERRUPT);
        }
    }
}

/// A pair of chained PIC controllers.  This is the standard setup on x86.
pub struct ChainedPics {
    pics: [Pic; 2],
}

impl ChainedPics {
    /// Create a new interface for the standard PIC1 and PIC2 controllers,
    /// specifying the desired interrupt offsets.
    pub const fn new(master_offset: u8, slave_offset: u8) -> ChainedPics {
        ChainedPics {
            pics: [
                Pic {
                    offset: master_offset,
                    command: port_io::Port::new(0x20),
                    data: port_io::Port::new(0x21),
                },
                Pic {
                    offset: slave_offset,
                    command: port_io::Port::new(0xA0),
                    data: port_io::Port::new(0xA1),
                },
            ]
        }
    }

    /// Initialize both our PICs.  We initialize them together, at the same
    /// time, because it's traditional to do so, and because I/O operations
    /// might not be instantaneous on older processors.
    /// Unsafe because we are changing the way the PIC works, 
    /// which could potentially stop certain interrupts from occurring.
    pub unsafe fn initialize(&mut self, master_mask: u8, slave_mask: u8) {
        // We need to add a delay between writes to our PICs, especially on
        // older motherboards.  But we don't necessarily have any kind of
        // timers yet, because most of them require interrupts.  Various
        // older versions of Linux and other PC operating systems have
        // worked around this by writing garbage data to port 0x80, which
        // allegedly takes long enough to make everything work on most
        // hardware.  Here, `io_wait` is a closure.
        let wait_port: port_io::Port<u8> = port_io::Port::new(0x80);
        let io_wait = || { wait_port.write(0) };

        // Save our original interrupt masks, because I'm too lazy to
        // figure out reasonable values.  We'll restore these when we're
        // done.
        // let saved_mask1 = self.pics[0].data.read();
        // let saved_mask2 = self.pics[1].data.read();
        // info!("saved masks: {:#x}, {:#x}", saved_mask1, saved_mask2); 
        // println_unsafe!("saved masks: {:#x}, {:#x}", saved_mask1, saved_mask2); 

        // mask all interrupts during init
        self.pics[0].data.write(0xFF);
        io_wait();
        self.pics[1].data.write(0xFF);
        io_wait();


        // pre-emptively acknowledge both PICs in case they have pending unhandled irqs
        self.pics[0].command.write(CMD_END_OF_INTERRUPT);
        io_wait();
        self.pics[1].command.write(CMD_END_OF_INTERRUPT);
        io_wait();


        // Tell each PIC that we're going to send it a three-byte
        // initialization sequence on its data port.
        self.pics[0].command.write(CMD_INIT);
        io_wait();
        self.pics[1].command.write(CMD_INIT);
        io_wait();

        // Byte 1: Set up our base offsets.
        self.pics[0].data.write(self.pics[0].offset);
        io_wait();
        self.pics[1].data.write(self.pics[1].offset);
        io_wait();

        // Byte 2: Configure chaining between PIC1 and PIC2.
        self.pics[0].data.write(4);
        io_wait();
        self.pics[1].data.write(2);
        io_wait();

        // Byte 3: Set our mode.
        self.pics[0].data.write(MODE_8086);
        io_wait();
        self.pics[1].data.write(MODE_8086);
        io_wait();

        
        // 0 means enabled, 1 means disabled (masked)
        self.pics[0].data.write(master_mask);
        io_wait();
        self.pics[1].data.write(slave_mask); 
        io_wait();


        // pre-emptively acknowledge both PICs in case they have pending unhandled irqs
        // this is generally unnecessary but doesn't hurt if the interrupt hardware is misbehaving
        self.pics[0].command.write(CMD_END_OF_INTERRUPT);
        io_wait();
        self.pics[1].command.write(CMD_END_OF_INTERRUPT);
        io_wait();

    }

    /// Do we handle this interrupt?
    fn handles_interrupt(&self, interrupt_id: u8) -> bool {
        self.pics.iter().any(|p| p.handles_interrupt(interrupt_id))
    }

    /// Figure out which (if any) PICs in our chain need to know about this
    /// interrupt.  This is tricky, because all interrupts from `pics[1]`
    /// get chained through `pics[0]`.
    pub fn notify_end_of_interrupt(&mut self, interrupt_id: u8) {
        if self.handles_interrupt(interrupt_id) {
            if self.pics[1].handles_interrupt(interrupt_id) {
                self.pics[1].end_of_interrupt();
            }
            self.pics[0].end_of_interrupt();
        }
    }


    /// Reads the ISR and IRR registers of both the master and slave PIC.
    pub fn read_isr_irr(&self) -> IrqStatusRegisters {
        // SAFE: just reading PIC registers, no harm can be done.
        unsafe {
            self.pics[0].command.write(CMD_ISR);
            self.pics[1].command.write(CMD_ISR);
            let master_isr = self.pics[0].command.read();
            let slave_isr  = self.pics[1].command.read();

            self.pics[0].command.write(CMD_IRR);
            self.pics[1].command.write(CMD_IRR);
            let master_irr = self.pics[0].command.read();
            let slave_irr  = self.pics[1].command.read();

            IrqStatusRegisters {
                master_isr: master_isr,
                master_irr: master_irr,
                slave_isr: slave_isr,
                slave_irr: slave_irr,
            }
        }
    }
}