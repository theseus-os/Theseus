#![no_std]
#![feature(let_chains)]

use core::{fmt, sync::atomic::{AtomicU32, Ordering}};
use volatile::{Volatile, ReadOnly, WriteOnly};
use zerocopy::FromBytes;
use spin::Once;
use raw_cpuid::CpuId as X86CpuIdInstr;
use msr::*;
use irq_safety::RwLockIrqSafe;
use memory::{PageTable, PhysicalAddress, PteFlags, MappedPages, allocate_pages, allocate_frames_at, AllocatedFrames, BorrowedMappedPages, Mutable};
use kernel_config::time::CONFIG_TIMESLICE_PERIOD_MICROSECONDS;
use atomic_linked_list::atomic_map::AtomicMap;
use crossbeam_utils::atomic::AtomicCell;
use pit_clock_basic::pit_wait;
use bit_field::BitField;
use log::{error, info, debug, trace};

/// A unique identifier for a CPU core.
pub type CpuId = u8;

/// The IRQ number reserved for Local APIC timer interrupts in the IDT.
pub const LOCAL_APIC_LVT_IRQ: u8 = 0x22;
/// The IRQ number reserved for spurious APIC interrupts (as recommended by OS dev wiki).
pub const APIC_SPURIOUS_INTERRUPT_IRQ: u8 = 0xFF;

/// The interrupt chip that is currently configured on this machine. 
/// The default is `InterruptChip::PIC`, but the typical case is `APIC` or `X2APIC`,
/// which will be set once those chips have been initialized.
pub static INTERRUPT_CHIP: AtomicCell<InterruptChip> = AtomicCell::new(InterruptChip::PIC);

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(u8)]
pub enum InterruptChip {
    APIC,
    X2APIC,
    PIC,
}

// Ensure that `AtomicCell<InterruptChip>` is actually a lock-free atomic.
const _: () = assert!(AtomicCell::<InterruptChip>::is_lock_free());

/// The set of system-wide `LocalApic`s, one per CPU core.
static LOCAL_APICS: AtomicMap<CpuId, RwLockIrqSafe<LocalApic>> = AtomicMap::new();

/// The number of CPUs currently initialized in the system.
/// This must match the number of Local APICs initialized in the system.
static CPU_COUNT: AtomicU32 = AtomicU32::new(0);

/// The processor id (from the ACPI MADT table) of the bootstrap CPU.
static BSP_PROCESSOR_ID: Once<CpuId> = Once::new(); 

/// Returns the ID of the bootstrap CPU (if known),
/// which is the first CPU to run after system power-on.
pub fn bootstrap_cpu() -> Option<CpuId> {
    BSP_PROCESSOR_ID.get().cloned()
}

/// Returns true if the currently executing CPU is the bootstrap CPU, 
/// i.e., the first procesor to run after system power-on.
pub fn is_bootstrap_cpu() -> bool {
    rdmsr(IA32_APIC_BASE) & IA32_APIC_BASE_MSR_IS_BSP == IA32_APIC_BASE_MSR_IS_BSP
}

/// Returns true if the machine has support for x2apic
pub fn has_x2apic() -> bool {
    static IS_X2APIC: Once<bool> = Once::new(); // cache the result
    let res: &bool = IS_X2APIC.call_once(||
        X86CpuIdInstr::new()
            .get_feature_info()
            .expect("Couldn't get CpuId feature info")
            .has_x2apic()
    );
    *res // because call_once returns a reference to the cached IS_X2APIC value
}

/// Returns a reference to the list of LocalApics, one per CPU core.
pub fn get_lapics() -> &'static AtomicMap<CpuId, RwLockIrqSafe<LocalApic>> {
	&LOCAL_APICS
}

/// Returns the number of CPUs (SMP cores) that exist 
/// and are currently initialized on this system.
#[doc(alias("cores", "numcpus"))]
pub fn cpu_count() -> u32 {
    CPU_COUNT.load(Ordering::Relaxed)
}

/// Returns the ID of the currently executing CPU.
pub fn current_cpu() -> CpuId {
    rdmsr(IA32_TSC_AUX) as CpuId
}

/// Returns a reference to the LocalApic for the currently executing CPU core.
pub fn get_my_apic() -> Option<&'static RwLockIrqSafe<LocalApic>> {
    LOCAL_APICS.get(&current_cpu())
}


