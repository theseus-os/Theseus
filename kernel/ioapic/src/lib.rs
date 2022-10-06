#![no_std]


extern crate alloc;
#[macro_use] extern crate log;
extern crate spin;
extern crate memory;
extern crate volatile;
extern crate zerocopy;
extern crate atomic_linked_list;
extern crate owning_ref;


use alloc::boxed::Box;
use spin::{Mutex, MutexGuard};
use volatile::{Volatile, WriteOnly};
use zerocopy::FromBytes;
use memory::{PageTable, PhysicalAddress, EntryFlags, allocate_pages, allocate_frames_at, MappedPages};
use atomic_linked_list::atomic_map::AtomicMap;
use owning_ref::BoxRefMut;


/// The system-wide list of all `IoApic`s, of which there is usually one, 
/// but larger systems can have multiple IoApic chips.
static IOAPICS: AtomicMap<u8, Mutex<IoApic>> = AtomicMap::new();


/// Returns a reference to the list of IoApics.
pub fn get_ioapics() -> &'static AtomicMap<u8, Mutex<IoApic>> {
	&IOAPICS
}

/// If an `IoApic` with the given `id` exists, then lock it (acquire its Mutex)
/// and return the locked `IoApic`.
pub fn get_ioapic(ioapic_id: u8) -> Option<MutexGuard<'static, IoApic>> {
	IOAPICS.get(&ioapic_id).map(|ioapic| ioapic.lock())
}

/// Returns the first `IoApic` that was created, if any, after locking it.
/// This is not necessarily the default one.
pub fn get_first_ioapic() -> Option<MutexGuard<'static, IoApic>> {
	IOAPICS.iter().next().map(|(_id, ioapic)| ioapic.lock())
}



#[derive(FromBytes)]
#[repr(C)]
struct IoApicRegisters {
    /// Chooses which IoApic register the following access will write to or read from.
    register_index:       WriteOnly<u32>,
    _padding0:            [u32; 3],
    /// The register containing the actual data that we want to read or write.
    register_data:        Volatile<u32>,
    _padding1:            [u32; 3],    
}


/// Each IoApic handles a maximum of 24 interrupt redirection entries. 
const INTERRUPT_ENTRIES_PER_IOAPIC: u32 = 24; 


/// A representation of an IoApic (x86-specific interrupt chip for I/O devices).
pub struct IoApic {
    regs: BoxRefMut<MappedPages, IoApicRegisters>,
    /// The ID of this IoApic.
    pub id: u8,
    /// not yet used.
    _phys_addr: PhysicalAddress,
    /// The first global interrupt number handled by this IoApic.
    /// Each IoApic only handles 24 interrupts, 
    /// so the last interrupt number supported by thie IoApic is `gsi_base + 23`.
    gsi_base: u32,
}

impl IoApic {
    /// Creates a new IoApic struct from the given `id`, `PhysicalAddress`, and `gsi_base`,
    /// and then adds it to the system-wide list of all IOAPICs.
    pub fn new(page_table: &mut PageTable, id: u8, phys_addr: PhysicalAddress, gsi_base: u32) -> Result<(), &'static str> {
        let new_page = allocate_pages(1).ok_or("IoApic::new(): couldn't allocate_pages!")?;
        let frame = allocate_frames_at(phys_addr, 1).map_err(|_e| "Couldn't allocate physical frame for IOAPIC")?;
        let ioapic_mapped_page = page_table.map_allocated_pages_to(
            new_page,
            frame, 
            EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE, 
        )?;

        let ioapic_regs = BoxRefMut::new(Box::new(ioapic_mapped_page)).try_map_mut(|mp| mp.as_type_mut::<IoApicRegisters>(0))?;
        let ioapic = IoApic {
            regs: ioapic_regs,
			id: id,
            _phys_addr: phys_addr,
            gsi_base: gsi_base,
		};

        debug!("Created new IoApic, id: {}, gsi_base: {}, phys_addr: {:#X}", id, gsi_base, phys_addr);
        IOAPICS.insert(id, Mutex::new(ioapic));
        Ok(())
    }

    /// Returns whether this IoApic handles the given `irq_num`, i.e.,
    /// whether it's within the range of IRQs handled by this `IoApic`.
    pub fn handles_irq(&self, irq_num: u32) -> bool {
        (irq_num >= self.gsi_base) && 
        (irq_num < (self.gsi_base + INTERRUPT_ENTRIES_PER_IOAPIC))
    }

    fn read_reg(&mut self, register_index: u32) -> u32 {
        // to read from an IoApic reg, we first write which register we want to read from,
        // then we read the value from it in the next register
        self.regs.register_index.write(register_index);
        self.regs.register_data.read()
    }

    fn write_reg(&mut self, register_index: u32, value: u32) {
        // to write to an IoApic reg, we first write which register we want to write to,
        // then we write the value to it in the next register
        self.regs.register_index.write(register_index);
        self.regs.register_data.write(value);
    }

    /// gets this IoApic's id.
    pub fn id(&mut self) -> u32 {
        self.read_reg(0x0)
    }

    /// gets this IoApic's version.
    pub fn version(&mut self) -> u32 {
        self.read_reg(0x1)
    }

    /// gets this IoApic's arbitration id.
    pub fn arbitration_id(&mut self) -> u32 {
        self.read_reg(0x2)
    }

    /// Masks (disables) the given IRQ line. 
    /// NOTE: this function is UNTESTED!
    pub fn mask_irq(&mut self, irq: u8) {
        let irq_reg: u32 = 0x10 + (2 * irq as u32);
        let direction = self.read_reg(irq_reg);
        self.write_reg(irq_reg, direction | (1 << 16));
    }

    /// Set IRQ to an interrupt vector.
    ///
    /// # Arguments
    /// * `ioapic_irq`: the IRQ number that this interrupt will trigger on this IoApic.
    /// * `lapic_id`: the id of the LocalApic that should handle this interrupt.
    /// * `irq_vector`: the system-wide IRQ vector number,
    ///    which after remapping is from 0x20 to 0x2F 
    ///    (see [`interrupts::IRQ_BASE_OFFSET`](../interrupts/constant.IRQ_BASE_OFFSET.html)).
    ///    For example, 0x20 is the PIT timer, 0x21 is the PS2 keyboard, etc.
    pub fn set_irq(&mut self, ioapic_irq: u8, lapic_id: u8, irq_vector: u8) {
        let vector = irq_vector as u8;

        let low_index: u32 = 0x10 + (ioapic_irq as u32) * 2;
        let high_index: u32 = 0x10 + (ioapic_irq as u32) * 2 + 1;

        let mut high = self.read_reg(high_index);
        high &= !0xff000000;
        high |= (lapic_id as u32) << 24;
        self.write_reg(high_index, high);

        let mut low = self.read_reg(low_index);
        low &= !(1<<16);
        low &= !(1<<11);
        low &= !0x700;
        low &= !0xff;
        low |= vector as u32;
        self.write_reg(low_index, low);
    }
}