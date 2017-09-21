#![no_std]
#![feature(const_fn)]
#![feature(collections)]
#![feature(alloc)]
#![feature(drop_types_in_const)]

extern crate collections;
extern crate alloc;
#[macro_use] extern crate lazy_static; // for lazy static initialization
extern crate spin;
extern crate typemap;


#[macro_use] mod downcast_rs_no_std;
use downcast_rs_no_std::Downcast;


// Any trait allows dynamic type checks
use core::any::{Any, TypeId};
use collections::BTreeMap;
use alloc::boxed::Box;
use spin::{RwLock, Mutex, Once};
use typemap::{TypeMap, Key};
use alloc::arc::Arc;
use core::sync::atomic::{Ordering, AtomicPtr};



/// The trait that all types must implement if they want to be
/// eligible for inclusion in the system-wide data store.
///
/// To wrap this in a Mutex, it has to be Send.
/// To wrap this in a RwLock, it has to be Sync.
pub trait SystemState : Downcast + Send { }
impl_downcast!(SystemState);


#[derive(Debug)]
struct TestStruct {
	data: u32,
	extra: u64,
}
impl SystemState for TestStruct { }

#[derive(Debug)]
struct Red (u64);
impl SystemState for Red { }

#[derive(Debug)]
struct Green (u64);
impl SystemState for Green { }


// type SSValue = Arc<RwLock<Box<SystemState>>>;
type SSValue = Box<SystemState>;

struct SystemWideStateMap( BTreeMap<TypeId, SSValue> );
impl SystemWideStateMap {
	fn new() -> SystemWideStateMap {
		let map: BTreeMap<TypeId, SSValue> = BTreeMap::new();
		SystemWideStateMap( map )
	}

	/// Inserts a new SystemState into the map. 
	/// If the map did not previously have a SystemState of this type, `None` is returned.
	/// If the map did previously have one, the value is updated, and the old value is returned.
	fn add<S: SystemState>(&mut self, state: S) {
		self.0.insert(TypeId::of::<S>(), Box::new(state));
	}

	fn get<S: SystemState>(&self) -> Option<&S> {
        self.0.get(&TypeId::of::<S>()).and_then(|f| f.downcast_ref::<S>())
    }	
}



// #[cfg(test)]
#[macro_use] extern crate std;

// #[cfg(test)] 
#[macro_use] extern crate debugit;


lazy_static! {
	/// A key-value store containing all of the system-wide states,
	/// of which there is only one instance of each (underlying storage is a BTreeMap). 
	/// Instead of wrapping the entire SYSTEM_STATE map in a coarse-grained global lock (RwLock or Mutex),
	/// we use fine-grained locking to protect each element individually. 
	/// Thus, we use the AtomicPtr type as a wrapper to protect the entire state map in a wait-free fashion (safe for IRQ contexts).
	/// To add something to the map, which we expect to be a rare operation compared to getting a reference to a item in the map,
	/// simply get the current map, duplicate it locally, add a new element to that duplicate, and then swap the global pointer with the new one. 
	/// Duplicating the map should be very quick, as it just entails cloning a bunch of Arc references, 
	/// not deeply copying the actual state data.
	///
	/// This design permits individual modules to retain Arc references to their individual data elements
	/// without having to constantly ask the data store module to get and re-insert a module's state.
	static ref SYSTEM_STATE: AtomicPtr<SystemWideStateMap> = AtomicPtr::new( 
		// this Box shit is to keep it in the heap forever and keep it from being dropped
		Box::into_raw(Box::new(SystemWideStateMap::new())) 
	);
}

// fn add<T: 'static>(&mut self, s: S, t: T) {
// 	self.0.insert(TypeId::of::<T>(), Box::new(s));
// }
// fn get<T: 'static>(&self, t: T) -> Option<&Box<S>> {
// 	self.0.get(&TypeId::of::<T>())
// }	


// /// Inserts a new SystemState into the data store. 
// /// If the data store did not previously have a SystemState of this type, `None` is returned.
// /// If the data store did previously have one, the value is updated, and the old value is returned.
// pub fn insert<S: SystemState>(state: S) -> Option<S> {
// 	let mut map = SYSTEM_STATE.write();
// 	map.0.insert(TypeId::of::<S>(), Box::new(state));
// 	None
// }

// pub fn get<'a, S: SystemState>() -> Option<&'a S> {
// 	let mut map = SYSTEM_STATE.read();
// 	map.0.get(&TypeId::of::<S>()).and_then(|f| f.downcast_ref::<S>())
// }



fn main() {
	{
		let raw_map: *mut SystemWideStateMap = SYSTEM_STATE.load(Ordering::SeqCst);
		println!("raw_map = {:?}", raw_map);

		let mut map: &mut SystemWideStateMap = unsafe { &mut *raw_map };
		println!("map     = {:?}", map as *mut _);
		println!("*map    = {:?}", &mut *map as *mut _);

		map.add((Red(234)));
		println!("here1 ");
		(map).add(Red(32));
		println!("here2 ");
		(map).add(Green(42));
		println!("here3 ");
		(map).add(TestStruct{
			data: 45,
			extra: 239874,
		});
		println!("here4 ");

		// FIXME: dangerous because we are directly modifying the loaded *mut value. 
		// We should never modify that value (check the Atom crate); 
		// Instead, we should clone it and then store the cloned value.

		// SYSTEM_STATE.store(map, Ordering::SeqCst);
		// atomic_map.store(map, Ordering::SeqCst);
		println!("here5 ");
	}

	{
		let raw_map: *mut SystemWideStateMap = SYSTEM_STATE.load(Ordering::SeqCst);
		println!("raw_map = {:?}", raw_map);

		let mut map: &mut SystemWideStateMap = unsafe { &mut *raw_map };
		println!("map     = {:?}", map as *mut _);
		println!("*map    = {:?}", &mut *map as *mut _);

		let r = map.get::<Red>().unwrap();
		let g = map.get::<Green>().unwrap();
		let t = map.get::<TestStruct>().unwrap();
	
		let rr: &Red = map.get().unwrap();

		println!("r: {:?} g: {:?} t: {:?}, rr: {:?}", r, g, t, rr);

		// let redred: &Red = red.downcast_ref().unwrap();
		// let greengreen: &Green = green.downcast_ref().unwrap();
		// let testtest: &TestStruct = test.downcast_ref::<TestStruct>().unwrap();
	}


	println!("DONE!");
	
}


#[test]
fn testme() {
	testmain();
	// main2();
}