/// The possible destination shorthand values for IPI ICR.
/// 
/// See Intel manual Figure 10-28, Vol. 3A, 10-45. (PDF page 3079) 
pub enum LapicIpiDestination {
    /// Send IPI to a specific APIC 
    One(CpuId),
    /// Send IPI to my own (the current) APIC  
    Me,
    /// Send IPI to all APICs, including myself
    All,
    /// Send IPI to all APICs except for myself
    AllButMe,
}
impl LapicIpiDestination {
    /// Convert the enum to a bitmask value to be used in the interrupt command register
    pub fn as_icr_value(&self) -> u64 {
        match *self {
            LapicIpiDestination::One(apic_id) => { 
                if has_x2apic() {
                    (apic_id as u64) << 32
                } else {
                    (apic_id as u64) << 56
                }
            }
            LapicIpiDestination::Me       => 0b01 << 18, // 0x4_0000
            LapicIpiDestination::All      => 0b10 << 18, // 0x8_0000
            LapicIpiDestination::AllButMe => 0b11 << 18, // 0xC_0000
        }
    }
}


/// Determines whether this system contains an xapic or x2apic
/// and enables the Local APIC hardware in the correct mode.
pub fn init() {
    let is_x2apic = has_x2apic();
    debug!("is x2apic? {}. IA32_APIC_BASE (phys addr): {:X?}", is_x2apic, rdmsr(IA32_APIC_BASE));

    if !is_x2apic {
        // Ensure the local apic is enabled in xapic mode, otherwise we'll get a General Protection fault
        unsafe { wrmsr(IA32_APIC_BASE, rdmsr(IA32_APIC_BASE) | IA32_APIC_XAPIC_ENABLE); }
    }
}


/// Map the physical frames containing the APIC's MMIO registers into the given `page_table`.
fn map_apic(page_table: &mut PageTable) -> Result<MappedPages, &'static str> {
    static APIC_FRAME: Once<AllocatedFrames> = Once::new();

    let frame = if let Some(apic_frame) = APIC_FRAME.get() {
        apic_frame
    } else {
        let phys_addr = PhysicalAddress::new(rdmsr(IA32_APIC_BASE) as usize)
            .ok_or("APIC physical address was invalid")?;
        let apic_frame = allocate_frames_at(phys_addr, 1)?;
        APIC_FRAME.call_once(|| apic_frame)
    };

    let new_page = allocate_pages(1).ok_or("out of virtual address space!")?;
    // The APIC frame is the same actual physical address across all CPU cores,
    // but they're actually completely independent pieces of hardware that share one address.
    // Therefore, there's no way to represent that to the Rust language or MappedPages/AllocatedFrames types,
    // so we must use unsafe code, at least for now.
    unsafe {
        memory::Mapper::map_to_non_exclusive(
            page_table,
            new_page,
            frame,
            PteFlags::new().valid(true).writable(true).device_memory(true),
        )
    }
}



const IA32_APIC_XAPIC_ENABLE: u64 = 1 << 11; // 0x800
const IA32_APIC_X2APIC_ENABLE: u64 = 1 << 10; // 0x400
const IA32_APIC_BASE_MSR_IS_BSP: u64 = 1 << 8; // 0x100
const APIC_SW_ENABLE: u32 = 1 << 8;
const APIC_TIMER_PERIODIC:  u32 = 0x2_0000;
const APIC_DISABLE: u32 = 0x1_0000;
const APIC_NMI: u32 = 4 << 8;



