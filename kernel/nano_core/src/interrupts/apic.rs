use core::ptr::{read_volatile, write_volatile};
use spin::{Mutex, MutexGuard, Once};
use x86::current::cpuid::CpuId;
use x86::shared::msr::*;
use core::ops::DerefMut;
use memory::{FRAME_ALLOCATOR, Frame, ActivePageTable, PhysicalAddress, Page, VirtualAddress, EntryFlags};
use kernel_config::memory::{APIC_START};
use atomic_linked_list::atomic_map::AtomicMap;
use drivers::acpi::madt::{MadtEntry, MadtIter};
use core::sync::atomic::{AtomicUsize, AtomicBool, Ordering};



/// The IRQ number used for IPIs
pub const TLB_SHOOTDOWN_IPI_IRQ: u8 = 0x40;
/// The virtual address used for TLB shootdown IPIs
pub static TLB_SHOOTDOWN_IPI_VIRT_ADDR: AtomicUsize = AtomicUsize::new(usize::max_value()); 
/// The number of remaining cores that still need to handle the curerent TLB shootdown IPI
pub static TLB_SHOOTDOWN_IPI_COUNT: AtomicUsize =AtomicUsize::new(usize::max_value()); 
/// The lock that makes sure only one set of TLB shootdown IPIs is concurrently happening
pub static TLB_SHOOTDOWN_IPI_LOCK: AtomicBool = AtomicBool::new(false);



lazy_static! {
    static ref LOCAL_APICS: AtomicMap<u8, LocalApic> = AtomicMap::new();
}

/// The VirtualAddress where the APIC chip has been mapped.
static APIC_VIRT_ADDR: Once<VirtualAddress> = Once::new();

/// The processor id (from the ACPI MADT table) of the bootstrap processor
static BSP_PROCESSOR_ID: Once<u8> = Once::new(); 

pub fn get_bsp_id() -> Option<u8> {
    BSP_PROCESSOR_ID.try().cloned()
}

/// Returns true if the currently executing processor core is the bootstrap processor, 
/// i.e., the first procesor to run 
pub fn is_bsp() -> bool {
    unsafe { 
        rdmsr(IA32_APIC_BASE) & IA32_APIC_BASE_MSR_IS_BSP == IA32_APIC_BASE_MSR_IS_BSP
    }
}

/// Returns true if the machine has support for x2apic
pub fn has_x2apic() -> bool {
    static IS_X2APIC: Once<bool> = Once::new(); // caches the result
    let res: &bool = IS_X2APIC.call_once( || {
        CpuId::new().get_feature_info().unwrap().has_x2apic()
    });
    *res // because call_once returns a reference to the cached IS_X2APIC value
}

/// Returns a reference to the list of LocalApics, one per processor core
pub fn get_lapics() -> &'static AtomicMap<u8, LocalApic> {
	&LOCAL_APICS
}


/// Returns the APIC ID of the currently executing processor core
pub fn get_my_apic_id() -> Option<u8> {
    let raw = if has_x2apic() {
        unsafe { rdmsr(IA32_X2APIC_APICID) as u32 }
    } else {
        match APIC_VIRT_ADDR.try() {
            Some(apic_start) => unsafe { read_volatile((apic_start + APIC_REG_LAPIC_ID as usize) as *const u32) },
            None => {
                return None;
            }
        }
    };
    Some((raw >> 24) as u8)
}


/// The possible destination shorthand values for IPI ICR. 
/// See Intel manual Figure 10-28, Vol. 3A, 10-45. (PDF page 3079) 
pub enum LapicIpiDestination {
    One(u8),
    Me,
    All,
    AllButMe,
}
impl LapicIpiDestination {
    pub fn as_icr_value(&self) -> u64 {
        match self {
            &LapicIpiDestination::One(apic_id) => { 
                if has_x2apic() {
                    (apic_id as u64) << 32
                } else {
                    (apic_id as u64) << 56
                }
            }
            &LapicIpiDestination::Me           => 0b01 << 18, // 0x4_0000
            &LapicIpiDestination::All          => 0b10 << 18, // 0x8_0000
            &LapicIpiDestination::AllButMe     => 0b11 << 18, // 0xC_0000
        }
    }
}


