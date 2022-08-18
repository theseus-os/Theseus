#![no_std]
#![feature(portable_simd)]

// The entire crate is only built if both `simd_personality` and `sse2` are enabled.
#[macro_use] extern crate cfg_if;
cfg_if! { if #[cfg(all(simd_personality, target_feature = "sse2"))] {

#[macro_use] extern crate log;
extern crate core_simd;

use core_simd::f64x4;

pub fn test1(_: ()) {
    warn!("at the top of simd_test::test1! simd_personality: {}, sse2: {}, avx: {}", 
        cfg!(simd_personality),
        cfg!(target_feature = "sse2"),
        cfg!(target_feature = "avx")
    );

    let mut x = f64x4::from_array([1.111, 11.11, 111.1, 1111.0]);
    let y = f64x4::from_array([0.0, 0.0, 0.0, 0.0]);

    let mut loop_ctr = 0;
    loop {
        x = add(x, y);
        if loop_ctr % 5000000 == 0 {
            debug!("SIMD TEST1 (should be 1.111, 11.11, 111.1, 1111): {:?}", x);
        }
        loop_ctr += 1;
    }
}

pub fn test2(_: ()) {
    warn!("at the top of simd_test::test1! simd_personality: {}, sse2: {}, avx: {}", 
        cfg!(simd_personality),
        cfg!(target_feature = "sse2"),
        cfg!(target_feature = "avx")
    );
    let mut x = f64x4::from_array([2.222, 22.22, 222.2, 2222.0]);
    let y = f64x4::from_array([0.0, 0.0, 0.0, 0.0]);

    let mut loop_ctr = 0;
    loop {
        x = add(x, y);
        if loop_ctr % 5000000 == 0 {
            trace!("SIMD TEST2 (should be 2.222, 22.22, 222.2, 2222): {:?}", x);
        }
        loop_ctr += 1;
    }
}


pub fn test_short(_: ()) {
    warn!("at the top of simd_test::test1! simd_personality: {}, sse2: {}, avx: {}", 
        cfg!(simd_personality),
        cfg!(target_feature = "sse2"),
        cfg!(target_feature = "avx")
    );
    let mut x = f64x4::from_array([3.333, 33.33, 333.3, 3333.0]);
    let y = f64x4::from_array([0.0, 0.0, 0.0, 0.0]);

    for i in 0..10 {
        x = add(x, y);
        trace!("SIMD TEST_SHORT [{}] (should be 3.333, 33.33, 333.3, 3333): {:?}", i, x);
    }
}


#[inline(never)]
fn add (a: f64x4, b: f64x4) -> f64x4 {
    a + b
}

}} // end of cfg_if block