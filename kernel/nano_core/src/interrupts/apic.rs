use core::ptr::{read_volatile, write_volatile};
use spin::{Mutex, MutexGuard};
use x86::current::cpuid::CpuId;
use x86::shared::msr::*;
use core::ops::DerefMut;
use memory::{FRAME_ALLOCATOR, Frame, ActivePageTable, PhysicalAddress, Page, VirtualAddress, EntryFlags};
use kernel_config::memory::{APIC_START, KERNEL_OFFSET};

static LOCAL_APIC: Mutex<Option<LocalApic>> = Mutex::new(None);

pub const APIC_SPURIOUS_INTERRUPT_VECTOR: u32 = 0xFF;
const IA32_APIC_BASE_MSR_IS_BSP: u64 = 0x100;
const IA32_APIC_BASE_MSR_ENABLE: usize = 0x800;


const APIC_REG_LAPIC_ID               : u32 =  0x20;
const APIC_REG_LAPIC_VERSION          : u32 =  0x30;
const APIC_REG_TASK_PRIORITY          : u32 =  0x80;	// Task Priority
const APIC_REG_ARBITRATION_PRIORITY   : u32 =  0x90;	// Arbitration Priority
const APIC_REG_PROCESSOR_PRIORITY     : u32 =  0xA0;	// Processor Priority
const APIC_REG_EOI                    : u32 =  0xB0; // End of Interrupt
const APIC_REG_REMOTE_READ            : u32 =  0xC0;	// Remote Read
const APIC_REG_LDR                    : u32 =  0xD0;	// Local (Logical?) Destination
const APIC_REG_DFR                    : u32 =  0xE0;	// Destination Format
const APIC_REG_SIR                    : u32 =  0xF0;	// Spurious Interrupt Vector
const APIC_REG_ISR                    : u32 =  0x100;	// In-Service Register (First of 8)
const APIC_REG_TMR                    : u32 =  0x180;	// Trigger Mode (1/8)
const APIC_REG_IRR                    : u32 =  0x200;	// Interrupt Request Register (1/8)
const APIC_REG_ErrStatus              : u32 =  0x280;	// Error Status
const APIC_REG_LVT_CMCI               : u32 =  0x2F0;	// LVT CMCI Registers (?)
const APIC_REG_ICR_LOW                : u32 =  0x300;	// Interrupt Command Register (1/2)
const APIC_REG_ICR_HIGH               : u32 =  0x300;	// Interrupt Command Register (2/2)
const APIC_REG_LVT_TIMER              : u32 =  0x320;
const APIC_REG_LVT_THERMAL            : u32 =  0x330; // Thermal sensor
const APIC_REG_LVT_PMI                : u32 =  0x340; // Performance Monitoring information
const APIC_REG_LVT_LINT0              : u32 =  0x350;
const APIC_REG_LVT_LINT1              : u32 =  0x360;
const APIC_REG_LVT_ERROR              : u32 =  0x370;
const APIC_REG_INIT_COUNT             : u32 =  0x380;
const APIC_REG_CURRENT_COUNT          : u32 =  0x390;
const APIC_REG_TIMER_DIVIDE           : u32 =  0x3E0;


const APIC_TIMER_PERIODIC:  u32 = 0x2_0000;
const APIC_DISABLE: u32 = 0x1_0000;
const APIC_NMI: u32 = 4 << 8;
const APIC_SW_ENABLE: u32 = 0x100;



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

