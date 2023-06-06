#![no_std]
#![allow(unused_variables, unused_mut)]

extern crate alloc;

use alloc::vec::Vec;
use cpu::CpuId;

#[cfg(target_arch = "aarch64")]
use {
    gic::{ArmGic, SpiDestination, Priority, IpiTargetCpu, Version as GicVersion},
    arm_boards::{BOARD_CONFIG, InterruptControllerConfig},
    irq_safety::MutexIrqSafe,
    memory::get_kernel_mmi_ref,
    core::ops::DerefMut,
};

#[cfg(target_arch = "x86_64")]
use {
    apic::{get_my_apic, LapicIpiDestination},
    ioapic::get_ioapic,
};

/// aarch64-specific definitions
#[cfg(target_arch = "aarch64")]
pub mod arch_def {
    use super::*;

    #[derive(Debug, Copy, Clone)]
    pub struct SystemInterruptControllerVersion(pub u8);
    #[derive(Debug, Copy, Clone)]
    pub struct      SystemInterruptControllerId(pub u8);
    #[derive(Debug, Copy, Clone)]
    pub struct       LocalInterruptControllerId(pub u16);
    #[derive(Debug, Copy, Clone)]
    pub struct            SystemInterruptNumber(pub(crate) gic::InterruptNumber);
    #[derive(Debug, Copy, Clone)]
    pub struct             LocalInterruptNumber(pub(crate) gic::InterruptNumber);

    impl SystemInterruptNumber {
        /// Constructor
        ///
        /// On aarch64, shared-peripheral interrupt numbers must lie
        /// between 32 & 1019 (inclusive)
        pub const fn new(raw_num: u32) -> Self {
            match raw_num {
                32..=1019 => Self(raw_num),
                _ => panic!("Invalid SystemInterruptNumber (must lie in 32..1020)"),
            }
        }
    }

    impl LocalInterruptNumber {
        /// Constructor
        ///
        /// On aarch64, shared-peripheral interrupt numbers must lie
        /// between 0 & 31 (inclusive)
        pub const fn new(raw_num: u32) -> Self {
            match raw_num {
                0..=31 => Self(raw_num),
                _ => panic!("Invalid LocalInterruptNumber (must lie in 0..32)"),
            }
        }
    }

    /// The private global Generic Interrupt Controller singleton
    pub(crate) static INTERRUPT_CONTROLLER: MutexIrqSafe<Option<ArmGic>> = MutexIrqSafe::new(None);

    /// Initializes the interrupt controller, on aarch64
    pub fn init() -> Result<(), &'static str> {
        let mut int_ctrl = INTERRUPT_CONTROLLER.lock();
        if int_ctrl.is_some() {
            Err("The interrupt controller has already been initialized!")
        } else {
            match BOARD_CONFIG.interrupt_controller {
                InterruptControllerConfig::GicV3(gicv3_cfg) => {
                    let kernel_mmi_ref = get_kernel_mmi_ref()
                        .ok_or("interrupts::aarch64::init: couldn't get kernel MMI ref")?;

                    let mut mmi = kernel_mmi_ref.lock();
                    let page_table = &mut mmi.deref_mut().page_table;

                    *int_ctrl = Some(ArmGic::init(
                        page_table,
                        GicVersion::InitV3 {
                            dist: gicv3_cfg.distributor_base_address,
                            redist: gicv3_cfg.redistributor_base_addresses,
                        },
                    )?);
                },
            }

            Ok(())
        }
    }
}

/// x86_64-specific definitions
#[cfg(target_arch = "x86_64")]
pub mod arch_def {
    #[derive(Debug, Copy, Clone)]
    pub struct SystemInterruptControllerVersion(pub u32);
    #[derive(Debug, Copy, Clone)]
    pub struct      SystemInterruptControllerId(pub u32);
    #[derive(Debug, Copy, Clone)]
    pub struct       LocalInterruptControllerId(pub u32);
    #[derive(Debug, Copy, Clone)]
    pub struct            SystemInterruptNumber(pub(crate) u8);
    #[derive(Debug, Copy, Clone)]
    pub struct             LocalInterruptNumber(pub(crate) u8);
    #[derive(Debug, Copy, Clone)]
    pub struct Priority;

    /// Initializes the interrupt controller, on aarch64
    pub fn init() -> Result<(), &'static str> { Ok(()) }
}

pub use arch_def::*;

#[cfg(target_arch = "aarch64")]
macro_rules! get_int_ctlr {
    ($name:ident, $func:ident, $this:expr) => {
        let mut $name = INTERRUPT_CONTROLLER.lock();
        let $name = $name.as_mut().expect(concat!("BUG: ", stringify!($func), "(): INTERRUPT_CONTROLLER was uninitialized"));
    };
    ($name:ident, $func:ident) => ( get_int_ctlr!($name, $func, ()) );
}

