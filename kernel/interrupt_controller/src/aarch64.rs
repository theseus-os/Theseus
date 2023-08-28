use {
    gic::{ArmGicDistributor, ArmGicCpuComponents, SpiDestination, IpiTargetCpu, Version as GicVersion},
    arm_boards::{NUM_CPUS, BOARD_CONFIG, InterruptControllerConfig},
    core::array::try_from_fn,
    sync_irq::IrqSafeMutex,
    cpu::current_cpu,
    spin::Once,
};

use super::*;

pub use gic::Priority;

#[derive(Debug, Copy, Clone)]
pub struct SystemInterruptControllerVersion(pub u8);
#[derive(Debug, Copy, Clone)]
pub struct SystemInterruptControllerId(pub u8);
#[derive(Debug, Copy, Clone)]
pub struct LocalInterruptControllerId(pub u16);

/// Per-CPU local interrupt controller
///
/// To get the controller for a specific CPU:
///    a. Find the position of its CpuId in `BOARD_CONFIG.cpu_ids`
///    b. Index into this array using that position
static LOCAL_INT_CTRL: Once<[LocalInterruptController; NUM_CPUS]> = Once::new();

/// System-wide interrupt controller
static SYSTEM_WIDE_INT_CTRL: Once<SystemInterruptController> = Once::new();

/// Initializes the interrupt controller, on aarch64
pub fn init() -> Result<(), &'static str> {
    match BOARD_CONFIG.interrupt_controller {
        InterruptControllerConfig::GicV3(gicv3_cfg) => {
            let version = GicVersion::InitV3 {
                dist: gicv3_cfg.distributor_base_address,
                redist: gicv3_cfg.redistributor_base_addresses,
            };

            SYSTEM_WIDE_INT_CTRL.try_call_once(|| -> Result<_, &'static str> {
                let distrib = ArmGicDistributor::init(&version)?;
                let mutex = IrqSafeMutex::new(distrib);
                Ok(SystemInterruptController(mutex))
            })?;

            LOCAL_INT_CTRL.try_call_once(|| -> Result<_, &'static str> {
                let cpu_ctlrs: [ArmGicCpuComponents; NUM_CPUS] = try_from_fn(|i| {
                    let cpu_id = BOARD_CONFIG.cpu_ids[i].into();
                    ArmGicCpuComponents::init(cpu_id, &version)
                })?;

                Ok(cpu_ctlrs.map(|ctlr| {
                    let mutex = IrqSafeMutex::new(ctlr);
                    LocalInterruptController(mutex)
                }))
            })?;
        },
    }

    Ok(())
}

/// Structure representing a top-level/system-wide interrupt controller chip,
/// responsible for routing interrupts between peripherals and CPU cores.
///
/// On aarch64 w/ GIC, this corresponds to the Distributor.
pub struct SystemInterruptController(IrqSafeMutex<ArmGicDistributor>);

/// Struct representing per-cpu-core interrupt controller chips.
///
/// On aarch64 w/ GIC, this corresponds to a Redistributor & CPU interface.
pub struct LocalInterruptController(IrqSafeMutex<ArmGicCpuComponents>);

impl SystemInterruptControllerApi for SystemInterruptController {
    fn get() -> &'static Self {
        SYSTEM_WIDE_INT_CTRL.get().expect("interrupt_controller wasn't initialized")
    }

    fn id(&self) -> SystemInterruptControllerId {
        let dist = self.0.lock();
        SystemInterruptControllerId(dist.implementer().product_id)
    }

    fn version(&self) -> SystemInterruptControllerVersion {
        let dist = self.0.lock();
        SystemInterruptControllerVersion(dist.implementer().version)
    }

    fn get_destination(
        &self,
        interrupt_num: InterruptNumber,
    ) -> Result<(Vec<CpuId>, Priority), &'static str> {
        assert!(interrupt_num >= 32, "shared peripheral interrupts have a number >= 32");
        let dist = self.0.lock();

        let priority = dist.get_spi_priority(interrupt_num as _);
        let vec = match dist.get_spi_target(interrupt_num as _)?.canonicalize() {
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
        let mut dist = self.0.lock();

        dist.set_spi_target(sys_int_num as _, SpiDestination::Specific(destination));
        dist.set_spi_priority(sys_int_num as _, priority);

        Ok(())
    }
}

