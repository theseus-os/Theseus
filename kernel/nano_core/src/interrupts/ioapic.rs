use core::ptr::{read_volatile, write_volatile};
use spin::{Mutex, MutexGuard, Once};
use x86::current::cpuid::CpuId;
use x86::shared::msr::*;
use core::ops::DerefMut;
use memory::{FRAME_ALLOCATOR, Frame, ActivePageTable, PhysicalAddress, Page, VirtualAddress, EntryFlags};
use kernel_config::memory::{IOAPIC_START, KERNEL_OFFSET};

static IOAPIC: Once<Mutex<IoApic>> = Once::new();


pub fn init(active_table: &mut ActivePageTable, id: u8, phys_addr: PhysicalAddress, gsi_base: u32)
            -> &'static Mutex<IoApic>
{
    let ioapic: &'static Mutex<IoApic> = IOAPIC.call_once( || {
	    unsafe { Mutex::new(IoApic::create(active_table, id, phys_addr, gsi_base)) }
    });
    ioapic
}

pub fn get_ioapic() -> Option<MutexGuard<'static, IoApic>> {
	IOAPIC.try().map( |ioapic| ioapic.lock())
}


/// An IoApic 
#[derive(Debug)]
pub struct IoApic {
    pub virt_addr: VirtualAddress,
    pub id: u8,
    phys_addr: PhysicalAddress,
    gsi_base: u32,
}

impl IoApic {
    fn create(active_table: &mut ActivePageTable, id: u8, phys_addr: PhysicalAddress, gsi_base: u32) -> IoApic {
		let mut ioapic = IoApic {
			virt_addr: IOAPIC_START as VirtualAddress,
			id: id,
            phys_addr: phys_addr,
            gsi_base: gsi_base,
		};

		debug!("Creating new IoApic: {:?}", ioapic); 

        {
            let page = Page::containing_address(ioapic.virt_addr);
            let frame = Frame::containing_address(ioapic.phys_addr);
			let mut fa = FRAME_ALLOCATOR.try().unwrap().lock();
            active_table.map_to(page, frame, EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE, fa.deref_mut());
        }
      
        ioapic
    }

    

    unsafe fn read_reg(&self, reg: u32) -> u32 {
        // to read from an IoApic reg, we first write which register we want to read from,
        // then we read the value from it in the next register
        write_volatile((self.virt_addr + 0x0) as *mut u32, reg);
        read_volatile((self.virt_addr + 0x10) as *const u32)
    }

    unsafe fn write_reg(&mut self, reg: u32, value: u32) {
        // to write to an IoApic reg, we first write which register we want to write to,
        // then we write the value to it in the next register
        write_volatile((self.virt_addr + 0x0) as *mut u32, reg);
        write_volatile((self.virt_addr + 0x10) as *mut u32, value);
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

    /// Set IRQ to an interrupt vector.
    /// # Arguments
    /// ioapic_irq: the IRQ number that this interrupt will trigger on this IoApic.
    /// lapic_id: the id of the LocalApic that should handle this interrupt.
    /// irq_vector: the system-wide (PIC-based) IRQ vector number,
    /// which after remapping is usually 0x20 to 0x2F  (0x20 is timer, 0x21 is keyboard, etc)
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
        low |= (vector as u32);
        unsafe { self.write_reg(low_index, low) };
    }
}