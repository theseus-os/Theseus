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