/// A structure that offers access to APIC/xAPIC through its I/O registers.
///
/// Definitions are based on Intel's x86 Manual Vol 3a, Table 10-1. 
/// [Link to the manual](https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-3a-part-1-manual.pdf).
#[derive(FromBytes)]
#[repr(C)]
pub struct ApicRegisters {
    _padding0:                        [u32; 8],
    /// This Lapic's ID. Some systems allow setting the ID, but it is typically read-only.
    /// Only the top 8 bits are relevant, so bit shift it to the right by 24 bits to get the actual ID.
    pub lapic_id:                     Volatile<u32>,         // 0x20
    _padding1:                        [u32; 3],
    pub lapic_version:                ReadOnly<u32>,         // 0x30
    _padding2:                        [u32; 3 + 4*4],
    pub task_priority:                Volatile<u32>,         // 0x80
    _padding3:                        [u32; 3],
    pub arbitration_priority:         ReadOnly<u32>,         // 0x90
    _padding4:                        [u32; 3],
    pub processor_priority:           ReadOnly<u32>,         // 0xA0
    _padding5:                        [u32; 3],
    pub eoi:                          WriteOnly<u32>,        // 0xB0
    _padding6:                        [u32; 3],
    pub remote_read:                  ReadOnly<u32>,         // 0xC0
    _padding7:                        [u32; 3],
    pub logical_destination:          Volatile<u32>,         // 0xD0
    _padding8:                        [u32; 3],
    pub destination_format:           Volatile<u32>,         // 0xE0
    _padding9:                        [u32; 3],
    pub spurious_interrupt_vector:    Volatile<u32>,         // 0xF0
    _padding10:                       [u32; 3],
    pub in_service_registers:         RegisterArray,         // 0x100
    pub trigger_mode_registers:       RegisterArray,         // 0x180
    pub interrupt_request_registers:  RegisterArray,         // 0x200
    pub error_status:                 ReadOnly<u32>,         // 0x280
    _padding11:                       [u32; 3 + 6*4],        
    pub lvt_cmci:                     Volatile<u32>,         // 0x2F0
    _padding12:                       [u32; 3],  
    pub interrupt_command_low:        Volatile<u32>,         // 0x300
    _padding13:                       [u32; 3],
    pub interrupt_command_high:       Volatile<u32>,         // 0x310
    _padding14:                       [u32; 3],
    pub lvt_timer:                    Volatile<u32>,         // 0x320
    _padding15:                       [u32; 3],
    pub lvt_thermal:                  Volatile<u32>,         // 0x330
    _padding16:                       [u32; 3],
    pub lvt_perf_monitor:             Volatile<u32>,         // 0x340
    _padding17:                       [u32; 3],
    pub lvt_lint0:                    Volatile<u32>,         // 0x350
    _padding18:                       [u32; 3],
    pub lvt_lint1:                    Volatile<u32>,         // 0x360
    _padding19:                       [u32; 3],
    pub lvt_error:                    Volatile<u32>,         // 0x370
    _padding20:                       [u32; 3],
    pub timer_initial_count:          Volatile<u32>,         // 0x380
    _padding21:                       [u32; 3],
    pub timer_current_count:          ReadOnly<u32>,         // 0x390
    _padding22:                       [u32; 3 + 4*4],
    pub timer_divide:                 Volatile<u32>,         // 0x3E0
    _padding23:                       [u32; 3 + 4],
    // ends at 0x400
}
const _: () = assert!(core::mem::size_of::<ApicRegisters>() == 0x400);


#[derive(FromBytes)]
#[repr(C)]
pub struct RegisterArray {
    reg0:                             ReadOnly<u32>,
    _padding0:                        [u32; 3],
    reg1:                             ReadOnly<u32>,
    _padding1:                        [u32; 3],
    reg2:                             ReadOnly<u32>,
    _padding2:                        [u32; 3],
    reg3:                             ReadOnly<u32>,
    _padding3:                        [u32; 3],
    reg4:                             ReadOnly<u32>,
    _padding4:                        [u32; 3],
    reg5:                             ReadOnly<u32>,
    _padding5:                        [u32; 3],
    reg6:                             ReadOnly<u32>,
    _padding6:                        [u32; 3],
    reg7:                             ReadOnly<u32>,
    _padding7:                        [u32; 3],
}
const _: () = assert!(core::mem::size_of::<RegisterArray>() == 8 * (4 + 12));

/// The Local APIC's vector table local interrupt pins.
#[doc(alias("lvt", "lint", "lint0", "lint1"))]
pub enum LvtLint {
    Pin0,
    Pin1,
}
impl LvtLint {
    /// Returns the MSR used to access this LvtLint pin.
    #[inline]
    fn msr(&self) -> u32 {
        match self {
            Self::Pin0 => IA32_X2APIC_LVT_LINT0,
            Self::Pin1 => IA32_X2APIC_LVT_LINT1,
        }
    }
}

/// The inner type of the Local APIC (xapic or x2apic)
/// used within the [`LocalApic`] struct.
enum LapicType {
    X2Apic,
    XApic(BorrowedMappedPages<ApicRegisters, Mutable>),
}
impl fmt::Debug for LapicType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}",
            match self {
                Self::X2Apic   => "x2apic",
                Self::XApic(_) => "xapic",
            }
        )
    }
}

