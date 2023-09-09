use super::*;

use {
    apic::{get_my_apic, LapicIpiDestination},
    ioapic::get_ioapic,
};

#[derive(Debug, Copy, Clone)]
pub struct SystemInterruptControllerVersion(pub u32);
#[derive(Debug, Copy, Clone)]
pub struct SystemInterruptControllerId(pub u32);
#[derive(Debug, Copy, Clone)]
pub struct LocalInterruptControllerId(pub u32);
#[derive(Debug, Copy, Clone)]
pub struct Priority;

/// Initializes the interrupt controller (not yet used on x86)
pub fn init() -> Result<(), &'static str> { Ok(()) }

/// Structure representing a top-level/system-wide interrupt controller chip,
/// responsible for routing interrupts between peripherals and CPU cores.
///
/// On x86_64, this corresponds to an IoApic.
pub struct SystemInterruptController {
    id: u8,
}

/// Struct representing per-cpu-core interrupt controller chips.
///
/// On x86_64, this corresponds to a LocalApic.
pub struct LocalInterruptController;

impl SystemInterruptControllerApi for SystemInterruptController {
    fn get() -> &'static Self {
        unimplemented!()
    }

    fn id(&self) -> SystemInterruptControllerId {
        let mut int_ctlr = get_ioapic(self.id).expect("BUG: id(): get_ioapic() returned None");
        SystemInterruptControllerId(int_ctlr.id())
    }

    fn version(&self) -> SystemInterruptControllerVersion {
        let mut int_ctlr = get_ioapic(self.id).expect("BUG: version(): get_ioapic() returned None");
        SystemInterruptControllerVersion(int_ctlr.version())
    }

    fn get_destination(
        &self,
        interrupt_num: InterruptNumber,
    ) -> Result<(Vec<CpuId>, Priority), &'static str> {
        todo!("Getting interrupt destination from IOAPIC redirection tables is not yet implemented")
    }

    fn set_destination(
        &self,
        sys_int_num: InterruptNumber,
        destination: CpuId,
        priority: Priority,
    ) -> Result<(), &'static str> {
        let mut int_ctlr = get_ioapic(self.id).expect("BUG: set_destination(): get_ioapic() returned None");

        // no support for priority on x86_64
        let _ = priority;

        int_ctlr.set_irq(sys_int_num, destination.into(), sys_int_num /* <- is this correct? */)
    }
}


impl LocalInterruptControllerApi for LocalInterruptController {
    fn get() -> &'static Self {
        unimplemented!()
    }

    fn init_secondary_cpu_interface(&self) {
        panic!("This must not be used on x86_64")
    }

    fn id(&self) -> LocalInterruptControllerId {
        let int_ctlr = get_my_apic().expect("BUG: id(): get_my_apic() returned None");
        let int_ctlr = int_ctlr.read();
        LocalInterruptControllerId(int_ctlr.processor_id())
    }

    fn get_local_interrupt_priority(&self, num: InterruptNumber) -> Priority {
        // No priority support on x86_64
        Priority
    }

    fn set_local_interrupt_priority(&self, num: InterruptNumber, priority: Priority) {
        // No priority support on x86_64
        let _ = priority;
    }

    fn is_local_interrupt_enabled(&self, num: InterruptNumber) -> bool {
        todo!()
    }

    fn enable_local_interrupt(&self, num: InterruptNumber, enabled: bool) {
        todo!()
    }

    fn send_ipi(&self, num: InterruptNumber, dest: InterruptDestination) {
        use InterruptDestination::*;

        let mut int_ctlr = get_my_apic().expect("BUG: send_ipi(): get_my_apic() returned None");
        let mut int_ctlr = int_ctlr.write();
        int_ctlr.send_ipi(num, match dest {
            SpecificCpu(cpu) => LapicIpiDestination::One(cpu.into()),
            AllOtherCpus => LapicIpiDestination::AllButMe,
        });
    }

    fn get_minimum_priority(&self) -> Priority {
        // No priority support on x86_64
        Priority
    }

    fn set_minimum_priority(&self, priority: Priority) {
        // No priority support on x86_64
        let _ = priority;
    }

    fn acknowledge_interrupt(&self) -> (InterruptNumber, Priority) {
        panic!("This must not be used on x86_64")
    }

    fn end_of_interrupt(&self, _number: InterruptNumber) {
        let mut int_ctlr = get_my_apic().expect("BUG: end_of_interrupt(): get_my_apic() returned None");
        let mut int_ctlr = int_ctlr.write();

        // On x86, passing the number isn't required.
        int_ctlr.eoi();
    }
}
