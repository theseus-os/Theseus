#![no_std]

use rand_chacha::{
    rand_core::{RngCore, SeedableRng},
    ChaCha20Rng,
};
use spin::{mutex::Mutex, once::Once};

/// The global random number generator.
///
/// This PRNG is cryptographically-secure. It can be directly accessed using
/// [`next_u32`], [`next_u64`], and [`fill_bytes`] for one-off randomness.
/// However, it should mostly be used to seed local PRNGs using [`init_prng`].
///
/// Using a single global CSPRNG allows us to feed it with entropy from device
/// drivers and such.
static CSPRNG: Once<Mutex<ChaCha20Rng>> = Once::new();

/// Initialises the global CSPRNG.
///
/// Only the first call will initialise the CSPRNG. Subsequent calls will
/// return the already initialised CSPRNG.
pub fn init_once() -> &'static Mutex<ChaCha20Rng> {
    CSPRNG.call_once(|| {
        let seed = rdseed_seed()
            .or_else(|_| rdrand_seed())
            .unwrap_or_else(|_| tsc_seed());
        Mutex::new(ChaCha20Rng::from_seed(seed))
    })
}

/// Tries to generate a 32 byte seed using the RDSEED x86 instruction.
fn rdseed_seed() -> Result<[u8; 32], ()> {
    match rdrand::RdSeed::new() {
        Ok(mut generator) => {
            let mut seed = [0; 32];
            match generator.try_fill_bytes(&mut seed) {
                Ok(_) => {
                    log::info!("using RDSEED for CSPRNG seed");
                    Ok(seed)
                }
                Err(_) => {
                    log::warn!("failed to generate seed from RDSEED");
                    Err(())
                }
            }
        }
        Err(_) => {
            log::warn!("failed to initialise RDSEED");
            Err(())
        }
    }
}

/// Tries to generate a 32 byte seed using the RDRAND x86 instruction.
fn rdrand_seed() -> Result<[u8; 32], ()> {
    match rdrand::RdRand::new() {
        Ok(mut generator) => {
            let mut seed = [0; 32];
            match generator.try_fill_bytes(&mut seed) {
                Ok(_) => {
                    log::info!("using RDRAND for CSPRNG seed");
                    Ok(seed)
                }
                Err(_) => {
                    log::warn!("failed to generate seed from RDRAND");
                    Err(())
                }
            }
        }
        Err(_) => {
            log::warn!("failed to initialise RDRAND");
            Err(())
        }
    }
}

/// Generates a 32 byte seed using the TSC.
///
/// The TSC isn't a high quality source of randomness and so it is only used as
/// a last resort if both RDSEED and RDRAND fail.
fn tsc_seed() -> [u8; 32] {
    let mut seed = [0; 32];

    for i in 0..seed.len() {
        // The last byte is the _most_ random.
        seed[i] = u128::from(tsc::tsc_ticks()).to_be_bytes()[15];
    }

    // The TSC isn't a high quality source of randomness.
    log::error!("using TSC for CSPRNG seed - this is not ok");
    seed
}

/// Returns a random [`u32`].
///
/// Consider using [`init_rng`] if calling this function in a loop, or if you
/// don't require cryptographically-secure random numbers.
pub fn next_u32() -> u32 {
    let mut csprng = init_once().lock();
    csprng.next_u32()
}

/// Returns a random [`u64`].
///
/// Consider using [`init_rng`] if calling this function in a loop, or if you
/// don't require cryptographically-secure random numbers.
pub fn next_u64() -> u64 {
    let mut csprng = init_once().lock();
    csprng.next_u64()
}

/// Fills `dest` with random data.
///
/// Consider using [`init_rng`] if calling this function in a loop, or if you
/// don't require cryptographically-secure random numbers.
pub fn fill_bytes(dest: &mut [u8]) {
    let mut csprng = init_once().lock();
    csprng.fill_bytes(dest);
}

/// Initialises a `T` RNG.
///
/// Directly accessing the global CSPRNG can be expensive and so it is often
/// better to seed a local PRNG from the global CSPRNG. Using a local PRNG
/// allows for much faster non-cryptographically-secure PRNGs to be used.
///
/// Even if you require cryptographically-secure randomness, it's often better
/// to use a local CSPRNG as it doesn't require locking the global CSPRNG,
/// except during seeding.
pub fn init_rng<T>() -> Result<T, ()>
where
    T: SeedableRng,
{
    let mut csprng = init_once().lock();
    T::from_rng(&mut *csprng).map_err(|_| ())
}
