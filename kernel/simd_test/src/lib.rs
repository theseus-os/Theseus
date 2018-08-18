#![no_std]

#![feature(stdsimd)]

#[macro_use] extern crate log;
extern crate pit_clock;

// extern crate simd;


use core::simd::f32x4;

pub fn test1(_: ()) {
    trace!("at the top of simd_test::test1.");
    let mut x = f32x4::new(1.111, 11.11, 111.1, 1111.0);
    let y = f32x4::new(0.0, 0.0, 0.0, 0.0);

    loop {
        x = add(x, y);
        debug!("SIMD TEST1 (should be 1.111, 11.11, 111.1, 1111): {:?}", x); // {} {} {}", x.extract(0), x.extract(1), x.extract(2), x.extract(3));
        // for _ in 1..10 {
        //     let _ = pit_clock::pit_wait(50000);
        // }
    }
}

pub fn test2(_: ()) {
    trace!("at the top of simd_test::test2.");
    let mut x = f32x4::new(2.222, 22.22, 222.2, 2222.0);
    let y = f32x4::new(0.0, 0.0, 0.0, 0.0);

    loop {
        x = add(x, y);
        trace!("SIMD TEST2 (should be 2.222, 22.22, 222.2, 2222): {:?}", x); //" {} {} {}", x.extract(0), x.extract(1), x.extract(2), x.extract(3));
        // for _ in 1..10 {
        //     let _ = pit_clock::pit_wait(50000);
        // }
    }
}

// pub fn test3(_: ()) {
//     trace!("at the top of simd_test::test3.");
//     let mut x = f32x4::new(1.333, 2.333, 3.333, 4.333);
//     let y = f32x4::new(1.0, 1.0, 1.0, 1.0);

//     loop {
//         x = add(x, y);
//         debug!("SIMD TEST3: {} {} {} {}", x.extract(0), x.extract(1), x.extract(2), x.extract(3));
//         // for _ in 1..10 {
//         //     let _ = pit_clock::pit_wait(50000);
//         // }
//     }
// }


#[inline(never)]
fn add (a: f32x4, b: f32x4) -> f32x4 {
    a + b
}