impl LocalInterruptControllerApi for LocalInterruptController {
    fn get() -> &'static Self {
        // how this function works:
        //   a. get the current CpuId: this CpuId of the current CPU
        //   b. iterate on all valid CpuIds, find the index of the current CpuId.
        //      This is used as a current CPU index.
        //   c. get the global array of interrupt controllers
        //   d. index into this array based on the current CPU index

        let cpu_id = current_cpu();
        // While we're waiting for cpu-local-storage, this loop will work as fine as an AtomicMap
        let index = BOARD_CONFIG.cpu_ids.iter().position(|mpidr| cpu_id == (*mpidr).into());
        let index = index.expect("Invalid CpuId returned by current_cpu()");

        let ctrls = LOCAL_INT_CTRL.get();
        let ctrls = ctrls.expect("interrupt_controller wasn't initialized");

        &ctrls[index]
    }

    fn init_secondary_cpu_interface(&self) {
        let mut cpu_ctrl = self.0.lock();
        cpu_ctrl.init_secondary_cpu_interface();
    }

    fn id(&self) -> LocalInterruptControllerId {
        let cpu_ctrl = self.0.lock();
        LocalInterruptControllerId(cpu_ctrl.get_cpu_interface_id())
    }

    fn get_local_interrupt_priority(&self, num: InterruptNumber) -> Priority {
        assert!(num < 32, "local interrupts have a number < 32");
        let cpu_ctrl = self.0.lock();
        cpu_ctrl.get_interrupt_priority(num as _)
    }

    fn set_local_interrupt_priority(&self, num: InterruptNumber, priority: Priority) {
        assert!(num < 32, "local interrupts have a number < 32");
        let mut cpu_ctrl = self.0.lock();
        cpu_ctrl.set_interrupt_priority(num as _, priority);
    }

    fn is_local_interrupt_enabled(&self, num: InterruptNumber) -> bool {
        assert!(num < 32, "local interrupts have a number < 32");
        let cpu_ctrl = self.0.lock();
        cpu_ctrl.get_interrupt_state(num as _)
    }

    fn enable_local_interrupt(&self, num: InterruptNumber, enabled: bool) {
        assert!(num < 32, "local interrupts have a number < 32");
        let mut cpu_ctrl = self.0.lock();
        cpu_ctrl.set_interrupt_state(num as _, enabled);
    }

    fn send_ipi(&self, num: InterruptNumber, dest: InterruptDestination) {
        use InterruptDestination::*;
        assert!(num < 16, "IPIs have a number < 16");
        let mut cpu_ctrl = self.0.lock();

        cpu_ctrl.send_ipi(num as _, match dest {
            SpecificCpu(cpu) => IpiTargetCpu::Specific(cpu),
            AllOtherCpus => IpiTargetCpu::AllOtherCpus,
        });
    }

    fn get_minimum_priority(&self) -> Priority {
        let cpu_ctrl = self.0.lock();
        cpu_ctrl.get_minimum_priority()
    }

    fn set_minimum_priority(&self, priority: Priority) {
        let mut cpu_ctrl = self.0.lock();
        cpu_ctrl.set_minimum_priority(priority)
    }

    fn acknowledge_interrupt(&self) -> (InterruptNumber, Priority) {
        let mut cpu_ctrl = self.0.lock();
        let (num, prio) = cpu_ctrl.acknowledge_interrupt();
        (num as _, prio)
    }

    fn end_of_interrupt(&self, number: InterruptNumber) {
        let mut cpu_ctrl = self.0.lock();
        cpu_ctrl.end_of_interrupt(number as _)
    }
}