/// The possible errors that can occur in [`LocalApic::init()`].
#[derive(Debug)]
pub enum LapicInitError {
    /// This CPU wasn't the BSP, as expected.
    NotBSP,
    /// This CPU wasn't an AP, as expected.
    NotAP,
    /// Invalid NMI local interrupt pin value; given by the included `u8`.
    InvalidNmiLint(u8),
    UnexpectedApicID {
        expected: u8,
        actual: u8,
    },
    /// An error occurred while mapping the Local APIC's MMIO registers into memory.
    MemoryMappingError(&'static str),
    /// The Local APIC already existed (BUG), given by the included `u8` APIC ID.
    AlreadyExisted(u8),
}


/// This structure represents a single Local APIC in the system; there is one per CPU. 
#[doc(alias = "lapic")]
pub struct LocalApic {
    /// The inner lapic object that disambiguates between xapic and x2apic.
    inner: LapicType,
    /// The hardware-provided ID of this Local APIC.
    apic_id: u8,
    /// The processor ID of this APIC (from the `MADT` ACPI table entry).
    /// This is currently not used for anything in Theseus.
    processor_id: u8,
    /// Whether this Local APIC is the BootStrap Processor (the first CPU to boot up).
    is_bootstrap_cpu: bool,
}
impl fmt::Debug for LocalApic {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("LocalApic")
            .field("type", &self.inner)
            .field("apic_id", &self.apic_id)
            .field("processor_id", &self.processor_id)
            .field("is_bootstrap_cpu", &self.is_bootstrap_cpu)
            .finish()
    }
}
impl Drop for LocalApic {
    fn drop(&mut self) {
        error!("Unexpected: dropping {:?}", self);
        CPU_COUNT.fetch_sub(1, Ordering::Relaxed);
    }
}
impl LocalApic {
    /// Creates and initializes a new `LocalApic` for the current CPU core
    /// and adds it to the global set of initialized local APICs.
    /// 
    /// ## Arguments
    /// * `page_table`: the page table used to map the APIC MMIO registers
    ///    (only used for xapic, not x2apic).
    /// * `processor_id`: the processor ID specified in the ACPI `Madt` table.
    ///    This value is currently unused in Theseus.
    /// * `expected_apic_id`: the expected APIC ID as specified in the ACPI `Madt` table.
    ///    * If `Some`, the APIC's own ID (given by [`LocalApic::read_apic_id()`]) *must* match this value,
    ///      otherwise an error will be returned.
    ///    * If `None`, the local APIC will determine its own ID value, and no check is performed.
    /// * `should_be_bsp`: whether or not this CPU is expected to be the BSP.
    ///    * If `true`, this CPU must be the BSP (bootstrap processor).
    ///    * If `false`, this CPU must *not* be the BSP -- it must be an AP.
    ///    * An error is returned if the above checks fail.
    /// * `nmi_lint`: the local interrupt pin used for NMI. Must be `0` or `1`.
    /// * `nmi_flags`: the flags used to configure the local NMI interrupt.
    /// 
    /// ## Important Usage Note
    /// This MUST be invoked from each CPU itself when it is booting up, i.e.,
    /// the BSP cannot invoke this for other APs.
    pub fn init(
        page_table: &mut PageTable,
        processor_id: u8,
        expected_apic_id: Option<u8>,
        should_be_bsp: bool,
        nmi_lint: u8,
        nmi_flags: u16,
    ) -> Result<(), LapicInitError> {

        let nmi_lint = match nmi_lint {
            0 => LvtLint::Pin0,
            1 => LvtLint::Pin1,
            _invalid => {
                error!("BUG: invalid `nmi_lint` value (must be `0` or `1`) in \
                    LocalApic::init(processor_id: {}, expected_apic_id: {:?}, should_be_bsp: {}, nmi_lint: {}, nmi_flags: {}",
                    processor_id, expected_apic_id, should_be_bsp, nmi_lint, nmi_flags
                );
                return Err(LapicInitError::InvalidNmiLint(nmi_lint));
            }
        };

        // Check whether the caller's expectations about BSP vs AP were met.
        let is_bootstrap_cpu = rdmsr(IA32_APIC_BASE) & IA32_APIC_BASE_MSR_IS_BSP == IA32_APIC_BASE_MSR_IS_BSP;
        if should_be_bsp && !is_bootstrap_cpu {
            return Err(LapicInitError::NotBSP);
        }
        if !should_be_bsp && is_bootstrap_cpu {
            return Err(LapicInitError::NotAP);
        }

        // Next, before we can check other conditions, we have to enable the APIC hardware
        // (which, if xapic, also requires mapping the Local APIC's MMIO registers).
        let inner: LapicType;
        let enable_bitmask: u64;
        if has_x2apic() {
            inner = LapicType::X2Apic;
            enable_bitmask = IA32_APIC_XAPIC_ENABLE | IA32_APIC_X2APIC_ENABLE;
        } else {
            let apic_regs = map_apic(page_table)
                .map_err(LapicInitError::MemoryMappingError)
                .and_then(|apic_mp| 
                    apic_mp.into_borrowed_mut(0)
                        .map_err(|(_mp, err)| LapicInitError::MemoryMappingError(err))
                )?;

            inner = LapicType::XApic(apic_regs);
            enable_bitmask = IA32_APIC_XAPIC_ENABLE;
        };

        // Enable the xapic/x2apic hardware.
        unsafe { wrmsr(IA32_APIC_BASE, rdmsr(IA32_APIC_BASE) | enable_bitmask); }

		let mut lapic = LocalApic {
            inner,
            processor_id,
            apic_id: CpuId::MAX, // placeholder, is replaced below.
            is_bootstrap_cpu,
        };

        // Now that the APIC hardware is enabled, we can safely obtain this Local APIC's ID.
        let actual_apic_id = lapic.read_apic_id();
        if let Some(expected) = expected_apic_id && expected != actual_apic_id {
            return Err(LapicInitError::UnexpectedApicID { expected, actual: actual_apic_id });
        }

        lapic.apic_id = actual_apic_id;
        lapic.clean_enable();
        lapic.init_lvt_timer();
        lapic.set_nmi(nmi_lint, nmi_flags);
        info!("Initialized new CPU ({:?})", lapic);

        // Theseus uses this MSR to hold each CPU's ID (which is an OS-chosen value).
        unsafe { wrmsr(IA32_TSC_AUX, actual_apic_id as u64); }
        if is_bootstrap_cpu {
            BSP_PROCESSOR_ID.call_once(|| actual_apic_id); 
        }

        let _existing = LOCAL_APICS.insert(actual_apic_id, RwLockIrqSafe::new(lapic));
        if _existing.is_some() {
            return Err(LapicInitError::AlreadyExisted(actual_apic_id));
        }

        CPU_COUNT.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }


