//! Support for the x86 PIC (8259 Programmable Interrupt Controller),
//! which handles basic interrupts.
//! In multicore mode, this isn't used in favor of the APIC interface.

//! This was modified from the toyos pic8259_simple crate.

#![no_std]

extern crate port_io;

use core::fmt;


/// The offset added to the first IRQ: `0x20`.
/// 
/// This is needed to shift the start of all IRQ vectors 
/// to after the end of the CPU exception vectors,
/// which occupy the first 32 IRQ vectors.
pub const IRQ_BASE_OFFSET: u8 = 0x20;

/// The IRQ number reserved for spurious PIC interrupts (as recommended by OS dev wiki).
pub const PIC_SPURIOUS_INTERRUPT_IRQ: u8 = IRQ_BASE_OFFSET + 0x7;

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

const MASTER_CMD:  u16 = 0x20;
const MASTER_DATA: u16 = 0x21;
const SLAVE_CMD:   u16 = 0xA0;
const SLAVE_DATA:  u16 = 0xA1;


/// The set of status registers for both PIC chips.
///
/// Each PIC chip has two interrupt status registers: 
///  * `ISR`: the In-Service Register: specifies which interrupts are currently being serviced,
///     meaning IRQs sent to the CPU. 
///  * `IRR`: the Interrupt Request Register: specifies which interrupts have been raised
///     but not necessarily serviced yet.
///
/// Based on the interrupt mask, the PIC will send interrupts from the IRR to the CPU, 
/// at which point they are marked in the ISR.
///
/// For more, [see this explanation](http://wiki.osdev.org/8259_PIC#ISR_and_IRR).
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
impl fmt::Debug for IrqStatusRegisters {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
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
    fn end_of_interrupt(&self) {
        unsafe {
            self.command.write(CMD_END_OF_INTERRUPT);
        }
    }
}

/// A pair of chained PIC chips, which represents the standard x86 configuration.
pub struct ChainedPics {
    pics: [Pic; 2],
}

impl ChainedPics {
    /// Create a new interface for the standard PIC1 and PIC2 controllers,
    /// specifying the desired interrupt offsets.
    /// Then, it initializes the PICs in a standard chained fashion, 
    /// which involved mapping the master PIC to 0x20 and the slave to 0x28 (standard rempaping),
    /// because even if we don't use them (and disable them for APIC instead),
    /// we still need to remap them to avoid a spurious interrupt clashing with an exception.
    pub fn init(master_mask: u8, slave_mask: u8) -> ChainedPics {
        let mut cpic = ChainedPics {
            pics: [
                Pic {
                    offset: IRQ_BASE_OFFSET,
                    command: port_io::Port::new(MASTER_CMD),
                    data: port_io::Port::new(MASTER_DATA),
                },
                Pic {
                    offset: IRQ_BASE_OFFSET + 8, // 8 IRQ lines per PIC
                    command: port_io::Port::new(SLAVE_CMD),
                    data: port_io::Port::new(SLAVE_DATA),
                },
            ]
        };
        // SAFE: we already checked that we are the only ones to have called this constructor.
        unsafe {
            cpic.configure(master_mask, slave_mask);
        }
        cpic
    }


    /// Initialize both our PICs.  We initialize them together, at the same
    /// time, because it's traditional to do so, and because I/O operations
    /// might not be instantaneous on older processors.
    unsafe fn configure(&mut self, master_mask: u8, slave_mask: u8) {
      
        // mask all interrupts during init
        self.mask_irqs(0xFF, 0xFF);

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

        self.mask_irqs(master_mask, slave_mask);

        // pre-emptively acknowledge both PICs in case they have pending unhandled irqs
        // this is generally unnecessary but doesn't hurt if the interrupt hardware is misbehaving
        self.pics[0].command.write(CMD_END_OF_INTERRUPT);
        io_wait();
        self.pics[1].command.write(CMD_END_OF_INTERRUPT);
        io_wait();

    }

    /// Each mask is a bitwise mask for each IRQ line, with the master's IRQ line 2 (0x4) 
    /// affecting the entire slave IRQ mask. So if the master's IRQ line 2 is masked (disabled),
    /// all slave IRQs (0x28 to 0x2F) are masked.
    /// If a bit is set to 1, it is masked (disabled). If set to 0, it is unmasked (enabled).
    pub fn mask_irqs(&self, master_mask: u8, slave_mask: u8) {
        // SAFE: we are guaranteed to have initialized this structure in its constructor.
        unsafe {
            self.pics[1].data.write(slave_mask);
            io_wait();
            self.pics[0].data.write(master_mask);
            io_wait();
        }
    }
 

    /// Do we handle this interrupt?
    fn handles_interrupt(&self, interrupt_id: u8) -> bool {
        self.pics.iter().any(|p| p.handles_interrupt(interrupt_id))
    }

    /// Figure out which (if any) PICs in our chain need to know about this
    /// interrupt.  This is tricky, because all interrupts from `pics[1]`
    /// get chained through `pics[0]`.
    pub fn notify_end_of_interrupt(&self, interrupt_id: u8) {
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


#[inline(always)]
fn io_wait() {
    // We need to add a short delay between writes to our PICs, especially on
    // older motherboards.  But we don't necessarily have any kind of
    // timers yet, because most of them require interrupts.  Various
    // older versions of Linux and other PC operating systems have
    // worked around this by writing garbage data to port 0x80, which
    // allegedly takes long enough to make everything work on most hardware.
    unsafe { port_io::Port::<u8>::new(0x80).write(0); }
}