// pub fn init_ap() -> Result<(), &'static str> {
// 	let mut la = LOCAL_APIC.lock();
// 	if let Some(ref mut lapic) = *la {
//     	unsafe { lapic.enable_apic(); }
// 		Ok(())
// 	}
// 	else {
// 		error!("apic::init() hasn't yet been called!");
// 		Err("apic::init() has to be called first")
// 	}
// }

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

		debug!("Apic has x2apic?  {}", lapic.x2);

        // redox bitmasked the base paddr with 0xFFFF_0000, os dev wiki says 0xFFFF_F000 ...
        // seems like 0xFFFF_F000 is more correct since it just frame/page-aligns the address
        let phys_addr = (rdmsr(IA32_APIC_BASE) as usize & 0xFFFF_F000) as PhysicalAddress;
		
        // x2apic doesn't require MMIO, it just uses MSRs instead, so we don't need to map the APIC registers.
		// x2apic is better because it's easier to write a 64-bit value directly into an MSR instead of 
		// writing 2 separate 32-bit values into adjacent 32-bit APIC memory-mapped I/O registers.
        if !lapic.x2 {
            let page = Page::containing_address(lapic.virt_addr);
            let frame = Frame::containing_address(phys_addr);
			let mut fa = FRAME_ALLOCATOR.try().unwrap().lock();
            active_table.map_to(page, frame, EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_EXECUTE, fa.deref_mut());
        }

        lapic.enable_apic();
        if lapic.x2 { lapic.init_timer_x2(); } else { lapic.init_timer(); }
		lapic
    }

    /// enables the spurious interrupt vector, which enables the APIC itself.
    unsafe fn enable_apic(&mut self) {
        // init APIC to a clean state, based on: http://wiki.osdev.org/APIC_timer#Example_code_in_ASM
        self.write_reg(APIC_REG_DFR, 0xFFFF_FFFF);
        let old_ldr = self.read_reg(APIC_REG_LDR);
        self.write_reg(APIC_REG_LDR, old_ldr & 0x00FF_FFFF | 1);
        self.write_reg(APIC_REG_LVT_TIMER, APIC_DISABLE);
        self.write_reg(APIC_REG_LVT_PMI, APIC_NMI);
        self.write_reg(APIC_REG_LVT_LINT0, APIC_DISABLE);
        self.write_reg(APIC_REG_LVT_LINT1, APIC_DISABLE);
        self.write_reg(APIC_REG_TASK_PRIORITY, 0);


        let is_bsp = rdmsr(IA32_APIC_BASE) & IA32_APIC_BASE_MSR_IS_BSP;
        wrmsr(IA32_APIC_BASE, rdmsr(IA32_APIC_BASE) | is_bsp | (IA32_APIC_BASE_MSR_ENABLE as u64));
        info!("LAPIC ID {} is_bsp: {}", self.version(), is_bsp == IA32_APIC_BASE_MSR_IS_BSP);

        if self.x2 {
            wrmsr(IA32_X2APIC_SIVR, (APIC_SPURIOUS_INTERRUPT_VECTOR | APIC_SW_ENABLE) as u64);
        } else {
            self.write_reg(APIC_REG_SIR, APIC_SPURIOUS_INTERRUPT_VECTOR | APIC_SW_ENABLE); // set bit 8 to start receiving interrupts (still need to "sti")
        }
    }

    unsafe fn init_timer(&mut self) {

        self.write_reg(APIC_REG_TIMER_DIVIDE, 3); // set divide value to 16 ( ... how does 3 => 16 )
        // map APIC timer to an interrupt, here we use 0x20 (IRQ 32)
        self.write_reg(APIC_REG_LVT_TIMER, 0x20 | APIC_TIMER_PERIODIC); // TODO: FIXME: change 0x20, it's the IRQ number we use for timer
        self.write_reg(APIC_REG_INIT_COUNT, 0x100000);

        // stuff below taken from Tifflin rust-os
        self.write_reg(APIC_REG_LVT_THERMAL, 0);
        // self.write_reg(APIC_REG_LVT_PMI, 0);
        // self.write_reg(APIC_REG_LVT_LINT0, 0);
        // self.write_reg(APIC_REG_LVT_LINT1, 0);
        self.write_reg(APIC_REG_LVT_ERROR, 0);

        // os dev wiki guys say that setting this again as a last step helps on some strange hardware.
        self.write_reg(APIC_REG_TIMER_DIVIDE, 3); 
    }

    unsafe fn init_timer_x2(&mut self) {
        unimplemented!();

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
            unsafe { self.read_reg(APIC_REG_LAPIC_ID) }
        }
    }

    pub fn version(&self) -> u32 {
        if self.x2 {
            unsafe { rdmsr(IA32_X2APIC_VERSION) as u32 }
        } else {
            unsafe { self.read_reg(APIC_REG_LAPIC_VERSION) }
        }
    }

    pub fn icr(&self) -> u64 {
        if self.x2 {
            unsafe { rdmsr(IA32_X2APIC_ICR) }
        } else {
            unsafe {
                (self.read_reg(APIC_REG_ICR_HIGH) as u64) << 32 | self.read_reg(APIC_REG_ICR_LOW) as u64
            }
        }
    }

    pub fn set_icr(&mut self, value: u64) {
        if self.x2 {
            unsafe { wrmsr(IA32_X2APIC_ICR, value); }
        } else {
            unsafe {
                while self.read_reg(APIC_REG_ICR_LOW) & 1 << 12 == 1 << 12 {}
                self.write_reg(APIC_REG_ICR_HIGH, (value >> 32) as u32);
                self.write_reg(APIC_REG_ICR_LOW, value as u32);
                while self.read_reg(APIC_REG_ICR_LOW) & 1 << 12 == 1 << 12 {}
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

    pub fn eoi(&mut self, isr: u32) {
        unsafe {
            if self.x2 {
                wrmsr(IA32_X2APIC_EOI, isr as u64); // should be isr, not 0?
            } else {
                self.write_reg(APIC_REG_EOI, isr);
            }
        }
    }
}