/// initially maps the base APIC MMIO register frames so that we can know which LAPIC (processor core) we are,
/// and because it only needs to be done once -- not every time we bring up a new AP core
pub fn init(active_table: &mut ActivePageTable) {
    assert_has_not_been_called!("Error: tried to call apic::init() more than once!");

    let x2 = has_x2apic();
    let phys_addr = unsafe { rdmsr(IA32_APIC_BASE) };
    debug!("is x2apic? {}.  IA32_APIC_BASE: {:#X}", x2, phys_addr);
    // x2apic doesn't require MMIO, it just uses MSRs instead, so we don't need to map the APIC registers.
    // x2apic is better because it's easier to write a 64-bit value directly into an MSR instead of 
    // writing 2 separate 32-bit values into adjacent 32-bit APIC memory-mapped I/O registers.
    let virt_addr = APIC_START as VirtualAddress;
    if !has_x2apic() {
        let page = Page::containing_address(virt_addr);
        let frame = Frame::containing_address(phys_addr as PhysicalAddress);
        let mut fa = FRAME_ALLOCATOR.try().unwrap().lock();
        active_table.map_to(page, frame, EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE, fa.deref_mut());
    }

    APIC_VIRT_ADDR.call_once( || virt_addr);
}

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
const APIC_REG_ICR_HIGH               : u32 =  0x310;	// Interrupt Command Register (2/2)
const APIC_REG_LVT_TIMER              : u32 =  0x320;
const APIC_REG_LVT_THERMAL            : u32 =  0x330; // Thermal sensor
const APIC_REG_LVT_PMI                : u32 =  0x340; // Performance Monitoring information
const APIC_REG_LVT_LINT0              : u32 =  0x350;
const APIC_REG_LVT_LINT1              : u32 =  0x360;
const APIC_REG_LVT_ERROR              : u32 =  0x370;
const APIC_REG_INIT_COUNT             : u32 =  0x380;
const APIC_REG_CURRENT_COUNT          : u32 =  0x390;
const APIC_REG_TIMER_DIVIDE           : u32 =  0x3E0;



/// Local APIC
#[derive(Debug)]
pub struct LocalApic {
    pub virt_addr: VirtualAddress,
    pub phys_addr: PhysicalAddress,
    pub processor: u8,
    pub apic_id: u8,
    pub flags: u32,
    pub is_bsp: bool,
}

impl LocalApic {
    /// This MUST be invoked from the AP core that is booting up.
    /// The BSP cannot invoke this for other APs (it can only invoke it for itself).
    pub fn new(processor: u8, apic_id: u8, flags: u32, is_bsp: bool, madt_iter: MadtIter) -> LocalApic {
		
        assert!(flags == 1, "LocalApic::create() processor was disabled! (flags != 1)");
		let mut lapic = LocalApic {
			virt_addr: APIC_START as VirtualAddress,
            // redox bitmasked the base paddr with 0xFFFF_0000, os dev wiki says 0xFFFF_F000 ...
            // seems like 0xFFFF_F000 is more correct since it just frame/page-aligns the address
            phys_addr: ( unsafe { rdmsr(IA32_APIC_BASE) } as usize & 0xFFFF_F000) as PhysicalAddress,
            processor: processor,
            apic_id: apic_id,
            flags: flags,
            is_bsp: is_bsp,
		};

        if is_bsp {
            BSP_PROCESSOR_ID.call_once( || apic_id); 
        }

        unsafe {
            if has_x2apic() { 
                lapic.enable_x2apic();
                lapic.init_timer_x2();  // this should be called later once the IDT is fully populated
            } 
            else { 
                lapic.enable_apic();
                lapic.init_timer(); // this should be called later once the IDT is fully populated
            }
        }

        lapic.parse_and_set_nmi(madt_iter);
        info!("Found new processor core ({:?})", lapic);
		lapic
    }

