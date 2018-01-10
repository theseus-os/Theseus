use core::ptr::{read_volatile, write_volatile};
use spin::{Mutex, MutexGuard};
use x86::current::cpuid::CpuId;
use x86::shared::msr::*;
use core::ops::DerefMut;
use memory::{FRAME_ALLOCATOR, Frame, ActivePageTable, PhysicalAddress, Page, VirtualAddress, EntryFlags};
use kernel_config::memory::{APIC_START, KERNEL_OFFSET};

static LOCAL_APIC: Mutex<Option<LocalApic>> = Mutex::new(None);

pub fn init(active_table: &mut ActivePageTable) -> Result<(), &'static str> {
    let mut la = LOCAL_APIC.lock();
	if let Some(_) = *la {
		error!("apic::init() was already called! It cannot be called twice.");
		Err("apic::init() was already called before.")
	}
	else {
		*la = Some( unsafe { LocalApic::create(active_table) } );
		Ok(())
	}
}

pub fn init_ap() -> Result<(), &'static str> {
	let mut la = LOCAL_APIC.lock();
	if let Some(ref mut lapic) = *la {
    	unsafe { lapic.init_ap(); }
		Ok(())
	}
	else {
		error!("apic::init() hasn't yet been called!");
		Err("apic::init() has to be called first")
	}
}

pub fn get_lapic() -> MutexGuard<'static, Option<LocalApic>> {
	LOCAL_APIC.lock()
}

/// Local APIC
pub struct LocalApic {
    pub virt_addr: VirtualAddress,
    pub x2: bool
}

impl LocalApic {
    unsafe fn create(active_table: &mut ActivePageTable) -> LocalApic {
		debug!("IA32_APIC_BASE: {:#X}", rdmsr(IA32_APIC_BASE));
		let mut lapic = LocalApic {
			virt_addr: APIC_START as VirtualAddress,
			x2: CpuId::new().get_feature_info().unwrap().has_x2apic(),
		};
		let phys_addr = (rdmsr(IA32_APIC_BASE) as usize & 0xFFFF_0000) as PhysicalAddress;
		debug!("Apic has x2apic?  {}", lapic.x2);

		// x2apic doesn't require MMIO, it just uses MSRs instead, 
		// cuz it's easier to write on 64-bit value directly into an MSR instead of 
		// writing 2 separated 32-bit values into adjacent 32-bit MMIO-registers
        if !lapic.x2 {
            let page = Page::containing_address(lapic.virt_addr);
            let frame = Frame::containing_address(phys_addr);
			let mut fa = FRAME_ALLOCATOR.try().unwrap().lock();
            active_table.map_to(page, frame, EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_EXECUTE, fa.deref_mut());
        }

        lapic.init_ap();
		lapic
    }

    unsafe fn init_ap(&mut self) {
        if self.x2 {
            wrmsr(IA32_APIC_BASE, rdmsr(IA32_APIC_BASE) | 1 << 10);
            wrmsr(IA32_X2APIC_SIVR, 0x100);
        } else {
            self.write_reg(0xF0, 0x100);
        }
    }

    unsafe fn read_reg(&self, reg: u32) -> u32 {
        read_volatile((self.virt_addr + reg as usize) as *const u32)
    }

    unsafe fn write_reg(&mut self, reg: u32, value: u32) {
        write_volatile((self.virt_addr + reg as usize) as *mut u32, value);
    }

    pub fn id(&self) -> u32 {
        if self.x2 {
            unsafe { rdmsr(IA32_X2APIC_APICID) as u32 }
        } else {
            unsafe { self.read_reg(0x20) }
        }
    }

    pub fn version(&self) -> u32 {
        if self.x2 {
            unsafe { rdmsr(IA32_X2APIC_VERSION) as u32 }
        } else {
            unsafe { self.read_reg(0x30) }
        }
    }

    pub fn icr(&self) -> u64 {
        if self.x2 {
            unsafe { rdmsr(IA32_X2APIC_ICR) }
        } else {
            unsafe {
                (self.read_reg(0x310) as u64) << 32 | self.read_reg(0x300) as u64
            }
        }
    }

    pub fn set_icr(&mut self, value: u64) {
        if self.x2 {
            unsafe { wrmsr(IA32_X2APIC_ICR, value); }
        } else {
            unsafe {
                while self.read_reg(0x300) & 1 << 12 == 1 << 12 {}
                self.write_reg(0x310, (value >> 32) as u32);
                self.write_reg(0x300, value as u32);
                while self.read_reg(0x300) & 1 << 12 == 1 << 12 {}
            }
        }
    }

    pub fn ipi(&mut self, apic_id: usize) {
        let mut icr = 0x4040;
        if self.x2 {
            icr |= (apic_id as u64) << 32;
        } else {
            icr |= (apic_id as u64) << 56;
        }
        self.set_icr(icr);
    }

    pub unsafe fn eoi(&mut self) {
        if self.x2 {
            wrmsr(IA32_X2APIC_EOI, 0);
        } else {
            self.write_reg(0xB0, 0);
        }
    }
}