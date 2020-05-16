#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate slabmalloc;
extern crate memory;
extern crate hpet;
extern crate libtest;

use alloc::vec::Vec;
use alloc::string::String;
use slabmalloc::{MappedPages8k, ObjectPage8k};
use memory::{create_mapping, EntryFlags};
use hpet::get_hpet;
use libtest::hpet_2_ns;
extern crate hashbrown;
extern crate fnv;

use hashbrown::HashMap;
use fnv::{FnvBuildHasher, FnvHasher};
use core::hash::{Hasher, BuildHasherDefault};

pub type FnvHashMap<K, V> = HashMap<K, V, FnvBuildHasher>;

pub fn main(_args: Vec<String>) -> isize {
    // info!("Hello, world! (from hello application)");
    println!("Hello, world! Args: {:?}", _args);

    let hpet = get_hpet().unwrap();

    let capacity = 12200;
    let mut orig = Vec::with_capacity(capacity);
    let mut addr = Vec::with_capacity(capacity);
    for i in 0..capacity {
        let mp = create_mapping(8192, EntryFlags::WRITABLE).unwrap();
        addr.push(mp.start_address());
        orig.push(mp);
    }

    let mut vec = Vec::with_capacity(capacity);
    let mut hm1 = HashMap::with_capacity(capacity);
    let mut hm2 = FnvHashMap::with_capacity_and_hasher(capacity, Default::default());
    
    let start = hpet.get_counter();
    for _ in 0..capacity {
        vec.push(orig.pop().unwrap());
    }
    let end = hpet.get_counter();

    println!("vec push time: {} ns", hpet_2_ns(end-start));

    let start = hpet.get_counter();
    for mp in vec.iter() {
        if mp.start_address() == addr[capacity/2] {
            break;
        }
    }
    let end = hpet.get_counter();

    println!("vec iter time: {} ns", hpet_2_ns(end-start));

    let start = hpet.get_counter();
    for _ in 0..capacity {
        orig.push(vec.pop().unwrap());
    }
    let end = hpet.get_counter();

    println!("vec pop time: {} ns", hpet_2_ns(end-start));

    let start = hpet.get_counter();
    for _ in 0..capacity {
        let mp = orig.pop().unwrap();
        hm1.insert(mp.start_address(), mp);
    }
    let end = hpet.get_counter();

    println!("hashbrown insert time: {} ns", hpet_2_ns(end-start));

    let start = hpet.get_counter();
    for mp in hm1.values() {
        if mp.start_address() == addr[capacity/2] {
            break;
        }
    }
    let end = hpet.get_counter();

    println!("hashbrown iter time: {} ns", hpet_2_ns(end-start));
    
    let start = hpet.get_counter();
    let mp = hm1.get(&addr[capacity/2]).unwrap();
    let end = hpet.get_counter();

    println!("hashbrown get time: {} ns, {}", hpet_2_ns(end-start), mp.start_address());

    let start = hpet.get_counter();
    for a in &addr {
        let mp = hm1.remove(&a).unwrap();
        orig.push(mp);
    }
    let end = hpet.get_counter();

    println!("hashbrown remove time: {} ns", hpet_2_ns(end-start));

    let start = hpet.get_counter();
    for _ in 0..capacity {
        let mp = orig.pop().unwrap();
        hm2.insert(mp.start_address(), mp);
    }
    let end = hpet.get_counter();

    println!("fnv insert time: {} ns", hpet_2_ns(end-start));

    let start = hpet.get_counter();
    for mp in hm2.values() {
        if mp.start_address() == addr[capacity/2] {
            break;
        }
    }
    let end = hpet.get_counter();

    println!("fnv iter time: {} ns", hpet_2_ns(end-start));

    let start = hpet.get_counter();
    let mp = hm2.get(&addr[capacity/2]).unwrap();
    let end = hpet.get_counter();

    println!("fnv get time: {} ns, {}", hpet_2_ns(end-start), mp.start_address());

    let start = hpet.get_counter();
    for a in &addr {
        let mp = hm2.remove(&a).unwrap();
        orig.push(mp);
    }
    let end = hpet.get_counter();

    println!("fnv remove time: {} ns", hpet_2_ns(end-start));

    0
}
