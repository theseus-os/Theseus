use core::ptr::{read_volatile, write_volatile};
use spin::{Mutex, MutexGuard};
use x86::current::cpuid::CpuId;
use x86::shared::msr::*;
use core::ops::DerefMut;
use memory::{FRAME_ALLOCATOR, Frame, ActivePageTable, PhysicalAddress, Page, VirtualAddress, EntryFlags};
use kernel_config::memory::{APIC_START, KERNEL_OFFSET};

static LOCAL_APIC: Mutex<Option<LocalApic>> = Mutex::new(None);

pub const APIC_SPURIOUS_INTERRUPT_VECTOR: u32 = 0xFF; // as recommended by everyone on os dev wiki
const IA32_APIC_XAPIC_ENABLE: u64 = 1 << 11; // 0x800
const IA32_APIC_X2APIC_ENABLE: u64 = 1 << 10; // 0x400
const IA32_APIC_BASE_MSR_IS_BSP: u64 = 1 << 8; // 0x100
const APIC_SW_ENABLE: u32 = 1 << 8;
const APIC_TIMER_PERIODIC:  u32 = 0x2_0000;
const APIC_DISABLE: u32 = 0x1_0000;
const APIC_NMI: u32 = 4 << 8;


const APIC_REG_LAPIC_ID               : u32 =  0x20;
const APIC_REG_LAPIC_VERSION          : u32 =  0x30;
const APIC_REG_TASK_PRIORITY          : u32 =  0x80;	// Task Priority
const APIC_REG_ARBITRATION_PRIORITY   : u32 =  0x90;	// Arbitration Priority
const APIC_REG_PROCESSOR_PRIORITY     : u32 =  0xA0;	// Processor Priority
const APIC_REG_EOI                    : u32 =  0xB0; // End of Interrupt
const APIC_REG_REMOTE_READ            : u32 =  0xC0;	// Remote Read
const APIC_REG_LDR                    : u32 =  0xD0;	// Logical Destination
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

        // let x2apic_version_support - CpuIid::new().get_feature_info().unwrap().
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

        if lapic.x2 { 
            lapic.enable_x2apic();
            lapic.init_timer_x2(); 
        } 
        else { 
            lapic.enable_apic();
            lapic.init_timer(); 
        }
		lapic
    }

    /// enables the spurious interrupt vector, which enables the APIC itself.
    unsafe fn enable_apic(&mut self) {
        assert!(!self.x2, "an x2apic system must not use enable_apic(), it should use enable_x2apic() instead.");

        // globally enable the apic by setting the xapic_enable bit
        let is_bsp = rdmsr(IA32_APIC_BASE) & IA32_APIC_BASE_MSR_IS_BSP; // need to preserve this when we set other bits in the APIC_BASE reg
        wrmsr(IA32_APIC_BASE, rdmsr(IA32_APIC_BASE) | is_bsp | IA32_APIC_XAPIC_ENABLE);
        info!("LAPIC ID {:#x} is_bsp: {}", self.version(), is_bsp == IA32_APIC_BASE_MSR_IS_BSP);


        // init APIC to a clean state, based on: http://wiki.osdev.org/APIC_timer#Example_code_in_ASM
        self.write_reg(APIC_REG_DFR, 0xFFFF_FFFF);
        let old_ldr = self.read_reg(APIC_REG_LDR);
        self.write_reg(APIC_REG_LDR, old_ldr & 0x00FF_FFFF | 1); // flat logical addressing
        self.write_reg(APIC_REG_LVT_TIMER, APIC_DISABLE);
        self.write_reg(APIC_REG_LVT_PMI, APIC_NMI);
        self.write_reg(APIC_REG_LVT_LINT0, APIC_DISABLE);
        self.write_reg(APIC_REG_LVT_LINT1, APIC_DISABLE);
        self.write_reg(APIC_REG_TASK_PRIORITY, 0);


        // set bit 8 to start receiving interrupts (still need to "sti")
        self.write_reg(APIC_REG_SIR, APIC_SPURIOUS_INTERRUPT_VECTOR | APIC_SW_ENABLE); 
    }

    unsafe fn enable_x2apic(&mut self) {
        assert!(self.x2, "an apic/xapic system must not use enable_x2apic(), it should use enable_apic() instead.");
        
        debug!("in enable_x2apic");
        // globally enable the x2apic, which includes also setting the xapic enable bit
        let is_bsp = rdmsr(IA32_APIC_BASE) & IA32_APIC_BASE_MSR_IS_BSP; // need to preserve this when we set other bits in the APIC_BASE reg
        debug!("in enable_x2apic 1: is_bsp: {:#X}", is_bsp);
        wrmsr(IA32_APIC_BASE, rdmsr(IA32_APIC_BASE) | is_bsp | IA32_APIC_XAPIC_ENABLE | IA32_APIC_X2APIC_ENABLE);
        debug!("in enable_x2apic 2: new apic_base: {:#X}", rdmsr(IA32_APIC_BASE));


        // init x2APIC to a clean state, just as in enable_apic() above 
        // info!("x2LAPIC ID {:#x} (cluster {:#X} logical {:#X}), is_bsp: {}", self.version(), cluster_id, logical_id, is_bsp == IA32_APIC_BASE_MSR_IS_BSP);
        // Note: in x2apic, there is not DFR reg because only cluster mode is enabled; there is no flat logical mode
        // Note: in x2apic, the IA32_X2APIC_LDR is read-only.
        let ldr = rdmsr(IA32_X2APIC_LDR);
        debug!("in enable_x2apic 0.1");
        let cluster_id = (ldr >> 16) & 0xFFFF; // highest 16 bits
        let logical_id = ldr & 0xFFFF; // lowest 16 bits
        debug!("in enable_x2apic 0.2"); wrmsr(IA32_X2APIC_LVT_TIMER, APIC_DISABLE as u64);
        debug!("in enable_x2apic 0.3"); wrmsr(IA32_X2APIC_LVT_PMI, APIC_NMI as u64);
        debug!("in enable_x2apic 0.4"); wrmsr(IA32_X2APIC_LVT_LINT0, APIC_DISABLE as u64);
        debug!("in enable_x2apic 0.5"); wrmsr(IA32_X2APIC_LVT_LINT1, APIC_DISABLE as u64);
        debug!("in enable_x2apic 0.6"); wrmsr(IA32_X2APIC_TPR, 0);
        
        
        
        wrmsr(IA32_X2APIC_SIVR, (APIC_SPURIOUS_INTERRUPT_VECTOR | APIC_SW_ENABLE) as u64); // set bit 8 to start receiving interrupts (still need to "sti")
        debug!("in enable_x2apic end");
        info!("x2LAPIC ID {:#x}  is_bsp: {}", self.version(), is_bsp == IA32_APIC_BASE_MSR_IS_BSP);
    }


    unsafe fn init_timer(&mut self) {
        assert!(!self.x2, "an x2apic system must not use init_timer(), it should use init_timerx2() instead.");

        self.write_reg(APIC_REG_TIMER_DIVIDE, 3); // set divide value to 16 ( ... how does 3 => 16 )
        // map APIC timer to an interrupt, here we use 0x20 (IRQ 32)
        self.write_reg(APIC_REG_LVT_TIMER, 0x20 | APIC_TIMER_PERIODIC); // TODO: FIXME: change 0x20, it's the IRQ number we use for timer
        self.write_reg(APIC_REG_INIT_COUNT, 0x100000);

        // stuff below taken from Tifflin rust-os
        self.write_reg(APIC_REG_LVT_THERMAL, 0);
        self.write_reg(APIC_REG_LVT_ERROR, 0);

        // os dev wiki guys say that setting this again as a last step helps on some strange hardware.
        self.write_reg(APIC_REG_TIMER_DIVIDE, 3); 
    }

    unsafe fn init_timer_x2(&mut self) {
        assert!(self.x2, "an apic/xapic system must not use init_timerx2(), it should use init_timer() instead.");
        debug!("in init_timer_x2 start");
        debug!("in init_timer_x2 2"); wrmsr(IA32_X2APIC_DIV_CONF, 3); // set divide value to 16 ( ... how does 3 => 16 )
        // map APIC timer to an interrupt, here we use 0x20 (IRQ 32)
        debug!("in init_timer_x2 3");wrmsr(IA32_X2APIC_LVT_TIMER, 0x20 | APIC_TIMER_PERIODIC as u64); // TODO: FIXME: change 0x20, it's the IRQ number we use for timer
        debug!("in init_timer_x2 4");wrmsr(IA32_X2APIC_INIT_COUNT, 0x100000);

        // stuff below taken from Tifflin rust-os
        debug!("in init_timer_x2 5"); wrmsr(IA32_X2APIC_LVT_THERMAL, 0);
        debug!("in init_timer_x2 6"); wrmsr(IA32_X2APIC_ESR, 0);

        // os dev wiki guys say that setting this again as a last step helps on some strange hardware.
        debug!("in init_timer_x2 7"); wrmsr(IA32_X2APIC_DIV_CONF, 3);
        debug!("in init_timer_x2 end");
    }

    unsafe fn read_reg(&self, reg: u32) -> u32 {
        assert!(!self.x2, "an x2apic system must not use the MMIO read/write_reg() functions.");
        read_volatile((self.virt_addr + reg as usize) as *const u32)
    }

    unsafe fn write_reg(&mut self, reg: u32, value: u32) {
        assert!(!self.x2, "an x2apic system must not use the MMIO read/write_reg() functions.");
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
            unsafe { (rdmsr(IA32_X2APIC_VERSION) & 0xFFFF_FFFF) as u32 }
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

    pub fn eoi(&mut self) {
        // 0 is the only valid value to write to the EOI register/msr, others cause General Protection Fault
        unsafe {
            if self.x2 {
                wrmsr(IA32_X2APIC_EOI, 0); 
            } else {
                self.write_reg(APIC_REG_EOI, 0);
            }
        }
    }


    pub fn get_isr(&mut self) -> (u32, u32, u32, u32, u32, u32, u32, u32) {
        unsafe {
            if self.x2 {
                ( 
                    rdmsr(IA32_X2APIC_ISR0) as u32, 
                    rdmsr(IA32_X2APIC_ISR1) as u32,
                    rdmsr(IA32_X2APIC_ISR2) as u32, 
                    rdmsr(IA32_X2APIC_ISR3) as u32,
                    rdmsr(IA32_X2APIC_ISR4) as u32,
                    rdmsr(IA32_X2APIC_ISR5) as u32,
                    rdmsr(IA32_X2APIC_ISR6) as u32,
                    rdmsr(IA32_X2APIC_ISR7) as u32,
                )
            }
            else {
                (
                    self.read_reg(APIC_REG_ISR + 0x00),
                    self.read_reg(APIC_REG_ISR + 0x10),
                    self.read_reg(APIC_REG_ISR + 0x20),
                    self.read_reg(APIC_REG_ISR + 0x30),
                    self.read_reg(APIC_REG_ISR + 0x40),
                    self.read_reg(APIC_REG_ISR + 0x50),
                    self.read_reg(APIC_REG_ISR + 0x60),
                    self.read_reg(APIC_REG_ISR + 0x70)
                )
            }
        }
    }


    pub fn get_irr(&mut self) -> (u32, u32, u32, u32, u32, u32, u32, u32) {
        unsafe {
            if self.x2 {
                ( 
                    rdmsr(IA32_X2APIC_IRR0) as u32, 
                    rdmsr(IA32_X2APIC_IRR1) as u32,
                    rdmsr(IA32_X2APIC_IRR2) as u32, 
                    rdmsr(IA32_X2APIC_IRR3) as u32,
                    rdmsr(IA32_X2APIC_IRR4) as u32,
                    rdmsr(IA32_X2APIC_IRR5) as u32,
                    rdmsr(IA32_X2APIC_IRR6) as u32,
                    rdmsr(IA32_X2APIC_IRR7) as u32,
                )
            }
            else {
                (
                    self.read_reg(APIC_REG_IRR + 0x00),
                    self.read_reg(APIC_REG_IRR + 0x10),
                    self.read_reg(APIC_REG_IRR + 0x20),
                    self.read_reg(APIC_REG_IRR + 0x30),
                    self.read_reg(APIC_REG_IRR + 0x40),
                    self.read_reg(APIC_REG_IRR + 0x50),
                    self.read_reg(APIC_REG_IRR + 0x60),
                    self.read_reg(APIC_REG_IRR + 0x70)
                )
            }
        }
    }
}