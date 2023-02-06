//! Structures needed for interacting with the IOMMU.

use zerocopy::FromBytes;
use volatile::{ReadOnly, WriteOnly};
use bitflags::bitflags;
use core::fmt;

/// The layout of the IOMMU's MMIO registers.
#[derive(FromBytes)]
#[repr(C)]
pub struct IntelIommuRegisters {
    /// Version register
    pub version:            ReadOnly<u32>,     // 0x00
    /// Reserved
    _reserved0:             [u8; 4],           // 0x04 - 0x07
    /// Capability register
    pub cap:                ReadOnly<u64>,     // 0x08
    /// Extended Capability register
    pub ecap:               ReadOnly<u64>,     // 0x10
    /// Global command register
    pub gcommand:           WriteOnly<u32>,    // 0x18
    /// Global status register
    pub gstatus:            ReadOnly<u32>,     // 0x1c
    /// Unimplemented (may be architecturally defined)
    _unimplemented:         [u8; 4096-0x20],   // 0x20-0xFFF
}
// TODO: Hardware may use more than 4kB, which means the registers may occupy
//       more than one contiguous page.
//       Currently we assume the IOMMU registers occupy only a single page.
const _: () = assert!(core::mem::size_of::<IntelIommuRegisters>() == 4096);

/// Helper struct for decoding and printing capability register
pub struct Capability(pub u64);

impl Capability {
    fn esrtps(&self)  -> bool { (self.0) & (1 << 63) != 0 }
    fn esirtps(&self) -> bool { (self.0) & (1 << 62) != 0 }
    fn fl5lp(&self)   -> bool { (self.0) & (1 << 60) != 0 }
    fn pi(&self)      -> bool { (self.0) & (1 << 59) != 0 }
    fn fl1gp(&self)   -> bool { (self.0) & (1 << 56) != 0 }
    fn drd(&self)     -> bool { (self.0) & (1 << 55) != 0 }
    fn dwd(&self)     -> bool { (self.0) & (1 << 54) != 0 }
    fn mamv(&self)    -> u64  { (self.0 >> 48) & 0x3f }
    fn nfr(&self)     -> u64  { ((self.0 >> 40) & 0xff) + 1 }
    fn psi(&self)     -> bool { (self.0) & (1 << 39) != 0 }
    fn sllps(&self)   -> u64  { (self.0 >> 34) & 0xf }
    fn fro(&self)     -> u64  { (self.0 >> 24) & 0x3ff }
    fn zlr(&self)     -> bool { (self.0) & (1 << 22) != 0 }
    fn mgaw(&self)    -> u64  { ((self.0 >> 16) & 0x3f) + 1 }
    fn sagaw(&self)   -> u64  { (self.0 >> 8) & 0x1f }
    fn cm(&self)      -> bool { (self.0) & (1 << 7) != 0 }
    fn phmr(&self)    -> bool { (self.0) & (1 << 6) != 0 }
    fn plmr(&self)    -> bool { (self.0) & (1 << 5) != 0 }
    fn rwbf(&self)    -> bool { (self.0) & (1 << 4) != 0 }
    fn afl(&self)     -> bool { (self.0) & (1 << 3) != 0 }
    fn nd(&self)      -> u64  { self.0 & 0x7 }
}

impl fmt::Debug for Capability {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Capability")
            .field("ND",       &self.nd())
            .field("AFL",      &self.afl())
            .field("RWBF",     &self.rwbf())
            .field("PLMR",     &self.plmr())
            .field("PHMR",     &self.phmr())
            .field("CM",       &self.cm())
            .field("SAGAW",    &self.sagaw())
            .field("MGAW",     &self.mgaw())
            .field("ZLR",      &self.zlr())
            .field("FRO",      &self.fro())
            .field("SLLPS",    &self.sllps())
            .field("PSI",      &self.psi())
            .field("NFR",      &self.nfr())
            .field("MAMV",     &self.mamv())
            .field("DWD",      &self.dwd())
            .field("DRD",      &self.drd())
            .field("FL1GP",    &self.fl1gp())
            .field("PI",       &self.pi())
            .field("FL5LP",    &self.fl5lp())
            .field("ESIRTPS",  &self.esirtps())
            .field("ESRTPS",   &self.esrtps())
            .finish()
    }
}

/// Helper struct for decoding and printing extended capability register
pub struct ExtendedCapability(pub u64);

