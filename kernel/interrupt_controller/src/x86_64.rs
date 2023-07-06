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

    /// Initializes the interrupt controller, on aarch64
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

// 1st variant: get system controller
// 2nd variant: get local controller
macro_rules! get_int_ctlr {
    ($name:ident, $func:ident, $this:expr) => {
        let mut $name = get_ioapic($this.id).expect(concat!("BUG: ", stringify!($func), "(): get_ioapic() returned None"));
    };
    ($name:ident, $func:ident) => {
        let mut $name = get_my_apic().expect(concat!("BUG: ", stringify!($func), "(): get_my_apic() returned None"));
        let mut $name = $name.write();
    };
}

impl SystemInterruptControllerApi for SystemInterruptController {
    fn id(&self) -> SystemInterruptControllerId {
        get_int_ctlr!(int_ctlr, id, self);
        SystemInterruptControllerId(int_ctlr.id())
    }

    fn version(&self) -> SystemInterruptControllerVersion {
        get_int_ctlr!(int_ctlr, version, self);
        SystemInterruptControllerVersion(int_ctlr.version())
    }

    fn get_destination(
        &self,
        interrupt_num: InterruptNumber,
    ) -> Result<(Vec<InterruptDestination>, Priority), &'static str> {
        // no way to read the destination for an IRQ number in IoApic
        unimplemented!()
    }

    fn set_destination(
        &self,
        sys_int_num: InterruptNumber,
        destination: InterruptDestination,
        priority: Priority,
    ) -> Result<(), &'static str> {
        get_int_ctlr!(int_ctlr, set_destination, self);

        // no support for priority on x86_64
        let _ = priority;

        int_ctlr.set_irq(sys_int_num, destination.cpu.into(), destination.local_number)
    }
}


impl LocalInterruptControllerApi for LocalInterruptController {
    fn init_secondary_cpu_interface(&self) {
        panic!("This must not be used on x86_64")
    }

    fn id(&self) -> LocalInterruptControllerId {
        get_int_ctlr!(int_ctlr, id);

        LocalInterruptControllerId(int_ctlr.processor_id())
    }

    fn get_local_interrupt_priority(&self, num: InterruptNumber) -> Priority {
        get_int_ctlr!(int_ctlr, get_local_interrupt_priority);

        // No priority support on x86_64
        Priority
    }

    fn set_local_interrupt_priority(&self, num: InterruptNumber, priority: Priority) {
        get_int_ctlr!(int_ctlr, set_local_interrupt_priority);

        // No priority support on x86_64
        let _ = priority;
    }

    fn is_local_interrupt_enabled(&self, num: InterruptNumber) -> bool {
        todo!()
    }

    fn enable_local_interrupt(&self, num: InterruptNumber, enabled: bool) {
        todo!()
    }

    fn send_ipi(&self, num: InterruptNumber, dest: Option<CpuId>) {
        get_int_ctlr!(int_ctlr, send_ipi);
        int_ctlr.send_ipi(num, match dest {
            Some(cpu) => LapicIpiDestination::One(cpu.into()),
            None => LapicIpiDestination::AllButMe,
        });
    }

    fn get_minimum_priority(&self) -> Priority {
        get_int_ctlr!(int_ctlr, get_minimum_priority);

        // No priority support on x86_64
        Priority
    }

    fn set_minimum_priority(&self, priority: Priority) {
        get_int_ctlr!(int_ctlr, set_minimum_priority);

        // No priority support on x86_64
        let _ = priority;
    }

    fn acknowledge_interrupt(&self) -> (InterruptNumber, Priority) {
        panic!("This must not be used on x86_64")
    }

    fn end_of_interrupt(&self, _number: InterruptNumber) {
        get_int_ctlr!(int_ctlr, end_of_interrupt);

        // On x86, passing the number isn't required.
        int_ctlr.eoi();
    }
}
