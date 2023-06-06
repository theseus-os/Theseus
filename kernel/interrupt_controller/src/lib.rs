

// The private global Generic Interrupt Controller singleton
static INTERRUPT_CONTROLLER: MutexIrqSafe<Option<ArmGic>> = MutexIrqSafe::new(None);

pub fn init() -> Result<(), &'static str> {
    // *INTERRUPT_CONTROLLER = Some(...);
}

pub struct LocalInterruptControllerId(/* arch-specific unsigned int */);
pub struct GlobalInterruptNumber(/* arch-specific unsigned int */);
pub struct LocalInterruptNumber(/* arch-specific unsigned int */);

/// Singleton representing the main/system-wide interrupt controller chip,
/// responsible for routing interrupts between peripherals and CPU cores.
///
/// On x86_64, this corresponds to the IoApic.
/// On aarch64 w/ GIC, this corresponds to the Distributor.
pub struct GlobalInterruptController {}

/// Struct representing per-cpu-core interrupt controller chips.
///
/// On x86_64, this corresponds to the LocalApic.
/// On aarch64 w/ GIC, this corresponds to the Redistributor.
pub struct LocalInterruptController {}

/// Allows code to control the currently pending or active interrupt.
///
/// On x86_64, this uses the LocalApic.
/// On aarch64 w/ GIC, this uses the CPU Interface.
pub struct CurrentInterrupt {}

// Should I implement Index & IndexMut instead?
impl GlobalInterruptController {
    pub fn get_destination(&self, interrupt_num: GlobalInterruptNumber) -> Option<(CpuId, LocalInterruptNumber, Priority)> {
        todo!()
    }

    pub fn set_destination(&mut self, interrupt_num: GlobalInterruptNumber, destination: Option<(CpuId, LocalInterruptNumber, Priority)>) {
        todo!()
    }
}

impl LocalInterruptController {
    pub fn cpu_id(&self) -> CpuId

    pub fn get_min_priority(&self) -> Priority {
        todo!()
    }

    pub fn set_min_priority(&mut self, priority: Priority) {
        todo!()
    }


}
