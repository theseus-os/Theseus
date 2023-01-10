//! A cryptographically secure source of randomness.
//!
//! The randomness is provided by a global, cryptographically secure
//! pseudorandom number generator. More specifically,
//! [`rand_chacha::ChaCha20Rng`].
//!
//! The CSPRNG is instantiated using [`lazy_static`] and hence it is initialized
//! lazily on the first request for randomness. It attempts to obtains a seed
//! from the following sources in order:
//! - `RDSEED`
//! - `RDRAND`
//! - `TSC`
//!
//! An error will be logged if the `TSC` is used as it is not a high quality
//! source of randomness.
//!
//! If a consumer requires one-off randomness, [`next_u32`], [`next_u64`], or
//! [`fill_bytes`] should be used. Otherwise, [`init_rng`] should be used to
//! seed a local PRNG, which can then be used as a source of randomness. Using a
//! local PRNG avoids contention on the global CSPRNG and allows for PRNGs
//! better suited for the task (e.g. non-crypto PRNGs).

#![no_std]

use rand_chacha::{
    rand_core::{RngCore, SeedableRng},
    ChaCha20Rng,
};
use spin::mutex::Mutex;

pub use rand_chacha::rand_core::Error;

lazy_static::lazy_static! {
    /// The global random number generator.
    ///
    /// This PRNG is cryptographically-secure. It can be directly accessed using
    /// [`next_u32`], [`next_u64`], and [`fill_bytes`] for one-off randomness.
    /// However, it should mostly be used to seed local PRNGs using
    /// [`init_rng`].
    ///
    /// Using a single global CSPRNG allows us to feed it with entropy from
    /// device drivers and such.
    static ref CSPRNG: Mutex<ChaCha20Rng> = {
        let seed = rdseed_seed()
            .or_else(rdrand_seed)
            .unwrap_or_else(tsc_seed);
        Mutex::new(ChaCha20Rng::from_seed(seed))
    };
}

/// Tries to generate a 32 byte seed using the RDSEED x86 instruction.
fn rdseed_seed() -> Option<[u8; 32]> {
    match rdrand::RdSeed::new() {
        Ok(mut generator) => {
            let mut seed = [0; 32];
            match generator.try_fill_bytes(&mut seed) {
                Ok(_) => {
                    log::info!("using RDSEED for CSPRNG seed");
                    Some(seed)
                }
                Err(_) => {
                    log::warn!("failed to generate seed from RDSEED");
                    None
                }
            }
        }
        Err(_) => {
            log::warn!("failed to initialise RDSEED");
            None
        }
    }
}

/// Tries to generate a 32 byte seed using the RDRAND x86 instruction.
fn rdrand_seed() -> Option<[u8; 32]> {
    match rdrand::RdRand::new() {
        Ok(mut generator) => {
            let mut seed = [0; 32];
            match generator.try_fill_bytes(&mut seed) {
                Ok(_) => {
                    log::info!("using RDRAND for CSPRNG seed");
                    Some(seed)
                }
                Err(_) => {
                    log::warn!("failed to generate seed from RDRAND");
                    None
                }
            }
        }
        Err(_) => {
            log::warn!("failed to initialise RDRAND");
            None
        }
    }
}

/// Generates a 32 byte seed using the TSC.
///
/// The TSC isn't a high quality source of randomness and so it is only used as
/// a last resort if both RDSEED and RDRAND fail.
fn tsc_seed() -> [u8; 32] {
    let mut seed = [0; 32];

    for s in &mut seed {
        // The last byte is the _most_ random.
        *s = u128::from(tsc::tsc_ticks()).to_be_bytes()[15];
    }

    // The TSC isn't a high quality source of randomness.
    log::error!("using TSC for CSPRNG seed - this is not ok");
    seed
}

/// Returns a random [`u32`].
///
/// Consider using [`init_rng`] if calling this function in a loop, or if you
/// don't require cryptographically secure random numbers.
pub fn next_u32() -> u32 {
    let mut csprng = CSPRNG.lock();
    csprng.next_u32()
}

/// Returns a random [`u64`].
///
/// Consider using [`init_rng`] if calling this function in a loop, or if you
/// don't require cryptographically secure random numbers.
pub fn next_u64() -> u64 {
    let mut csprng = CSPRNG.lock();
    csprng.next_u64()
}

/// Fills `dest` with random data.
///
/// Consider using [`init_rng`] if calling this function in a loop, or if you
/// don't require cryptographically secure random numbers.
pub fn fill_bytes(dest: &mut [u8]) {
    let mut csprng = CSPRNG.lock();
    csprng.fill_bytes(dest);
}

/// Initialises a `T` RNG.
///
/// Directly accessing the global CSPRNG can be expensive and so it is often
/// better to seed a local PRNG from the global CSPRNG. Using a local PRNG
/// also allows for much faster cryptographically insecure PRNGs to be used.
pub fn init_rng<T>() -> Result<T, Error>
where
    T: SeedableRng,
{
    let mut csprng = CSPRNG.lock();
    T::from_rng(&mut *csprng)
}