    /// Returns the "processor ID" of this local APIC, which is currently unused.
    /// 
    /// This value comes from the `MADT` ACPI table entry that was used
    /// to boot up this CPU core.
    pub fn processor_id(&self) -> u8 { self.processor_id }

    /// Returns `true` if this CPU core was the BootStrap Processor (BSP),
    /// i.e., the first CPU to boot and run the OS code.
    /// 
    /// There is only one BSP per system.
    pub fn is_bootstrap_cpu(&self) -> bool { self.is_bootstrap_cpu }

    /// Set this Local APIC to a known "clean state" and enable its spurious interrupt vector.
    fn clean_enable(&mut self) {
        let is_bootstrap_cpu = self.is_bootstrap_cpu;
        let id = self.read_apic_id();
        let version = self.version();

        match &mut self.inner {
            LapicType::X2Apic => {
                info!("LAPIC x2 ID {:#x}, version: {:#x}, is_bootstrap_cpu: {}", id, version, is_bootstrap_cpu);
                if is_bootstrap_cpu {
                    INTERRUPT_CHIP.store(InterruptChip::X2APIC);
                }

                // Init x2APIC to a known clean state.
                // Note: in x2apic, there is no DFR reg because only cluster mode is enabled; 
                //       there is no flat logical mode, and the IA32_X2APIC_LDR is read-only.
                let ldr = rdmsr(IA32_X2APIC_LDR);
                let cluster_id = (ldr >> 16) & 0xFFFF; // highest 16 bits
                let logical_id = ldr & 0xFFFF; // lowest 16 bits
                info!("x2LAPIC ID {:#x}, version {:#X}, (cluster {:#X} logical {:#X}), is_bootstrap_cpu: {}",
                    id, version, cluster_id, logical_id, is_bootstrap_cpu
                );
                // NOTE: we're not yet using logical or cluster mode APIC addressing, only physical APIC addressing.
                
                unsafe {
                    wrmsr(IA32_X2APIC_LVT_TIMER,  APIC_DISABLE as u64);
                    wrmsr(IA32_X2APIC_LVT_PMI,    APIC_NMI as u64);
                    wrmsr(IA32_X2APIC_LVT_LINT0,  APIC_DISABLE as u64);
                    wrmsr(IA32_X2APIC_LVT_LINT1,  APIC_DISABLE as u64);
                    wrmsr(IA32_X2APIC_TPR,        0);
                    
                    // set bit 8 to start receiving interrupts (still need to "sti")
                    wrmsr(IA32_X2APIC_SIVR, (APIC_SPURIOUS_INTERRUPT_IRQ as u32 | APIC_SW_ENABLE) as u64); 
                }
            }
            LapicType::XApic(regs) => {
                info!("LAPIC ID {:#x}, version: {:#x}, is_bootstrap_cpu: {}", id, version, is_bootstrap_cpu);
                if is_bootstrap_cpu {
                    INTERRUPT_CHIP.store(InterruptChip::APIC);
                }

                // Init xAPIC to a clean known state.
                // See <http://wiki.osdev.org/APIC#Logical_Destination_Mode>
                regs.destination_format.write(0xFFFF_FFFF);
                regs.lvt_timer.write(APIC_DISABLE);
                regs.lvt_perf_monitor.write(APIC_NMI);
                regs.lvt_lint0.write(APIC_DISABLE);
                regs.lvt_lint1.write(APIC_DISABLE);
                regs.task_priority.write(0);

                // set bit 8 to allow receiving interrupts (still need to "sti")
                regs.spurious_interrupt_vector.write(APIC_SPURIOUS_INTERRUPT_IRQ as u32 | APIC_SW_ENABLE);   
            }
        }
    }

