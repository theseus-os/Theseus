#![no_std]
#![feature(trait_alias)]

extern crate alloc;

use alloc::sync::Arc;

// pub trait PowerDownFn = Fn();

// pub struct Wakelock<F: PowerDownFn + Copy> {
//     ref_count: Arc<()>,
//     refcount_threshold: usize,
//     power_down_fn: F
// }

// impl<F: PowerDownFn + Copy> Wakelock<F> {
//     pub fn create_wakelock(refcount_threshold: usize, power_down_fn: F) -> Wakelock<F> {
//         Wakelock {
//             ref_count: Arc::new(()),
//             refcount_threshold: refcount_threshold,
//             power_down_fn: power_down_fn
//         }
//     }

//     pub fn clone_wakelock(&mut self) -> Wakelock<F> {
//         Wakelock {
//             ref_count: self.ref_count.clone(),
//             refcount_threshold: self.refcount_threshold,
//             power_down_fn: self.power_down_fn
//         }
//     }
// }

// impl<F: PowerDownFn + Copy> Drop for Wakelock<F> {
//     fn drop(&mut self) {
//         if Arc::strong_count(&self.ref_count) <= self.refcount_threshold {
//             (self.power_down_fn)();
//         } 
//     }
// }

pub trait PowerDownFn = Fn();

pub struct Wakelock {
    ref_count: Arc<()>,
    refcount_threshold: usize,
    power_down_fn: fn()
}

impl Wakelock {
    pub fn create_wakelock(refcount_threshold: usize, power_down_fn: fn()) -> Wakelock {
        Wakelock {
            ref_count: Arc::new(()),
            refcount_threshold: refcount_threshold,
            power_down_fn: power_down_fn
        }
    }

    pub fn clone_wakelock(&mut self) -> Wakelock {
        Wakelock {
            ref_count: self.ref_count.clone(),
            refcount_threshold: self.refcount_threshold,
            power_down_fn: self.power_down_fn
        }
    }

    pub fn ref_count(&self) -> usize {
        Arc::strong_count(&self.ref_count)
    }
}

impl Drop for Wakelock {
    fn drop(&mut self) {
        if Arc::strong_count(&self.ref_count) <= self.refcount_threshold {
            (self.power_down_fn)();
        } 
    }
}