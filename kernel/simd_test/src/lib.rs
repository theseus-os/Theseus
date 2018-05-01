#![no_std]

#![feature(stdsimd)]

#[macro_use] extern crate log;
extern crate pit_clock;

// extern crate simd;


use core::simd::f32x4;

pub fn test1(_: ()) {
    let mut x = f32x4::new(1.111, 2.111, 3.111, 4.111);
    let y = f32x4::new(1.0, 1.0, 1.0, 1.0);

    loop {
        x = add(x, y);
        debug!("SIMD TEST1: {} {} {} {}", x.extract(0), x.extract(1), x.extract(2), x.extract(3));
        for _ in 1..10 {
            let _ = pit_clock::pit_wait(50000);
        }
    }
}

pub fn test2(_: ()) {
    let mut x = f32x4::new(1.222, 2.222, 3.222, 4.222);
    let y = f32x4::new(1.0, 1.0, 1.0, 1.0);

    loop {
        x = add(x, y);
        debug!("SIMD TEST2: {} {} {} {}", x.extract(0), x.extract(1), x.extract(2), x.extract(3));
        for _ in 1..10 {
            let _ = pit_clock::pit_wait(50000);
        }
    }
}

pub fn test3(_: ()) {
    let mut x = f32x4::new(1.333, 2.333, 3.333, 4.333);
    let y = f32x4::new(1.0, 1.0, 1.0, 1.0);

    loop {
        x = add(x, y);
        debug!("SIMD TEST3: {} {} {} {}", x.extract(0), x.extract(1), x.extract(2), x.extract(3));
        for _ in 1..10 {
            let _ = pit_clock::pit_wait(50000);
        }
    }
}


fn add (a: f32x4, b: f32x4) -> f32x4 {
    a + b
}