    /// Returns the number of APIC ticks that occurred during the given number of `microseconds`.
    fn calibrate_lapic_timer(&mut self, microseconds: u32) -> u64 {
        // Start with the max counter value, since we're counting down
        const INITIAL_COUNT: u64 = 0xFFFF_FFFF;

        let end_count = match &mut self.inner {
            LapicType::X2Apic => {
                unsafe { 
                    wrmsr(IA32_X2APIC_DIV_CONF, 3); // set divide value to 16
                    wrmsr(IA32_X2APIC_INIT_COUNT, INITIAL_COUNT);
                }

                // wait for the given period using the PIT clock
                pit_wait(microseconds).unwrap();

                unsafe { wrmsr(IA32_X2APIC_LVT_TIMER, APIC_DISABLE as u64); } // stop apic timer
                rdmsr(IA32_X2APIC_CUR_COUNT)
            }
            LapicType::XApic(regs) => {
                regs.timer_divide.write(3); // set divide value to 16
                regs.timer_initial_count.write(INITIAL_COUNT as u32);

                // wait for the given period using the PIT clock
                pit_wait(microseconds).unwrap();

                regs.lvt_timer.write(APIC_DISABLE); // stop apic timer
                regs.timer_current_count.read() as u64
            }
        };
        
        INITIAL_COUNT - end_count
    }

    /// After this lapic has been enabled, initialize its LVT timer.
    fn init_lvt_timer(&mut self) {
        let apic_period = if cfg!(apic_timer_fixed) {
            info!("apic_timer_fixed config: overriding LocalAPIC LVT timer period to {}", 0x10000);
            0x10000 // for bochs, which doesn't do apic periods right
        } else {
            self.calibrate_lapic_timer(CONFIG_TIMESLICE_PERIOD_MICROSECONDS)
        };
        trace!("LocalApic {}, timer period count: {} ({:#X})", self.apic_id, apic_period, apic_period);

        match &mut self.inner {
            LapicType::X2Apic => unsafe {
                wrmsr(IA32_X2APIC_DIV_CONF, 3); // set divide value to 16 ( ... how does 3 => 16 )
                
                // map X2APIC timer to the `LOCAL_APIC_LVT_IRQ` interrupt handler in the IDT
                wrmsr(IA32_X2APIC_LVT_TIMER, LOCAL_APIC_LVT_IRQ as u64 | APIC_TIMER_PERIODIC as u64); 
                wrmsr(IA32_X2APIC_INIT_COUNT, apic_period); 
    
                wrmsr(IA32_X2APIC_LVT_THERMAL, 0);
                wrmsr(IA32_X2APIC_ESR, 0);
    
                // os dev wiki guys say that setting this again as a last step helps on some strange hardware.
                wrmsr(IA32_X2APIC_DIV_CONF, 3);
            }
            LapicType::XApic(regs) => {
                regs.timer_divide.write(3); // set divide value to 16 ( ... how does 3 => 16 )
                // map APIC timer to an interrupt handler in the IDT
                regs.lvt_timer.write(LOCAL_APIC_LVT_IRQ as u32 | APIC_TIMER_PERIODIC); 
                regs.timer_initial_count.write(apic_period as u32); 

                regs.lvt_thermal.write(0);
                regs.lvt_error.write(0);

                // os dev wiki guys say that setting this again as a last step helps on some strange hardware.
                regs.timer_divide.write(3);
            }
        }
    }

    /// Enable (unmask) or disable (mask) the LVT timer interrupt on this lapic.
    /// 
    /// This does **not** modify the timer's current count value.
    pub fn enable_lvt_timer(&mut self, enable: bool) {
        let value = if enable {
            LOCAL_APIC_LVT_IRQ as u32 | APIC_TIMER_PERIODIC
        } else {
            APIC_DISABLE
        };
        match &mut self.inner {
            LapicType::X2Apic => unsafe {
                wrmsr(IA32_X2APIC_LVT_TIMER, value as u64)
            },
            LapicType::XApic(regs) => regs.lvt_timer.write(value),
        }
    }

