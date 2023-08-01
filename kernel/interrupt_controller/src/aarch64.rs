use {
    gic::{ArmGic, SpiDestination, IpiTargetCpu, Version as GicVersion},
    arm_boards::{BOARD_CONFIG, InterruptControllerConfig},
    sync_irq::IrqSafeMutex,
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

/// The private global Generic Interrupt Controller singleton
pub(crate) static INTERRUPT_CONTROLLER: IrqSafeMutex<Option<ArmGic>> = IrqSafeMutex::new(None);

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

impl SystemInterruptControllerApi for SystemInterruptController {
    fn id(&self) -> SystemInterruptControllerId {
        let mut int_ctlr = INTERRUPT_CONTROLLER.lock();
        let int_ctlr = int_ctlr
            .as_ref()
            .expect("BUG: id(): INTERRUPT_CONTROLLER was uninitialized");

        SystemInterruptControllerId(int_ctlr.implementer().product_id)
    }

    fn version(&self) -> SystemInterruptControllerVersion {
        let mut int_ctlr = INTERRUPT_CONTROLLER.lock();
        let int_ctlr = int_ctlr
            .as_ref()
            .expect("BUG: version(): INTERRUPT_CONTROLLER was uninitialized");

        SystemInterruptControllerVersion(int_ctlr.implementer().version)
    }

    fn get_destination(
        &self,
        interrupt_num: InterruptNumber,
    ) -> Result<(Vec<CpuId>, Priority), &'static str> {
        assert!(interrupt_num >= 32, "shared peripheral interrupts have a number >= 32");
        let mut int_ctlr = INTERRUPT_CONTROLLER.lock();
        let int_ctlr = int_ctlr
            .as_ref()
            .expect("BUG: get_destination(): INTERRUPT_CONTROLLER was uninitialized");

        let priority = int_ctlr.get_interrupt_priority(interrupt_num as _);
        let vec = match int_ctlr.get_spi_target(interrupt_num as _)?.canonicalize() {
            SpiDestination::Specific(cpu) => [cpu].to_vec(),
            SpiDestination::AnyCpuAvailable => BOARD_CONFIG.cpu_ids.map(|mpidr| mpidr.into()).to_vec(),
            SpiDestination::GICv2TargetList(list) => {
                let mut vec = Vec::with_capacity(8);
                for result in list.iter() {
                    vec.push(result?);
                }
                vec
            }
        };

        Ok((vec, priority))
    }

    fn set_destination(
        &self,
        sys_int_num: InterruptNumber,
        destination: CpuId,
        priority: Priority,
    ) -> Result<(), &'static str> {
        assert!(sys_int_num >= 32, "shared peripheral interrupts have a number >= 32");
        let mut int_ctlr = INTERRUPT_CONTROLLER.lock();
        let int_ctlr = int_ctlr
            .as_mut()
            .expect("BUG: set_destination(): INTERRUPT_CONTROLLER was uninitialized");

        int_ctlr.set_spi_target(sys_int_num as _, SpiDestination::Specific(destination));
        int_ctlr.set_interrupt_priority(sys_int_num as _, priority);

        Ok(())
    }
}

impl LocalInterruptControllerApi for LocalInterruptController {
    fn init_secondary_cpu_interface(&self) {
        let mut int_ctlr = INTERRUPT_CONTROLLER.lock();
        let int_ctlr = int_ctlr
            .as_mut()
            .expect("BUG: init_secondary_cpu_interface(): INTERRUPT_CONTROLLER was uninitialized");

        int_ctlr.init_secondary_cpu_interface();
    }

    fn id(&self) -> LocalInterruptControllerId {
        let mut int_ctlr = INTERRUPT_CONTROLLER.lock();
        let int_ctlr = int_ctlr
            .as_ref()
            .expect("BUG: id(): INTERRUPT_CONTROLLER was uninitialized");

        LocalInterruptControllerId(int_ctlr.get_cpu_interface_id())
    }

