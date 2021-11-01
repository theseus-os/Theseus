use core::fmt;
use scheduler;
use zerocopy::FromBytes;

/// Registers stacked (pushed into the stack) during an exception
#[derive(Clone, Copy, FromBytes)]
#[repr(C)]
pub struct ExceptionFrame {
    /// (General purpose) Register 0
    pub r0: usize,

    /// (General purpose) Register 1
    pub r1: usize,

    /// (General purpose) Register 2
    pub r2: usize,

    /// (General purpose) Register 3
    pub r3: usize,

    /// (General purpose) Register 12
    pub r12: usize,

    /// Linker Register
    pub lr: usize,

    /// Program Counter
    pub pc: usize,

    /// Program Status Register
    pub xpsr: usize,
}

impl ExceptionFrame {
    pub fn new(pc: usize) -> ExceptionFrame {
        ExceptionFrame {
            r0: 0,
            r1: 0,
            r2: 0,
            r3: 0,
            r12: 0,
            lr: 0,
            pc,
            xpsr: 0x0100_0000
        }
    }
}

impl fmt::Debug for ExceptionFrame {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        struct Hex(usize);
        impl fmt::Debug for Hex {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "0x{:08x}", self.0)
            }
        }
        f.debug_struct("ExceptionFrame")
            .field("r0", &Hex(self.r0))
            .field("r1", &Hex(self.r1))
            .field("r2", &Hex(self.r2))
            .field("r3", &Hex(self.r3))
            .field("r12", &Hex(self.r12))
            .field("lr", &Hex(self.lr))
            .field("pc", &Hex(self.pc))
            .field("xpsr", &Hex(self.xpsr))
            .finish()
    }
}

#[export_name = "SysTick"]
pub unsafe extern "C" fn systick_handler() {
    scheduler::schedule();
}

#[export_name = "DefaultHandler_"]
pub unsafe extern "C" fn default_handler() {
    loop {}
}

#[export_name = "HardFault"]
pub unsafe extern "C" fn hardfault_handler(_ef: &ExceptionFrame) -> ! {
    loop {}
}