    /// Returns the ID of this Local APIC (fast).
    /// 
    /// Unlike [`LocalApic::read_apic_id()`], this does not read any hardware registers.
    pub fn apic_id(&self) -> u8 { self.apic_id }

    /// Reads the hardware-provided ID of this Local APIC from its registers (slow).
    pub fn read_apic_id(&self) -> u8 {
        match &self.inner {
            LapicType::X2Apic => rdmsr(IA32_X2APIC_APICID) as u32 as u8,
            LapicType::XApic(regs) => {
                let raw = regs.lapic_id.read();
                (raw >> 24) as u8
            }
        }
    }

    /// Returns the version of this lapic.
    pub fn version(&self) -> u32 {
        match &self.inner {
            LapicType::X2Apic => (rdmsr(IA32_X2APIC_VERSION) & 0xFFFF_FFFF) as u32,
            LapicType::XApic(regs) => regs.lapic_version.read()
        }
    }

    /// Returns the value of this lapic's error register.
    pub fn error(&self) -> u32 {
        let raw = match &self.inner {
            LapicType::X2Apic => (rdmsr(IA32_X2APIC_ESR) & 0xFFFF_FFFF) as u32,
            LapicType::XApic(regs) => regs.error_status.read(),
        };
        raw & 0x0000_00F0
    }

    /// Clears/resets this lapic's error register.
    pub fn clear_error(&mut self) {
        match &mut self.inner {
            LapicType::X2Apic => unsafe { wrmsr(IA32_X2APIC_ESR, 0) },
            LapicType::XApic(_regs) => {
                // a no-op, since apic/xapic cannot write to the error status register
            }
        }
    }

    /// Reads the current value of this lapic's Interrupt Control Register.
    pub fn icr(&self) -> u64 {
        match &self.inner {
            LapicType::X2Apic => rdmsr(IA32_X2APIC_ICR),
            LapicType::XApic(regs) => {
                let high = regs.interrupt_command_high.read();
                let low  = regs.interrupt_command_low.read();
                ((high as u64) << 32) | (low as u64)
            }
        }
    }

    /// Writes `value` to this lapic's Interrupt Control Register.
    pub fn set_icr(&mut self, value: u64) {
        match &mut self.inner {
            LapicType::X2Apic => unsafe { wrmsr(IA32_X2APIC_ICR, value) },
            LapicType::XApic(regs) => {
                const ICR_DELIVERY_STATUS: u32 = 1 << 12;
                while regs.interrupt_command_low.read() & ICR_DELIVERY_STATUS == ICR_DELIVERY_STATUS {} // wait until ready
                let high = (value >> 32) as u32;
                regs.interrupt_command_high.write(high); // sets part of ICR register, but doesn't yet issue the IPI
                let low = value as u32;
                regs.interrupt_command_low.write(low); // this actually issues the IPI
                while regs.interrupt_command_low.read() & ICR_DELIVERY_STATUS == ICR_DELIVERY_STATUS {} // wait until finished
            }
        }
    }

    /// Send an IPI to the cores specified by the given destination
    pub fn send_ipi(&mut self, irq: u8, destination: LapicIpiDestination) {
        const NORMAL_IPI_ICR: u64 = 0x4000;
        
        let dest = destination.as_icr_value();
        let icr = NORMAL_IPI_ICR | (irq as u64) | dest;

        // trace!("send_ipi(): setting icr value to {:#X}", icr);
        self.set_icr(icr);
    }


    /// Send a NMI IPI to the cores specified by the given destination
    pub fn send_nmi_ipi(&mut self, destination: LapicIpiDestination) {
        const NORMAL_IPI_ICR: u64 = 0x4000;
        const NMI_DELIVERY_MODE: u64 = 0b100 << 8;
        
        let dest = destination.as_icr_value();
        let icr = NORMAL_IPI_ICR | NMI_DELIVERY_MODE | dest;

        // trace!("send_ipi(): setting icr value to {:#X}", icr);
        self.set_icr(icr);
    }

    /// Send an End Of Interrupt (EOI) signal to this local APIC,
    /// which indicates that the calling interrupt handler has finished handling the current interrupt.
    pub fn eoi(&mut self) {
        // 0 is the only valid value to write to the EOI register/msr, others cause General Protection Fault
        match &mut self.inner {
            LapicType::X2Apic => unsafe { wrmsr(IA32_X2APIC_EOI, 0) },
            LapicType::XApic(regs) => regs.eoi.write(0),
        }
    }