    fn get_local_interrupt_priority(&self, num: InterruptNumber) -> Priority {
        assert!(num < 32, "local interrupts have a number < 32");
        let mut int_ctlr = INTERRUPT_CONTROLLER.lock();
        let int_ctlr = int_ctlr
            .as_ref()
            .expect("BUG: get_local_interrupt_priority(): INTERRUPT_CONTROLLER was uninitialized");

        int_ctlr.get_interrupt_priority(num as _)
    }

    fn set_local_interrupt_priority(&self, num: InterruptNumber, priority: Priority) {
        assert!(num < 32, "local interrupts have a number < 32");
        let mut int_ctlr = INTERRUPT_CONTROLLER.lock();
        let int_ctlr = int_ctlr
            .as_mut()
            .expect("BUG: set_local_interrupt_priority(): INTERRUPT_CONTROLLER was uninitialized");

        int_ctlr.set_interrupt_priority(num as _, priority);
    }

    fn is_local_interrupt_enabled(&self, num: InterruptNumber) -> bool {
        assert!(num < 32, "local interrupts have a number < 32");
        let mut int_ctlr = INTERRUPT_CONTROLLER.lock();
        let int_ctlr = int_ctlr
            .as_ref()
            .expect("BUG: is_local_interrupt_enabled(): INTERRUPT_CONTROLLER was uninitialized");

        int_ctlr.get_interrupt_state(num as _)
    }

    fn enable_local_interrupt(&self, num: InterruptNumber, enabled: bool) {
        assert!(num < 32, "local interrupts have a number < 32");
        let mut int_ctlr = INTERRUPT_CONTROLLER.lock();
        let int_ctlr = int_ctlr
            .as_mut()
            .expect("BUG: enable_local_interrupt(): INTERRUPT_CONTROLLER was uninitialized");

        int_ctlr.set_interrupt_state(num as _, enabled);
    }

    fn send_ipi(&self, num: InterruptNumber, dest: InterruptDestination) {
        use InterruptDestination::*;
        assert!(num < 16, "IPIs have a number < 16");

        let mut int_ctlr = INTERRUPT_CONTROLLER.lock();
        let int_ctlr = int_ctlr
            .as_mut()
            .expect("BUG: send_ipi(): INTERRUPT_CONTROLLER was uninitialized");

        int_ctlr.send_ipi(num as _, match dest {
            SpecificCpu(cpu) => IpiTargetCpu::Specific(cpu),
            AllOtherCpus => IpiTargetCpu::AllOtherCpus,
        });
    }

    fn get_minimum_priority(&self) -> Priority {
        let mut int_ctlr = INTERRUPT_CONTROLLER.lock();
        let int_ctlr = int_ctlr
            .as_ref()
            .expect("BUG: get_minimum_priority(): INTERRUPT_CONTROLLER was uninitialized");

        int_ctlr.get_minimum_priority()
    }

    fn set_minimum_priority(&self, priority: Priority) {
        let mut int_ctlr = INTERRUPT_CONTROLLER.lock();
        let int_ctlr = int_ctlr
            .as_mut()
            .expect("BUG: set_minimum_priority(): INTERRUPT_CONTROLLER was uninitialized");

        int_ctlr.set_minimum_priority(priority)
    }

    fn acknowledge_interrupt(&self) -> (InterruptNumber, Priority) {
        let mut int_ctlr = INTERRUPT_CONTROLLER.lock();
        let int_ctlr = int_ctlr
            .as_mut()
            .expect("BUG: acknowledge_interrupt(): INTERRUPT_CONTROLLER was uninitialized");

        let (num, prio) = int_ctlr.acknowledge_interrupt();
        (num as _, prio)
    }

    fn end_of_interrupt(&self, number: InterruptNumber) {
        let mut int_ctlr = INTERRUPT_CONTROLLER.lock();
        let int_ctlr = int_ctlr
            .as_mut()
            .expect("BUG: end_of_interrupt(): INTERRUPT_CONTROLLER was uninitialized");

        int_ctlr.end_of_interrupt(number as _)
    }
}