#[cfg(target_arch = "x86_64")]
macro_rules! get_int_ctlr {
    ($name:ident, $func:ident, $this:expr) => {
        let mut $name = get_ioapic($this.id).expect(concat!("BUG: ", stringify!($func), "(): get_ioapic() returned None"));
    };
    ($name:ident, $func:ident) => {
        let mut $name = get_my_apic().expect(concat!("BUG: ", stringify!($func), "(): get_my_apic() returned None"));
        let mut $name = $name.write();
    };
}

/// The Cpu were this interrupt should be handled, as well as
/// the local interrupt number this gets translated to.
///
/// On aarch64, the system interrupt number and the local interrupt
/// number must be the same.
#[derive(Debug, Copy, Clone)]
pub struct InterruptDestination {
    pub cpu: CpuId,
    pub local_number: LocalInterruptNumber,
}

/// Structure representing a top-level/system-wide interrupt controller chip,
/// responsible for routing interrupts between peripherals and CPU cores.
///
/// On x86_64, this corresponds to an IoApic.
/// On aarch64 w/ GIC, this corresponds to the Distributor.
pub struct SystemInterruptController {
    #[cfg(target_arch = "x86_64")]
    id: u8,
}

/// Struct representing per-cpu-core interrupt controller chips.
///
/// On x86_64, this corresponds to a LocalApic.
/// On aarch64 w/ GIC, this corresponds to a Redistributor & CPU interface.
pub struct LocalInterruptController {}

impl SystemInterruptController {
    pub fn id(&self) -> SystemInterruptControllerId {
        get_int_ctlr!(int_ctlr, id, self);

        #[cfg(target_arch = "aarch64")] {
            SystemInterruptControllerId(int_ctlr.implementer().product_id)
        }

        #[cfg(target_arch = "x86_64")] {
            SystemInterruptControllerId(int_ctlr.id())
        }
    }

    pub fn version(&self) -> SystemInterruptControllerVersion {
        get_int_ctlr!(int_ctlr, version, self);

        #[cfg(target_arch = "aarch64")] {
            SystemInterruptControllerVersion(int_ctlr.implementer().version)
        }

        #[cfg(target_arch = "x86_64")] {
            SystemInterruptControllerVersion(int_ctlr.version())
        }
    }

    pub fn get_destination(&self, interrupt_num: SystemInterruptNumber) -> Result<(Vec<InterruptDestination>, Priority), &'static str> {
        get_int_ctlr!(int_ctlr, get_destination, self);

        #[cfg(target_arch = "aarch64")] {
            let priority = int_ctlr.get_interrupt_priority(interrupt_num.0);
            let local_number = LocalInterruptNumber(interrupt_num.0);

            let vec = match int_ctlr.get_spi_target(interrupt_num.0)?.canonicalize() {
                SpiDestination::Specific(cpu) => [InterruptDestination {
                    cpu,
                    local_number,
                }].to_vec(),
                SpiDestination::AnyCpuAvailable => BOARD_CONFIG.cpu_ids.map(|mpidr| InterruptDestination {
                    cpu: mpidr.into(),
                    local_number,
                }).to_vec(),
                SpiDestination::GICv2TargetList(list) => {
                    let mut vec = Vec::with_capacity(8);
                    for result in list.iter() {
                        vec.push(InterruptDestination {
                            cpu: result?,
                            local_number,
                        });
                    }
                    vec
                }
            };

            Ok((vec, priority))
        }

        #[cfg(target_arch = "x86_64")] {
            // no way to read the destination for an IRQ number in IoApic
            unimplemented!()
        }
    }

    pub fn set_destination(&self, sys_int_num: SystemInterruptNumber, destination: InterruptDestination, priority: Priority) -> Result<(), &'static str> {
        get_int_ctlr!(int_ctlr, set_destination, self);

        #[cfg(target_arch = "aarch64")] {
            assert_eq!(sys_int_num.0, destination.local_number.0, "Local & System Interrupt Numbers cannot be different with GIC");

            int_ctlr.set_spi_target(sys_int_num.0, SpiDestination::Specific(destination.cpu));
            int_ctlr.set_interrupt_priority(sys_int_num.0, priority);

            Ok(())
        }

        #[cfg(target_arch = "x86_64")] {
            // no support for priority on x86_64
            let _ = priority;

            int_ctlr.set_irq(sys_int_num.0, destination.cpu.into(), destination.local_number.0)
        }
    }
}