    /// enables the spurious interrupt vector, which enables the APIC itself.
    unsafe fn enable_apic(&mut self) {
        assert!(!has_x2apic(), "an x2apic system must not use enable_apic(), it should use enable_x2apic() instead.");

        // globally enable the apic by setting the xapic_enable bit
        let bsp_bit = rdmsr(IA32_APIC_BASE) & IA32_APIC_BASE_MSR_IS_BSP; // need to preserve this when we set other bits in the APIC_BASE reg
        wrmsr(IA32_APIC_BASE, rdmsr(IA32_APIC_BASE) | bsp_bit | IA32_APIC_XAPIC_ENABLE);
        let is_bsp = bsp_bit == IA32_APIC_BASE_MSR_IS_BSP;
        info!("LAPIC ID {:#x}, version: {:#x}, is_bsp: {}", self.id(), self.version(), is_bsp);
        if is_bsp {
            ::interrupts::INTERRUPT_CHIP.store(::interrupts::InterruptChip::APIC, ::atomic::Ordering::Release);
        }


        // init APIC to a clean state
        // see this: http://wiki.osdev.org/APIC#Logical_Destination_Mode
        // let old_dfr = self.read_reg(APIC_REG_DFR);
        // self.write_reg(APIC_REG_DFR, old_dfr | 0xF000_0000); 

        self.write_reg(APIC_REG_DFR, 0xFFFF_FFFF); // Flat destination mode (only bits [31:27] matter, should be all 1s. All 0s would be cluster mode)
        // let old_ldr = self.read_reg(APIC_REG_LDR);
        // debug!("old_dr: {:#x}, old_ldr: {:#x}", old_dfr, old_ldr);
        // TODO FIXME: i think the below line might set the IPI destination as processor 0  (1 << 0) ???
        //             or perhaps it's a bitmask of which destination apic_ids the current (BSP) core will accept IPIs for?
        // self.write_reg(APIC_REG_LDR, old_ldr & 0x00FF_FFFF | 1); // 2  | 4 | 8);  // (1 << (24 + 1)) /* 1 << 0 ... apic_id?? */); // flat logical addressing
        // above: don't set LDR (just to be consistent with steps in x2apic mode, because in x2apic mode we cannot set our own APIC IDs, they're read only)
        
        // Now, I'm pretty sure that reading the LDR will give us the current core's apic_id
        info!("enable_apic(): LDR = {:#X}", self.read_reg(APIC_REG_LDR));
        self.write_reg(APIC_REG_LVT_TIMER, APIC_DISABLE);
        self.write_reg(APIC_REG_LVT_PMI, APIC_NMI);
        self.write_reg(APIC_REG_LVT_LINT0, APIC_DISABLE);
        self.write_reg(APIC_REG_LVT_LINT1, APIC_DISABLE);
        self.write_reg(APIC_REG_TASK_PRIORITY, 0);


        // set bit 8 to start receiving interrupts (still need to "sti")
        self.write_reg(APIC_REG_SIR, APIC_SPURIOUS_INTERRUPT_VECTOR | APIC_SW_ENABLE); 
    }

