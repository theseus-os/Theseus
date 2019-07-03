//! Performance in Hardware, Isolation in Software.
//! 
//! The PHIS principle is one of the guiding lights in the design of Theseus. 
//! It states that hardware should only be responsible for improving performance and efficiency,
//! but should have no role (or a minimal role) in providing isolation, safety, and security. 
//! Those characteristics should be the responsibility of software, not hardware. 
//! 
//! One of Theseus's goals is to transcend the reliance on hardware to provide isolation,
//! mainly by completely foregoing hardware privilege levels, such as x86's Ring 0 - Ring 3 distinctions. 
//! Instead, we run all code at Ring 0, including user applications that are written in purely safe Rust,
//! because we can guarantee at compile time that a given application or kernel module 
//! cannot violate the isolation between modules, rendering hardware privilege levels obsolete. 
//! 
//! Why? Meltdown, Spectre... need I say more?
//! 
//! 
//! // TODO mention TSA precheck analogy? 
//! 
//! 
//! TODO: finish this up.
