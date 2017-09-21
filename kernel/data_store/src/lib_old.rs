#![no_std]
#![feature(const_fn)]
#![feature(collections)]
#![feature(alloc)]

extern crate collections;
extern crate alloc;
#[macro_use] extern crate lazy_static; // for lazy static initialization
extern crate spin;
extern crate typemap;


// Any trait allows dynamic type checks
use core::any::{Any, TypeId};
use collections::BTreeMap;
use alloc::boxed::Box;
use spin::{Mutex, Once};
use typemap::{TypeMap, Key};


// TODO: automatically derive this for types that use it
trait SystemWideState { 
	fn do_something(&self);
}

#[derive(Debug)]
struct TestStruct {
	data: u32,
	extra: u64,
}
impl SystemWideState for TestStruct { 
	fn do_something(&self) { }
}

struct SystemWideStateMap<S>( BTreeMap<TypeId, Box<S>> );

impl<S> SystemWideStateMap<S> {
	fn new() -> SystemWideStateMap<S> {
		let map: BTreeMap<TypeId, Box<S>> = BTreeMap::new();
		SystemWideStateMap( map )
	}

	fn add<T: 'static>(&mut self, s: S, t: T) {
		self.0.insert(TypeId::of::<T>(), Box::new(s));
	}

	fn find<T: 'static>(&self, t: T) -> Option<&Box<S>> {
        self.0.get(&TypeId::of::<T>())
    }	
}


// trait SystemWideStateFinder {
//     fn add<T: SystemWideState>(&mut self, SystemWideState: T);
//     fn find<T: SystemWideState>(&self) -> Option<&T>;
// }

// type SystemWideStateMap = BTreeMap<TypeId, Box<Any>>;
// impl SystemWideStateFinder for SystemWideStateMap {
//     fn add<T: SystemWideState>(&mut self, SystemWideState: T) {
//         self.insert(TypeId::of::<T>(), Box::new(SystemWideState));
//     }
//     fn find<T: SystemWideState>(&self) -> Option<&T> {
//         self.get(&TypeId::of::<T>()).and_then(|f| f.downcast_ref())
//     }
// }



#[derive(Debug)]
struct Red (u64);
#[derive(Debug)]
struct Green (u64);


impl SystemWideState for Red {
	fn do_something(&self) { }
}
impl SystemWideState for Green {
	fn do_something(&self) { }
}


/// A key-value store containing all of the system-wide states,
/// of which there is only one instance of each (underlying storage is a BTreeMap). 
// lazy_static! {
// 	static ref SYSTEM_WIDE_STATE: Mutex<SystemWideStateMap> = Mutex::new(SystemWideStateMap::new());
// }

// static SYSTEM_WIDE_STATE: Once<Mutex<SystemWideStateMap<SystemWideState>>> = Once::new();

fn main() {
	// let mut map: SystemWideStateMap<SystemWideState> = SystemWideStateMap::new();

	let  state: Mutex<BTreeMap<TypeId, Box<SystemWideState>>> = Mutex::new(BTreeMap::new());
	// let state = SYSTEM_WIDE_STATE.call_once( || {
	// 	Mutex::new(SystemWideStateMap::new())
	// });
	let mut map = state.lock();

    // map.add(Red(23), Red);
    // map.add(Green(43), Green);
	map.insert(TypeId::of::<Red>(), Box::new(Red(32)));
	map.insert(TypeId::of::<Green>(), Box::new(Green(42)));
	map.insert(TypeId::of::<TestStruct>(), Box::new(TestStruct{
		data: 45,
		extra: 239874,
	}));

    // Note that find() magically works! It picks the type from the variable.
    // let red = map.find(Red).expect("red should be in there");
    // let green = map.find(Green).expect("green should be in there, too");

	let red = map.get(&TypeId::of::<Red>()).unwrap();
	let green = map.get(&TypeId::of::<Green>()).unwrap();
	let test = map.get(&TypeId::of::<TestStruct>()).unwrap();
	
	// let red_ref = red.downcast_ref::<Red>();

	// if let Ok(val) = red.downcast_ref::<Red>() {
		// let () = val;
	// }
	// let rbox: &Box<Red> = red.unwrap() as &Box<Red>;
}

#[allow(dead_code)]
fn main2() {
	let  state: Mutex<BTreeMap<TypeId, Box<Any>>> = Mutex::new(BTreeMap::new());

	let mut map = state.lock(); 

	map.insert(TypeId::of::<Red>(), Box::new(Red(32)));
	map.insert(TypeId::of::<Green>(), Box::new(Green(42)));
	map.insert(TypeId::of::<TestStruct>(), Box::new(TestStruct{
		data: 45,
		extra: 239874,
	}));

    // Note that find() magically works! It picks the type from the variable.
    // let red = map.find(Red).expect("red should be in there");
    // let green = map.find(Green).expect("green should be in there, too");

	let red = map.get(&TypeId::of::<Red>()).unwrap();
	let green = map.get(&TypeId::of::<Green>()).unwrap();
	let test = map.get(&TypeId::of::<TestStruct>()).unwrap();
	
	let redred: &Red = red.downcast_ref::<Red>().unwrap();
	let greengreen: &Green = green.downcast_ref::<Green>().unwrap();
	let testtest: &TestStruct = test.downcast_ref::<TestStruct>().unwrap();
}


// #[test]
// fn testme() {
// 	// main();
// 	main2();
// }