    unsafe fn enable_x2apic(&mut self) {
        assert!(has_x2apic(), "an apic/xapic system must not use enable_x2apic(), it should use enable_apic() instead.");
        
        debug!("in enable_x2apic");
        // globally enable the x2apic, which includes also setting the xapic enable bit
        let bsp_bit = rdmsr(IA32_APIC_BASE) & IA32_APIC_BASE_MSR_IS_BSP; // need to preserve this when we set other bits in the APIC_BASE reg
        debug!("in enable_x2apic 1: bsp_bit: {:#X}", bsp_bit);
        wrmsr(IA32_APIC_BASE, rdmsr(IA32_APIC_BASE) | bsp_bit | IA32_APIC_XAPIC_ENABLE | IA32_APIC_X2APIC_ENABLE);
        debug!("in enable_x2apic 2: new apic_base: {:#X}", rdmsr(IA32_APIC_BASE));
        let is_bsp = bsp_bit == IA32_APIC_BASE_MSR_IS_BSP;
        if is_bsp {
            ::interrupts::INTERRUPT_CHIP.store(::interrupts::InterruptChip::x2apic, ::atomic::Ordering::Release);
        }

        // init x2APIC to a clean state, just as in enable_apic() above 
        // Note: in x2apic, there is not DFR reg because only cluster mode is enabled; there is no flat logical mode
        // Note: in x2apic, the IA32_X2APIC_LDR is read-only.
        let ldr = rdmsr(IA32_X2APIC_LDR);
        debug!("in enable_x2apic 0.1");
        let cluster_id = (ldr >> 16) & 0xFFFF; // highest 16 bits
        let logical_id = ldr & 0xFFFF; // lowest 16 bits
        info!("x2LAPIC ID {:#x} (cluster {:#X} logical {:#X}), is_bsp: {}", self.version(), cluster_id, logical_id, is_bsp);
        debug!("in enable_x2apic 0.2"); wrmsr(IA32_X2APIC_LVT_TIMER, APIC_DISABLE as u64);
        debug!("in enable_x2apic 0.3"); wrmsr(IA32_X2APIC_LVT_PMI, APIC_NMI as u64);
        debug!("in enable_x2apic 0.4"); wrmsr(IA32_X2APIC_LVT_LINT0, APIC_DISABLE as u64);
        debug!("in enable_x2apic 0.5"); wrmsr(IA32_X2APIC_LVT_LINT1, APIC_DISABLE as u64);
        debug!("in enable_x2apic 0.6"); wrmsr(IA32_X2APIC_TPR, 0);
        
        
        
        wrmsr(IA32_X2APIC_SIVR, (APIC_SPURIOUS_INTERRUPT_VECTOR | APIC_SW_ENABLE) as u64); // set bit 8 to start receiving interrupts (still need to "sti")
        debug!("in enable_x2apic end");
        info!("x2LAPIC ID {:#x}  is_bsp: {}", self.id(), is_bsp);
    }


    pub unsafe fn init_timer(&mut self) {
        assert!(!has_x2apic(), "an x2apic system must not use init_timer(), it should use init_timerx2() instead.");

        self.write_reg(APIC_REG_TIMER_DIVIDE, 3); // set divide value to 16 ( ... how does 3 => 16 )
        // map APIC timer to an interrupt, here we use 0x20 (IRQ 32)
        self.write_reg(APIC_REG_LVT_TIMER, 0x20 | APIC_TIMER_PERIODIC); // TODO: FIXME: change 0x20, it's the IRQ number we use for timer
        self.write_reg(APIC_REG_INIT_COUNT, 0x800000);

        // stuff below taken from Tifflin rust-os
        self.write_reg(APIC_REG_LVT_THERMAL, 0);
        self.write_reg(APIC_REG_LVT_ERROR, 0);

        // os dev wiki guys say that setting this again as a last step helps on some strange hardware.
        self.write_reg(APIC_REG_TIMER_DIVIDE, 3); 
    }

    pub unsafe fn init_timer_x2(&mut self) {
        assert!(has_x2apic(), "an apic/xapic system must not use init_timerx2(), it should use init_timer() instead.");
        debug!("in init_timer_x2 start");
        debug!("in init_timer_x2 2"); wrmsr(IA32_X2APIC_DIV_CONF, 3); // set divide value to 16 ( ... how does 3 => 16 )
        // map APIC timer to an interrupt, here we use 0x20 (IRQ 32)
        debug!("in init_timer_x2 3");wrmsr(IA32_X2APIC_LVT_TIMER, 0x20 | APIC_TIMER_PERIODIC as u64); // TODO: FIXME: change 0x20, it's the IRQ number we use for timer
        debug!("in init_timer_x2 4");wrmsr(IA32_X2APIC_INIT_COUNT, 0x800000);

        // stuff below taken from Tifflin rust-os
        debug!("in init_timer_x2 5"); wrmsr(IA32_X2APIC_LVT_THERMAL, 0);
        debug!("in init_timer_x2 6"); wrmsr(IA32_X2APIC_ESR, 0);

        // os dev wiki guys say that setting this again as a last step helps on some strange hardware.
        debug!("in init_timer_x2 7"); wrmsr(IA32_X2APIC_DIV_CONF, 3);
        debug!("in init_timer_x2 end");
    }


