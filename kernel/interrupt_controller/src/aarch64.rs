use {
    gic::{ArmGic, SpiDestination, IpiTargetCpu, Version as GicVersion},
    arm_boards::{BOARD_CONFIG, InterruptControllerConfig},
    irq_safety::MutexIrqSafe,
    memory::get_kernel_mmi_ref,
    core::ops::DerefMut,
};

use super::*;

pub use gic::Priority;

#[derive(Debug, Copy, Clone)]
pub struct SystemInterruptControllerVersion(pub u8);
#[derive(Debug, Copy, Clone)]
pub struct SystemInterruptControllerId(pub u8);
#[derive(Debug, Copy, Clone)]
pub struct LocalInterruptControllerId(pub u16);
#[derive(Debug, Copy, Clone)]
pub struct SystemInterruptNumber(pub(crate) gic::InterruptNumber);
#[derive(Debug, Copy, Clone)]
pub struct LocalInterruptNumber(pub(crate) gic::InterruptNumber);

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

/// Structure representing a top-level/system-wide interrupt controller chip,
/// responsible for routing interrupts between peripherals and CPU cores.
///
/// On aarch64 w/ GIC, this corresponds to the Distributor.
pub struct SystemInterruptController;

/// Struct representing per-cpu-core interrupt controller chips.
///
/// On aarch64 w/ GIC, this corresponds to a Redistributor & CPU interface.
pub struct LocalInterruptController;

// 1st variant: get system controller
// 2nd variant: get local controller
macro_rules! get_int_ctlr {
    ($name:ident, $func:ident, $this:expr) => {
        let mut $name = INTERRUPT_CONTROLLER.lock();
        let $name = $name.as_mut().expect(concat!("BUG: ", stringify!($func), "(): INTERRUPT_CONTROLLER was uninitialized"));
    };
    ($name:ident, $func:ident) => ( get_int_ctlr!($name, $func, ()) );
}

impl SystemInterruptControllerApi for SystemInterruptController {
    fn id(&self) -> SystemInterruptControllerId {
        get_int_ctlr!(int_ctlr, id, self);
        SystemInterruptControllerId(int_ctlr.implementer().product_id)
    }

    fn version(&self) -> SystemInterruptControllerVersion {
        get_int_ctlr!(int_ctlr, version, self);
        SystemInterruptControllerVersion(int_ctlr.implementer().version)
    }

    fn get_destination(
        &self,
        interrupt_num: SystemInterruptNumber,
    ) -> Result<(Vec<InterruptDestination>, Priority), &'static str> {
        get_int_ctlr!(int_ctlr, get_destination, self);

        let priority = int_ctlr.get_interrupt_priority(interrupt_num.0);
        let vec = match int_ctlr.get_spi_target(interrupt_num.0)?.canonicalize() {
            SpiDestination::Specific(cpu) => [InterruptDestination {
                cpu,
            }].to_vec(),
            SpiDestination::AnyCpuAvailable => BOARD_CONFIG.cpu_ids.map(|mpidr| InterruptDestination {
                cpu: mpidr.into(),
            }).to_vec(),
            SpiDestination::GICv2TargetList(list) => {
                let mut vec = Vec::with_capacity(8);
                for result in list.iter() {
                    vec.push(InterruptDestination {
                        cpu: result?,
                    });
                }
                vec
            }
        };

        Ok((vec, priority))
    }

    fn set_destination(
        &self,
        sys_int_num: SystemInterruptNumber,
        destination: InterruptDestination,
        priority: Priority,
    ) -> Result<(), &'static str> {
        get_int_ctlr!(int_ctlr, set_destination, self);

        int_ctlr.set_spi_target(sys_int_num.0, SpiDestination::Specific(destination.cpu));
        int_ctlr.set_interrupt_priority(sys_int_num.0, priority);

        Ok(())
    }
}

impl LocalInterruptControllerApi for LocalInterruptController {
    fn init_secondary_cpu_interface(&self) {
        get_int_ctlr!(int_ctlr, init_secondary_cpu_interface);
        int_ctlr.init_secondary_cpu_interface();
    }

    fn id(&self) -> LocalInterruptControllerId {
        get_int_ctlr!(int_ctlr, id);
        LocalInterruptControllerId(int_ctlr.get_cpu_interface_id())
    }

    fn get_local_interrupt_priority(&self, num: LocalInterruptNumber) -> Priority {
        get_int_ctlr!(int_ctlr, get_local_interrupt_priority);
        int_ctlr.get_interrupt_priority(num.0)
    }

    fn set_local_interrupt_priority(&self, num: LocalInterruptNumber, priority: Priority) {
        get_int_ctlr!(int_ctlr, set_local_interrupt_priority);
        int_ctlr.set_interrupt_priority(num.0, priority);
    }

    fn is_local_interrupt_enabled(&self, num: LocalInterruptNumber) -> bool {
        get_int_ctlr!(int_ctlr, is_local_interrupt_enabled);
        int_ctlr.get_interrupt_state(num.0)
    }

    fn enable_local_interrupt(&self, num: LocalInterruptNumber, enabled: bool) {
        get_int_ctlr!(int_ctlr, enable_local_interrupt);
        int_ctlr.set_interrupt_state(num.0, enabled);
    }

    fn send_ipi(&self, num: LocalInterruptNumber, dest: Option<CpuId>) {
        get_int_ctlr!(int_ctlr, send_ipi);
        int_ctlr.send_ipi(num.0, match dest {
            Some(cpu) => IpiTargetCpu::Specific(cpu),
            None => IpiTargetCpu::AllOtherCpus,
        });
    }

    fn get_minimum_priority(&self) -> Priority {
        get_int_ctlr!(int_ctlr, get_minimum_priority);
        int_ctlr.get_minimum_priority()
    }

    fn set_minimum_priority(&self, priority: Priority) {
        get_int_ctlr!(int_ctlr, set_minimum_priority);
        int_ctlr.set_minimum_priority(priority)
    }

    fn acknowledge_interrupt(&self) -> (InterruptNumber, Priority) {
        get_int_ctlr!(int_ctlr, acknowledge_interrupt);

        let (num, prio) = int_ctlr.acknowledge_interrupt();

        (match num {
            0..=31 => LocalInterruptNumber(num).into(),
            _ => SystemInterruptNumber(num).into(),
        }, prio)
    }

    fn end_of_interrupt(&self, number: InterruptNumber) {
        get_int_ctlr!(int_ctlr, end_of_interrupt);
        let num_usize: usize = number.into();
        int_ctlr.end_of_interrupt(num_usize as _)
    }
}
