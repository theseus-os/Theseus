use core::ptr::{read_volatile, write_volatile};
use spin::{Mutex, MutexGuard, Once};
use core::ops::DerefMut;
use memory::{FRAME_ALLOCATOR, Frame, ActivePageTable, PhysicalAddress, EntryFlags, allocate_pages, MappedPages};

static IOAPIC: Once<Mutex<IoApic>> = Once::new();


pub fn init(active_table: &mut ActivePageTable, id: u8, phys_addr: PhysicalAddress, gsi_base: u32)
            -> Result<&'static Mutex<IoApic>, &'static str>
{
    let ioapic = try!(IoApic::create(active_table, id, phys_addr, gsi_base));
    let res = IOAPIC.call_once( || {
	    Mutex::new(ioapic)
    });
    Ok(res)
}

pub fn get_ioapic() -> Option<MutexGuard<'static, IoApic>> {
	IOAPIC.try().map( |ioapic| ioapic.lock())
}


/// An IoApic 
#[derive(Debug)]
pub struct IoApic {
    pub page: MappedPages,
    pub id: u8,
    phys_addr: PhysicalAddress,
    gsi_base: u32,
}

impl IoApic {
    fn create(active_table: &mut ActivePageTable, id: u8, phys_addr: PhysicalAddress, gsi_base: u32) -> Result<IoApic, &'static str> {

        let ioapic_mapped_page = {
    		let new_page = try!(allocate_pages(1).ok_or("IoApic::create(): couldn't allocated virtual page!"));
            let frame = Frame::range_inclusive(Frame::containing_address(phys_addr), Frame::containing_address(phys_addr));
			let mut fa = try!(FRAME_ALLOCATOR.try().ok_or("Couldn't get frame allocator")).lock();
            try!(active_table.map_allocated_pages_to(new_page, frame, 
                EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE, 
                fa.deref_mut())
            )
        };
        
        let ioapic = IoApic {
			page: ioapic_mapped_page,
			id: id,
            phys_addr: phys_addr,
            gsi_base: gsi_base,
		};
		debug!("Creating new IoApic: {:?}", ioapic); 
        Ok(ioapic)
    }

    

    unsafe fn read_reg(&self, reg: u32) -> u32 {
        // to read from an IoApic reg, we first write which register we want to read from,
        // then we read the value from it in the next register
        write_volatile((self.page.start_address() + 0x0) as *mut u32, reg);
        read_volatile((self.page.start_address() + 0x10) as *const u32)
    }

    unsafe fn write_reg(&mut self, reg: u32, value: u32) {
        // to write to an IoApic reg, we first write which register we want to write to,
        // then we write the value to it in the next register
        write_volatile((self.page.start_address() + 0x0) as *mut u32, reg);
        write_volatile((self.page.start_address() + 0x10) as *mut u32, value);
    }

    /// I/O APIC id.
    pub fn id(&self) -> u32 {
        unsafe { self.read_reg(0x0) }
    }

    /// I/O APIC version.
    pub fn version(&self) -> u32 {
        unsafe { self.read_reg(0x1) }
    }

    /// I/O APIC arbitration id.
    pub fn arbitration_id(&self) -> u32 {
        unsafe { self.read_reg(0x2) }
    }

    /// Masks (disables) the given IRQ line. 
    /// NOTE: this function is UNTESTED!
    pub fn mask_irq(&mut self, irq: u8) {
        let irq_reg: u32 = 0x10 + (2 * irq as u32);
        unsafe {
            let direction = self.read_reg(irq_reg);
            self.write_reg(irq_reg, direction | (1 << 16));
        }
    }

    /// Set IRQ to an interrupt vector.
    /// # Arguments
    /// ioapic_irq: the IRQ number that this interrupt will trigger on this IoApic.
    /// lapic_id: the id of the LocalApic that should handle this interrupt.
    /// irq_vector: the system-wide (PIC-based) IRQ vector number,
    /// which after remapping is 0x20 to 0x2F  (0x20 is timer, 0x21 is keyboard, etc).
    /// See interrupts::PIC_MASTER_OFFSET.
    pub fn set_irq(&mut self, ioapic_irq: u8, lapic_id: u8, irq_vector: u8) {
        let vector = irq_vector as u8;

        let low_index: u32 = 0x10 + (ioapic_irq as u32) * 2;
        let high_index: u32 = 0x10 + (ioapic_irq as u32) * 2 + 1;

        let mut high = unsafe { self.read_reg(high_index) };
        high &= !0xff000000;
        high |= (lapic_id as u32) << 24;
        unsafe { self.write_reg(high_index, high) };

        let mut low = unsafe { self.read_reg(low_index) };
        low &= !(1<<16);
        low &= !(1<<11);
        low &= !0x700;
        low &= !0xff;
        low |= vector as u32;
        unsafe { self.write_reg(low_index, low) };
    }
}