    /// Parses and sets up the NonMaskableInterrupt (NMI) for this LocalApic,
    /// based on the entries in the given `MadtIter`.
    fn parse_and_set_nmi(&mut self, madt_iter: MadtIter) {
        for madt_entry in madt_iter {
            match madt_entry {
                MadtEntry::NonMaskableInterrupt(nmi) => {
                    trace!("Lapic {} looking at NMI entry {:?}", self.apic_id, nmi);
                    // if this is an NMI entry for this lapic, or for all lapics, use it
                    if nmi.processor == self.apic_id || nmi.processor == 0xFF  {
                        self.set_nmi(nmi.lint, nmi.flags);   
                        debug!("Set NMI for LocalApic {}, NMI Entry: {:?}", self.apic_id, nmi);                     
                    }
                }
                _ => {  }
            }
        }

    }

    unsafe fn read_reg(&self, reg: u32) -> u32 {
        assert!(!has_x2apic(), "an x2apic system must not use the MMIO read/write_reg() functions.");
        read_volatile((self.virt_addr + reg as usize) as *const u32)
    }

    unsafe fn write_reg(&mut self, reg: u32, value: u32) {
        assert!(!has_x2apic(), "an x2apic system must not use the MMIO read/write_reg() functions.");
        write_volatile((self.virt_addr + reg as usize) as *mut u32, value);
    }

    pub fn id(&self) -> u8 {
        let raw = if has_x2apic() {
            unsafe { rdmsr(IA32_X2APIC_APICID) as u32 }
        } else {
            unsafe { self.read_reg(APIC_REG_LAPIC_ID) }
        };
        let id = (raw >> 24) as u8;
        assert!(id == self.apic_id, "LocalApic::id() wasn't the same as given apic_id!");
        id
    }

    pub fn version(&self) -> u32 {
        if has_x2apic() {
            unsafe { (rdmsr(IA32_X2APIC_VERSION) & 0xFFFF_FFFF) as u32 }
        } else {
            unsafe { self.read_reg(APIC_REG_LAPIC_VERSION) }
        }
    }

    pub fn icr(&self) -> u64 {
        if has_x2apic() {
            unsafe { rdmsr(IA32_X2APIC_ICR) }
        } else {
            unsafe {
                (self.read_reg(APIC_REG_ICR_HIGH) as u64) << 32 | self.read_reg(APIC_REG_ICR_LOW) as u64
            }
        }
    }

    pub fn set_icr(&mut self, value: u64) {
        if has_x2apic() {
            unsafe { wrmsr(IA32_X2APIC_ICR, value); }
        } else {
            unsafe {
                while self.read_reg(APIC_REG_ICR_LOW) & 1 << 12 == 1 << 12 {} // wait until ready
                let high = (value >> 32) as u32;
                self.write_reg(APIC_REG_ICR_HIGH, high); // sets part of ICR register, but doesn't yet issue the IPI
                let low = value as u32;
                self.write_reg(APIC_REG_ICR_LOW, low ); // this actually issues the IPI
                while self.read_reg(APIC_REG_ICR_LOW) & 1 << 12 == 1 << 12 {} // wait until finished
            }
        }
    }

    /// Send an IPI to the cores specified by the given destination
    pub fn send_ipi(&mut self, irq: u8, destination: LapicIpiDestination) {
        const NORMAL_IPI_ICR: u64 = 0x4000;

        let _held_ints = ::irq_safety::hold_interrupts();
        
        let dest = destination.as_icr_value();
        let icr = NORMAL_IPI_ICR | (irq as u64) | dest;

        // trace!("send_ipi(): setting icr value to {:#X}", icr);
        self.set_icr(icr);

        // interrupts are released (restored) here when _held_ints is dropped
    }