impl ExtendedCapability {
    fn rprivs(&self)  -> bool { (self.0) & (1 << 53) != 0 }
    fn adms(&self)    -> bool { (self.0) & (1 << 52) != 0 }
    fn rps(&self)     -> bool { (self.0) & (1 << 49) != 0 }
    fn smpwcs(&self)  -> bool { (self.0) & (1 << 48) != 0 }
    fn flts(&self)    -> bool { (self.0) & (1 << 47) != 0 }
    fn slts(&self)    -> bool { (self.0) & (1 << 46) != 0 }
    fn slads(&self)   -> bool { (self.0) & (1 << 45) != 0 }
    fn vcs(&self)     -> bool { (self.0) & (1 << 44) != 0 }
    fn smts(&self)    -> bool { (self.0) & (1 << 43) != 0 }
    fn pds(&self)     -> bool { (self.0) & (1 << 42) != 0 }
    fn dit(&self)     -> bool { (self.0) & (1 << 41) != 0 }
    fn pasid(&self)   -> bool { (self.0) & (1 << 40) != 0 }
    fn pss(&self)     -> u64  { ((self.0 >> 35) & 0x1f) + 1 }
    fn eafs(&self)    -> bool { (self.0) & (1 << 34) != 0 }
    fn nwfs(&self)    -> bool { (self.0) & (1 << 33) != 0 }
    fn srs(&self)     -> bool { (self.0) & (1 << 31) != 0 }
    fn ers(&self)     -> bool { (self.0) & (1 << 30) != 0 }
    fn prs(&self)     -> bool { (self.0) & (1 << 29) != 0 }
    fn nest(&self)    -> bool { (self.0) & (1 << 26) != 0 }
    fn mts(&self)     -> bool { (self.0) & (1 << 25) != 0 }
    fn mhmv(&self)    -> u64  { (self.0 >> 20) & 0xf }
    fn iro(&self)     -> u64  { (self.0 >> 8) & 0x3ff }
    fn sc(&self)      -> bool { (self.0) & (1 << 7) != 0 }
    fn pt(&self)      -> bool { (self.0) & (1 << 6) != 0 }
    fn eim(&self)     -> bool { (self.0) & (1 << 4) != 0 }
    fn ir(&self)      -> bool { (self.0) & (1 << 3) != 0 }
    fn dt(&self)      -> bool { (self.0) & (1 << 2) != 0 }
    fn qi(&self)      -> bool { (self.0) & (1 << 1) != 0 }
    fn c(&self)       -> bool { (self.0) & (1 << 0) != 0 }
}

impl fmt::Debug for ExtendedCapability {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ExtendedCapability")
            .field("C",       &self.c())
            .field("QI",      &self.qi())
            .field("DT",      &self.dt())
            .field("IR",      &self.ir())
            .field("EIM",     &self.eim())
            .field("PT",      &self.pt())
            .field("SC",      &self.sc())
            .field("IRO",     &self.iro())
            .field("MHMV",    &self.mhmv())
            .field("MTS",     &self.mts())
            .field("NEST",    &self.nest())
            .field("PRS",     &self.prs())
            .field("ERS",     &self.ers())
            .field("SRS",     &self.srs())
            .field("NWFS",    &self.nwfs())
            .field("EAFS",    &self.eafs())
            .field("PSS",     &self.pss())
            .field("PASID",   &self.pasid())
            .field("DIT",     &self.dit())
            .field("PDS",     &self.pds())
            .field("SMTS",    &self.smts())
            .field("VCS",     &self.vcs())
            .field("SLADS",   &self.slads())
            .field("SLTS",    &self.slts())
            .field("FLTS",    &self.flts())
            .field("SMPWCS",  &self.smpwcs())
            .field("RPS",     &self.rps())
            .field("ADMS",    &self.adms())
            .field("RPRIVS",  &self.rprivs())
            .finish()
    }
}

/// Bits corresponding to commands in the Global Command register.
#[repr(u32)]
pub enum GlobalCommand {
    /// Compatibility Format Interrupt
    Cfi   = 1 << 23,
    /// Set Interrupt Remap Table Pointer
    Sirtp = 1 << 24,
    /// Interrupt Remapping Enable
    Ire   = 1 << 25,
    /// Queued Invalidation Enable
    Qie   = 1 << 26,
    /// Write Buffer Flush
    Wbf   = 1 << 27,
    /// Enable Advanced Fault Logging
    Eafl  = 1 << 28,
    /// Set Fault Log
    Sfl   = 1 << 29,
    /// Set Root Table Pointer
    Srtp  = 1 << 30,
    /// Translation Enable
    TE    = 1 << 31,
}

bitflags! {
    /// Global status register flags.
    ///
    /// The least significant bits `[22:0]` are `RsvdZ`,
    /// meaning that they are reserved for future usage and must be set to 0.
    pub struct GlobalStatus: u32 {
        /// Compatibility Format Interrupt Status
        const CFIS  = 1 << 23;
        /// Interrupt Remapping Table Pointer Status
        const IRTPS = 1 << 24;
        /// Interrupt Remapping Enable Status
        const IRES  = 1 << 25;
        /// Queued Invalidation Enable Status
        const QIES  = 1 << 26;
        /// Write Buffer Flush Status
        const WBFS  = 1 << 27;
        /// Advanced Fault Logging Status
        const AFLS  = 1 << 28;
        /// Fault Log Status
        const FLS   = 1 << 29;
        /// Root Table Pointer Status
        const RTPS  = 1 << 30;
        /// Translation Enable Status
        const TES   = 1 << 31;
    }
}
