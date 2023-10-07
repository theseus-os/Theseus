use super::*;

use apic::{LocalApic, LapicIpiDestination};
use ioapic::IoApic;
use madt::Madt;
use spin::Mutex;
use sync_irq::IrqSafeRwLock;

#[derive(Debug, Copy, Clone)]
pub struct SystemInterruptControllerVersion(pub u32);
#[derive(Debug, Copy, Clone)]
pub struct SystemInterruptControllerId(pub u32);
#[derive(Debug, Copy, Clone)]
pub struct LocalInterruptControllerId(pub u32);
#[derive(Debug, Copy, Clone)]
pub struct Priority;

/// Initializes the interrupt controller(s), including the Local APIC for the BSP
/// (bootstrap processor) and the system-wide IOAPIC(s).
pub fn init(kernel_mmi: &memory::MmiRef) -> Result<(), &'static str> {
    apic::init();

    // Use the MADT ACPI table to initialize more interrupt controller details.
    {
        let acpi_tables = acpi::get_acpi_tables().lock();
        let madt = Madt::get(&acpi_tables)
            .ok_or("The required MADT ACPI table wasn't found (signature 'APIC')")?;
        madt.bsp_init(&mut kernel_mmi.lock().page_table)?;
    }

    Ok(())
}

/// Structure representing a top-level/system-wide interrupt controller chip,
/// responsible for routing interrupts between peripherals and CPU cores.
///
/// On x86_64, this corresponds to an IoApic.
pub struct SystemInterruptController(&'static Mutex<IoApic>);

// TODO: implement `SystemInterruptController::get()` for IOAPIC,
//       but it needs to be able to handle multiple IOAPICs.

impl SystemInterruptControllerApi for SystemInterruptController {
    fn id(&self) -> SystemInterruptControllerId {
        SystemInterruptControllerId(self.0.lock().id())
    }

    fn version(&self) -> SystemInterruptControllerVersion {
        SystemInterruptControllerVersion(self.0.lock().version())
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
        destination: Option<CpuId>,
        priority: Priority,
    ) -> Result<(), &'static str> {
        // no support for priority on x86_64
        let _ = priority;

        if let Some(destination) = destination {
            self.0.lock().set_irq(sys_int_num, destination.into(), sys_int_num)
        } else {
            todo!("SystemInterruptController::set_destination: todo on x86: set the IOREDTBL MASK bit")
        }
    }
}


/// Struct representing per-cpu-core interrupt controller chips.
///
/// On x86_64, this corresponds to a LocalApic.
pub struct LocalInterruptController(&'static IrqSafeRwLock<LocalApic>);
impl LocalInterruptController {
    /// Returns a reference to the current CPU's local interrupt controller,
    /// if it has been initialized.
    pub fn get() -> Option<Self> {
        apic::get_my_apic().map(Self)
    }
}

impl LocalInterruptControllerApi for LocalInterruptController {
    fn id(&self) -> LocalInterruptControllerId {
        LocalInterruptControllerId(self.0.read().apic_id().value())
    }

    fn enable_local_timer_interrupt(&self, enable: bool) {
        self.0.write().enable_lvt_timer(enable)
    }

    fn send_ipi(&self, num: InterruptNumber, dest: InterruptDestination) {
        use InterruptDestination::*;

        self.0.write().send_ipi(num, match dest {
            SpecificCpu(cpu) => LapicIpiDestination::One(cpu.into()),
            AllOtherCpus => LapicIpiDestination::AllButMe,
        });
    }

    fn end_of_interrupt(&self, _number: InterruptNumber) {
        // When using APIC, we don't need to pass in an IRQ number.
        self.0.write().eoi();
    }
}
