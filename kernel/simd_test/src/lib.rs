#![no_std]

#[macro_use] extern crate log;
extern crate pit_clock;
extern crate packed_simd;

use packed_simd::f32x4;

pub fn test1(_: ()) {
    warn!("at the top of simd_test::test1! simd_personality = {}, sse2 = {}", cfg!(simd_personality), cfg!(target_feature = "sse2"));

    let mut x = f32x4::new(1.111, 11.11, 111.1, 1111.0);
    let y = f32x4::new(0.0, 0.0, 0.0, 0.0);

    let mut loop_ctr = 0;
    loop {
        x = add(x, y);
        if loop_ctr % 50000 == 0 {
            debug!("SIMD TEST1 (should be 1.111, 11.11, 111.1, 1111): {:?}", x);
        }
        loop_ctr += 1;
        // for _ in 1..10 {
        //     let _ = pit_clock::pit_wait(50000);
        // }
    }
}

pub fn test2(_: ()) {
    warn!("at the top of simd_test::test2! simd_personality = {}, sse2 = {}", cfg!(simd_personality), cfg!(target_feature = "sse2"));
    let mut x = f32x4::new(2.222, 22.22, 222.2, 2222.0);
    let y = f32x4::new(0.0, 0.0, 0.0, 0.0);

    let mut loop_ctr = 0;
    loop {
        x = add(x, y);
        if loop_ctr % 50000 == 0 {
            trace!("SIMD TEST2 (should be 2.222, 22.22, 222.2, 2222): {:?}", x);
        }
        loop_ctr += 1;
        // for _ in 1..10 {
        //     let _ = pit_clock::pit_wait(50000);
        // }
    }
}


pub fn test_short(_: ()) {
    warn!("at the top of simd_test::test_short! simd_personality = {}, sse2 = {}", cfg!(simd_personality), cfg!(target_feature = "sse2"));
    let mut x = f32x4::new(3.333, 33.33, 333.3, 3333.0);
    let y = f32x4::new(0.0, 0.0, 0.0, 0.0);

    for i in 0..10 {
        x = add(x, y);
        trace!("SIMD TEST_SHORT [{}] (should be 3.333, 33.33, 333.3, 3333): {:?}", i, x);
    }
}


#[inline(never)]
fn add (a: f32x4, b: f32x4) -> f32x4 {
    a + b
}