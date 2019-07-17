//! Processor state stored in the FLAGS, EFLAGS, or RFLAGS register.

bitflags! {
    /// The RFLAGS register. All variants are backwards compatable so only one
    /// bitflags struct needed.
    pub flags Flags: usize {
        /// ID Flag (ID)
        const ID = 1 << 21,
        /// Virtual Interrupt Pending (VIP)
        const VIP = 1 << 20,
        /// Virtual Interrupt Flag (VIF)
        const VIF = 1 << 19,
        /// Alignment Check (AC)
        const AC = 1 << 18,
        /// Virtual-8086 Mode (VM)
        const VM = 1 << 17,
        /// Resume Flag (RF)
        const RF = 1 << 16,
        /// Nested Task (NT)
        const NT = 1 << 14,
        /// I/O Privilege Level (IOPL) 0
        const IOPL0 = 0 << 12,
        /// I/O Privilege Level (IOPL) 1
        const IOPL1 = 1 << 12,
        /// I/O Privilege Level (IOPL) 2
        const IOPL2 = 2 << 12,
        /// I/O Privilege Level (IOPL) 3
        const IOPL3 = 3 << 12,
        /// Overflow Flag (OF)
        const OF = 1 << 11,
        /// Direction Flag (DF)
        const DF = 1 << 10,
        /// Interrupt Enable Flag (IF)
        const IF = 1 << 9,
        /// Trap Flag (TF)
        const TF = 1 << 8,
        /// Sign Flag (SF)
        const SF = 1 << 7,
        /// Zero Flag (ZF)
        const ZF = 1 << 6,
        /// Auxiliary Carry Flag (AF)
        const AF = 1 << 4,
        /// Parity Flag (PF)
        const PF = 1 << 2,
        /// Bit 1 is always 1.
        const A1 = 1 << 1,
        /// Carry Flag (CF)
        const CF = 1 << 0,
    }
}

/// Returns the current value of the RFLAGS register.
pub fn flags() -> Flags {
    Flags::from_bits_truncate(0)
}

/// Writes the RFLAGS register.
pub fn set_flags(val: Flags) {
}