    /// Set the NonMaskableInterrupt redirect for this LocalApic.
    /// Argument `lint` can be either 0 or 1, since each local APIC has two LVT LINTs
    /// (Local Vector Table Local INTerrupts)
    pub fn set_nmi(&mut self, lint: LvtLint, flags: u16) {
        let value = (flags << 12) as u32 | APIC_NMI; // or APIC_NMI | 0x2000 ??
        match &mut self.inner {
            LapicType::X2Apic => unsafe { wrmsr(lint.msr(), value as u64) },
            LapicType::XApic(regs) => match lint {
                LvtLint::Pin0 => regs.lvt_lint0.write(value),
                LvtLint::Pin1 => regs.lvt_lint1.write(value),
            }
        }
    }

    /// Returns the values of the 8 in-service registers for this APIC,
    /// which is a series of bitmasks that shows which interrupt lines are currently being serviced. 
    pub fn get_isr(&self) -> [u32; 8] {
        match &self.inner {
            LapicType::X2Apic => [
                rdmsr(IA32_X2APIC_ISR0) as u32, 
                rdmsr(IA32_X2APIC_ISR1) as u32,
                rdmsr(IA32_X2APIC_ISR2) as u32, 
                rdmsr(IA32_X2APIC_ISR3) as u32,
                rdmsr(IA32_X2APIC_ISR4) as u32,
                rdmsr(IA32_X2APIC_ISR5) as u32,
                rdmsr(IA32_X2APIC_ISR6) as u32,
                rdmsr(IA32_X2APIC_ISR7) as u32,
            ],
            LapicType::XApic(regs) => [
                regs.in_service_registers.reg0.read(),
                regs.in_service_registers.reg1.read(),
                regs.in_service_registers.reg2.read(),
                regs.in_service_registers.reg3.read(),
                regs.in_service_registers.reg4.read(),
                regs.in_service_registers.reg5.read(),
                regs.in_service_registers.reg6.read(),
                regs.in_service_registers.reg7.read(),
            ]
        }
    }

    /// Returns the values of the 8 request registers for this APIC,
    /// which is a series of bitmasks that shows which interrupt lines are currently raised, 
    /// but not yet being serviced.
    pub fn get_irr(&self) -> [u32; 8] {
        match &self.inner {
            LapicType::X2Apic => [ 
                rdmsr(IA32_X2APIC_IRR0) as u32, 
                rdmsr(IA32_X2APIC_IRR1) as u32,
                rdmsr(IA32_X2APIC_IRR2) as u32, 
                rdmsr(IA32_X2APIC_IRR3) as u32,
                rdmsr(IA32_X2APIC_IRR4) as u32,
                rdmsr(IA32_X2APIC_IRR5) as u32,
                rdmsr(IA32_X2APIC_IRR6) as u32,
                rdmsr(IA32_X2APIC_IRR7) as u32,
            ],
            LapicType::XApic(regs) => [
                regs.interrupt_request_registers.reg0.read(),
                regs.interrupt_request_registers.reg1.read(),
                regs.interrupt_request_registers.reg2.read(),
                regs.interrupt_request_registers.reg3.read(),
                regs.interrupt_request_registers.reg4.read(),
                regs.interrupt_request_registers.reg5.read(),
                regs.interrupt_request_registers.reg6.read(),
                regs.interrupt_request_registers.reg7.read(),
            ]
        }
    }

    /// Clears the interrupt mask bit in the apic performance monitor register.
    pub fn clear_pmi_mask(&mut self) {
        // The 16th bit is set to 1 whenever a performance monitoring interrupt occurs. 
        // It needs to be reset for another interrupt to occur.
        const INT_MASK_BIT: u8 = 16;

        match &mut self.inner {
            LapicType::X2Apic => {
                let mut value = rdmsr(IA32_X2APIC_LVT_PMI);
                value.set_bit(INT_MASK_BIT, false);
                unsafe { wrmsr(IA32_X2APIC_LVT_PMI, value) };
            }
            LapicType::XApic(regs) => {
                let mut value = regs.lvt_perf_monitor.read();
                value.set_bit(INT_MASK_BIT, false);
                regs.lvt_perf_monitor.write(value);
            }
        }
    }
}

// Below: temporary functions for reading MSRs that aren't yet in the `x86_64` crate.

fn rdmsr(msr: u32) -> u64 {
    unsafe { x86_64::registers::model_specific::Msr::new(msr).read() }
}

unsafe fn wrmsr(msr: u32, value: u64) {
    x86_64::registers::model_specific::Msr::new(msr).write(value)
}
