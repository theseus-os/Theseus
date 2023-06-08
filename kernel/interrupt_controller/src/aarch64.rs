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