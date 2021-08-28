//! This file contains structs that simplify programming the IOMMU.

use zerocopy::FromBytes;
use volatile::{ReadOnly, WriteOnly};
use bitflags::bitflags;

/// Struct which allows direct access to memory mapped registers when
/// overlayed over corresponding page.
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
// more than one contiguous page.
const_assert_eq!(core::mem::size_of::<IntelIommuRegisters>(), 4096);

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

impl core::fmt::Display for Capability {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        // TODO: Do we really want multiple lines, since this interferes with 
        // logging interface?
        writeln!(f, "ND:      {}", self.nd())?;
        writeln!(f, "AFL:     {}", self.afl())?;
        writeln!(f, "RWBF:    {}", self.rwbf())?;
        writeln!(f, "PLMR:    {}", self.plmr())?;
        writeln!(f, "PHMR:    {}", self.phmr())?;
        writeln!(f, "CM:      {}", self.cm())?;
        writeln!(f, "SAGAW:   {}", self.sagaw())?;
        writeln!(f, "MGAW:    {}", self.mgaw())?;
        writeln!(f, "ZLR:     {}", self.zlr())?;
        writeln!(f, "FRO:     {}", self.fro())?;
        writeln!(f, "SLLPS:   {}", self.sllps())?;
        writeln!(f, "PSI:     {}", self.psi())?;
        writeln!(f, "NFR:     {}", self.nfr())?;
        writeln!(f, "MAMV:    {}", self.mamv())?;
        writeln!(f, "DWD:     {}", self.dwd())?;
        writeln!(f, "DRD:     {}", self.drd())?;
        writeln!(f, "FL1GP:   {}", self.fl1gp())?;
        writeln!(f, "PI:      {}", self.pi())?;
        writeln!(f, "FL5LP:   {}", self.fl5lp())?;
        writeln!(f, "ESIRTPS: {}", self.esirtps())?;
        writeln!(f, "ESRTPS:  {}", self.esrtps())?;
    
        Ok(())
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

impl core::fmt::Display for ExtendedCapability {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // TODO: Do we really want multiple lines, since this interferes with 
        // logging interface?
        writeln!(f, "C:       {}", self.c())?;
        writeln!(f, "QI:      {}", self.qi())?;
        writeln!(f, "DT:      {}", self.dt())?;
        writeln!(f, "IR:      {}", self.ir())?;
        writeln!(f, "EIM:     {}", self.eim())?;
        writeln!(f, "PT:      {}", self.pt())?;
        writeln!(f, "SC:      {}", self.sc())?;
        writeln!(f, "IRO:     {}", self.iro())?;
        writeln!(f, "MHMV:    {}", self.mhmv())?;
        writeln!(f, "MTS:     {}", self.mts())?;
        writeln!(f, "NEST:    {}", self.nest())?;
        writeln!(f, "PRS:     {}", self.prs())?;
        writeln!(f, "ERS:     {}", self.ers())?;
        writeln!(f, "SRS:     {}", self.srs())?;
        writeln!(f, "NWFS:    {}", self.nwfs())?;
        writeln!(f, "EAFS:    {}", self.eafs())?;
        writeln!(f, "PSS:     {}", self.pss())?;
        writeln!(f, "PASID:   {}", self.pasid())?;
        writeln!(f, "DIT:     {}", self.dit())?;
        writeln!(f, "PDS:     {}", self.pds())?;
        writeln!(f, "SMTS:    {}", self.smts())?;
        writeln!(f, "VCS:     {}", self.vcs())?;
        writeln!(f, "SLADS:   {}", self.slads())?;
        writeln!(f, "SLTS:    {}", self.slts())?;
        writeln!(f, "FLTS:    {}", self.flts())?;
        writeln!(f, "SMPWCS:  {}", self.smpwcs())?;
        writeln!(f, "RPS:     {}", self.rps())?;
        writeln!(f, "ADMS:    {}", self.adms())?;
        writeln!(f, "RPRIVS:  {}", self.rprivs())?;

        Ok(())
    }
}

/// Bits corresponding to commands in the Global Command register.
pub enum GlobalCommand {
    /// Compatibility Format Interrupt
    CFI   = 1 << 23,
    /// Set Interrupt Remap Table Pointer
    SIRTP = 1 << 24,
    /// Interrupt Remapping Enable
    IRE   = 1 << 25,
    /// Queued Invalidation Enable
    QIE   = 1 << 26,
    /// Write Buffer Flush
    WBF   = 1 << 27,
    /// Enable Advanced Fault Logging
    EAFL  = 1 << 28,
    /// Set Fault Log
    SFL   = 1 << 29,
    /// Set Root Table Pointer
    SRTP  = 1 << 30,
    /// Translation Enable
    TE    = 1 << 31,
}

bitflags! {
    /// Global status register flags.
    ///
    /// The lowest 22 bits are `RsvdZ`. This is Intel parleance meaning that
    /// they may be used in the future, but for now, all writes to these
    /// bits must have the value 0. Since these bits may have values in the
    /// future, the `from_bits_truncate()` method should be used to construct
    /// this object from the underlying bit representation. Note of course
    /// that this conversion is not invertible.
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