impl LocalInterruptController {
    pub fn id(&self) -> LocalInterruptControllerId {
        get_int_ctlr!(int_ctlr, id);

        #[cfg(target_arch = "aarch64")] {
            LocalInterruptControllerId(int_ctlr.get_cpu_interface_id())
        }

        #[cfg(target_arch = "x86_64")] {
            // this or the Apic ID ?
            LocalInterruptControllerId(int_ctlr.processor_id())
        }
    }

    pub fn get_local_interrupt_priority(&self, num: LocalInterruptNumber) -> Priority {
        get_int_ctlr!(int_ctlr, get_local_interrupt_priority);

        #[cfg(target_arch = "aarch64")] {
            int_ctlr.get_interrupt_priority(num.0)
        }

        #[cfg(target_arch = "x86_64")] {
            // No priority support on x86_64
            Priority
        }
    }

    pub fn set_local_interrupt_priority(&self, num: LocalInterruptNumber, priority: Priority) {
        get_int_ctlr!(int_ctlr, set_local_interrupt_priority);

        #[cfg(target_arch = "aarch64")] {
            int_ctlr.set_interrupt_priority(num.0, priority);
        }

        #[cfg(target_arch = "x86_64")] {
            // No priority support on x86_64
            let _ = priority;
        }
    }

    pub fn is_local_interrupt_enabled(&self, num: LocalInterruptNumber) -> bool {
        get_int_ctlr!(int_ctlr, is_local_interrupt_enabled);

        #[cfg(target_arch = "aarch64")] {
            int_ctlr.get_interrupt_state(num.0)
        }

        #[cfg(target_arch = "x86_64")] {
            todo!()
        }
    }

    pub fn enable_local_interrupt(&self, num: LocalInterruptNumber, enabled: bool) {
        get_int_ctlr!(int_ctlr, enable_local_interrupt);

        #[cfg(target_arch = "aarch64")] {
            int_ctlr.set_interrupt_state(num.0, enabled);
        }

        #[cfg(target_arch = "x86_64")] {
            todo!()
        }
    }

    /// Sends an inter-processor interrupt to a specific CPU.
    pub fn send_ipi(&self, destination: InterruptDestination) {
        get_int_ctlr!(int_ctlr, send_ipi);

        #[cfg(target_arch = "aarch64")] {
            int_ctlr.send_ipi(destination.local_number.0, IpiTargetCpu::Specific(destination.cpu));
        }

        #[cfg(target_arch = "x86_64")] {
            int_ctlr.send_ipi(destination.local_number.0, LapicIpiDestination::One(destination.cpu.into()))
        }
    }

    /// Reads the minimum priority for an interrupt to reach this CPU.
    ///
    /// Note: aarch64-only, at the moment.
    pub fn get_minimum_priority(&self) -> Priority {
        get_int_ctlr!(int_ctlr, get_minimum_priority);

        #[cfg(target_arch = "aarch64")] {
            int_ctlr.get_minimum_priority()
        }

        #[cfg(target_arch = "x86_64")] {
            // No priority support on x86_64
            Priority
        }
    }

    /// Changes the minimum priority for an interrupt to reach this CPU.
    ///
    /// Note: aarch64-only, at the moment.
    pub fn set_minimum_priority(&self, priority: Priority) {
        get_int_ctlr!(int_ctlr, set_minimum_priority);

        #[cfg(target_arch = "aarch64")] {
            int_ctlr.set_minimum_priority(priority)
        }

        #[cfg(target_arch = "x86_64")] {
            // No priority support on x86_64
            let _ = priority;
        }
    }

    /// Aarch64-specific way to read the current pending interrupt number & priority.
    #[cfg(target_arch = "aarch64")]
    pub fn acknowledge_interrupt(&self) -> (LocalInterruptNumber, Priority) {
        get_int_ctlr!(int_ctlr, acknowledge_interrupt);

        #[cfg(target_arch = "aarch64")] {
            let (num, prio) = int_ctlr.acknowledge_interrupt();

            (LocalInterruptNumber(num), prio)
        }

        #[cfg(target_arch = "x86_64")] {
            panic!("This is an aarch64-only mechanism.");
        }
    }

    /// Tell the interrupt controller that the current interrupt handling as ended.
    pub fn end_of_interrupt(&self, number: LocalInterruptNumber) {
        get_int_ctlr!(int_ctlr, end_of_interrupt);

        #[cfg(target_arch = "aarch64")] {
            int_ctlr.end_of_interrupt(number.0)
        }

        #[cfg(target_arch = "x86_64")] {
            // what do we do with the IRQ number?
            int_ctlr.eoi();
        }
    }
}