    /// Sends an IPI to all other cores (except me) to trigger 
    /// a TLB flush of the given `VirtualAddress`
    pub fn send_tlb_shootdown_ipi(&mut self, vaddr: VirtualAddress) {
        // temporary page is not shared across cores
        use kernel_config::memory::{TEMPORARY_PAGE_VIRT_ADDR, PAGE_SIZE};
        const TEMPORARY_PAGE_FRAME: usize = TEMPORARY_PAGE_VIRT_ADDR & !(PAGE_SIZE - 1);
        if vaddr == TEMPORARY_PAGE_FRAME { 
            return;
        }
        
        let core_count = get_lapics().iter().count();
        if core_count <= 1 {
            return; // skip sending IPIs if there are no other cores running
        }
        trace!("send_tlb_shootdown_ipi(): from AP {}, vaddr: {:#X}, core_count: {}", 
                get_my_apic_id().unwrap_or(0xff), vaddr, core_count);

        {
            // acquire lock
            while TLB_SHOOTDOWN_IPI_LOCK.compare_and_swap(false, true, Ordering::SeqCst) {
                ::arch::pause();
            }


            TLB_SHOOTDOWN_IPI_VIRT_ADDR.store(vaddr, Ordering::Release);
            TLB_SHOOTDOWN_IPI_COUNT.store(core_count - 1, Ordering::SeqCst); // - 1 to exclude this core
            
            self.send_ipi(TLB_SHOOTDOWN_IPI_IRQ, LapicIpiDestination::AllButMe); // send IPI to all other cores but this one

            // wait for all other cores to handle this IPI
            // it must be a blocking, synchronous operation to ensure stale TLB entries don't cause problems
            while TLB_SHOOTDOWN_IPI_COUNT.load(Ordering::SeqCst) > 0  { 
                ::arch::pause();
            }
        
            // release lock
            TLB_SHOOTDOWN_IPI_LOCK.store(false, Ordering::SeqCst); 
        }
    }


    pub fn eoi(&mut self) {
        // 0 is the only valid value to write to the EOI register/msr, others cause General Protection Fault
        unsafe {
            if has_x2apic() {
                wrmsr(IA32_X2APIC_EOI, 0); 
            } else {
                self.write_reg(APIC_REG_EOI, 0);
            }
        }
    }


    pub fn set_ldr(&mut self, value: u32) {
        assert!(!has_x2apic(),"set_ldr(): Setting LDR MSR for x2apic is forbidden! (causes GPF)");
        unsafe {
            let old_ldr = self.read_reg(APIC_REG_LDR);
            self.write_reg(APIC_REG_LDR, old_ldr & 0x00FF_FFFF | value);
        }
    }

    /// lint can be either 0 or 1, since each local APIC has two LVT LINTs
    /// (Local Vector Table Local INTerrupts)
    pub fn set_nmi(&mut self, lint: u8, flags: u16) {
        unsafe {
            if has_x2apic() {
                match lint {
                    0 => wrmsr(IA32_X2APIC_LVT_LINT0, (flags << 12) as u64 | APIC_NMI as u64), // or APIC_NMI | 0x2000 ??
                    1 => wrmsr(IA32_X2APIC_LVT_LINT1, (flags << 12) as u64 | APIC_NMI as u64),
                    _ => panic!("set_nmi(): invalid lint {}!"),
                }
            } else {
                match lint {
                    0 => self.write_reg(APIC_REG_LVT_LINT0, (flags << 12) as u32 | APIC_NMI), // or APIC_NMI | 0x2000 ??
                    1 => self.write_reg(APIC_REG_LVT_LINT1, (flags << 12) as u32 | APIC_NMI),
                    _ => panic!("set_nmi(): invalid lint {}!"),
                }
            }
        }
    }


    pub fn get_isr(&mut self) -> (u32, u32, u32, u32, u32, u32, u32, u32) {
        unsafe {
            if has_x2apic() {
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
            if has_x2apic() {
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


/// Handles a TLB shootdown ipi by flushing the VirtualAddress 
/// currently stored in TLB_SHOOTDOWN_IPI_VIRT_ADDR.
pub fn handle_tlb_shootdown_ipi() {
    let apic_id = get_my_apic_id().unwrap_or(0xFF);
    let vaddr = TLB_SHOOTDOWN_IPI_VIRT_ADDR.load(Ordering::Acquire);

    trace!("handle_tlb_shootdown_ipi(): flushing vaddr {:#X}", vaddr);

    use x86_64::instructions::tlb;
    tlb::flush(::x86_64::VirtualAddress(vaddr));

    TLB_SHOOTDOWN_IPI_COUNT.fetch_sub(1, Ordering::SeqCst);
}