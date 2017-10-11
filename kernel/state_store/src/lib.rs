#![no_std]
#![feature(const_fn)]
#![feature(collections)]
#![feature(alloc)]
#![feature(drop_types_in_const)]
#![feature(core_intrinsics)]


extern crate collections;
extern crate alloc;
#[macro_use] extern crate lazy_static; // for lazy static initialization
extern crate spin;
extern crate typemap;
extern crate atomic_linked_list; 


#[macro_use] mod downcast_rs_no_std;
use downcast_rs_no_std::Downcast;


// Any trait allows dynamic type checks
use core::any::{Any, TypeId};
use collections::BTreeMap;
use alloc::boxed::Box;
use spin::{RwLock, Mutex, Once};
use typemap::{TypeMap, Key};
use alloc::arc::{Arc, Weak};
use core::sync::atomic::{Ordering, AtomicPtr};
use atomic_linked_list::{AtomicLinkedList, AtomicLinkedListIter};



/// The trait that all types must implement if they want to be
/// eligible for inclusion in the system-wide data store.
///
/// To wrap this in a Mutex, it has to be Send.
/// To wrap this in a RwLock, it has to be Sync.
pub trait SystemState : Downcast + Send { }
impl_downcast!(SystemState);



// type SSData = RwLock<Box<SystemState>>;
type SSData = Box<SystemState>;
type SSResult<S: SystemState> = Option<Weak<Box<S>>>;  // the thing inside Weak<> should match SSData


struct SystemStateNode {
	data: Arc<SSData>,
	typ: &'static str,
}
impl SystemStateNode {
	fn new<S: SystemState>(s: S) -> SystemStateNode {
		SystemStateNode {
			data: Arc::new(Box::new(s)),
			// SAFE: just using a rust internal type function
            typ: unsafe { core::intrinsics::type_name::<S>() },
		}
	}

}



struct SystemStateList( pub AtomicLinkedList<SystemStateNode> ); 
impl SystemStateList {
	fn new() -> SystemStateList {
		let list: AtomicLinkedList<SystemStateNode> = AtomicLinkedList::new();
		SystemStateList( list )
	}
}


// /// A convenience method that converts Weak<SystemState> to Arc<S>.
// /// This function takes a Weak pointer to a generic object that implements the `SystemState` Trait
// /// and returns a strong Arc reference to the given specific subtype `S`. 
// /// Returns None if the 
// pub fn upgrade_downcast<'a, S: SystemState>(weak_ptr: &'a Weak<SSData>) -> Option<&'a S> {
// 	/*
// 	let strong = weak_ptr.upgrade();
// 	match strong {
// 		Some(d) => d.downcast_ref::<S>(),
// 		_ => None,
// 	}
// 	*/
	
// 	None

// 	// weak_ptr.upgrade().map( |p| { p.downcast_ref::<S>().unwrap() } )

// 	// let strong = weak_ptr.upgrade().unwrap();
// 	// strong.downcast_ref::<S>()
// }


lazy_static! {
	/// A key-value store containing all of the system-wide states,
	/// of which there is only one instance of each (underlying storage is a BTreeMap). 
	/// Instead of wrapping the entire SYSTEM_STATE map in a coarse-grained global lock (RwLock or Mutex),
	/// we use fine-grained locking to protect each element individually. 
	///
	/// This design permits individual modules to retain Arc references to their individual data elements
	/// without having to constantly ask the data store module to get and re-insert a module's state.
	static ref SYSTEM_STATE: SystemStateList = SystemStateList::new();
}



/// Inserts a new SystemState-implementing type into the map. 
/// If the map did not previously have a SystemState of this type, `None` is returned.
/// If the map did previously have one, the value is updated, and the old value is returned.
pub fn insert_state<S: SystemState>(state: S) -> Option<Arc<SSData>> {
	// before adding the new state, first check if another one of the same type exists
	for elem in SYSTEM_STATE.0.iter_mut() {
		// SAFE, just getting core intrinsic type string
		if elem.typ == unsafe { core::intrinsics::type_name::<S>() } {
			
			// use mem::replace to swap our new node into the old node's place
			let old_data = core::mem::replace(&mut elem.data, Arc::new(Box::new(state)));
			return Some(old_data);
		}
	}
	
	// here: an element with the type S did not yet exist, so just add it
	SYSTEM_STATE.0.push_front(SystemStateNode::new(state));
	None
}

/// Returns a Weak reference to the SystemState of the requested type `S`,
/// which the caller can downcast into the specific type `S`
/// by invoking downcast_ref() on the data inside the Weak<> wrapper.
/// It's safe for the caller to save/cache the returned value. 
pub fn get_state<S: SystemState>() -> Option<Weak<SSData>> {
	for elem in SYSTEM_STATE.0.iter() {
		// SAFE, just getting core intrinsic type string
		if elem.typ == unsafe { core::intrinsics::type_name::<S>() } {
			return Some(Arc::downgrade(&elem.data));
		}
	}
	
	None
}







#[cfg(test)] 
#[macro_use] extern crate std;

#[cfg(test)] 
#[macro_use] extern crate debugit;


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


#[test]
fn statetest() {

	// add some things to SYSTEM_STATE
	println!("here1 ");
	insert_state(Red(32));
	println!("here2 ");
	insert_state(Green(42));
	println!("here3 ");
	let oldred = insert_state(Red(55));
	let rref = oldred.unwrap();
	let red: Option<&Red> = rref.downcast_ref();
	println!("oldred = {:?}", red);
	insert_state(TestStruct{
		data: 45,
		extra: 239874,
	});
	println!("here4 ");

	let rw: Weak<SSData>;

	// try to retrieve those things from the SYSTEM_STATE
	{
		let r = get_state::<Red>().expect("r not found"); 
		println!("here5 ");
		let g = get_state::<Green>().expect("g not found");;
		println!("here6 ");
		let t = get_state::<TestStruct>().expect("t not found");;
		println!("here7 ");
	
		let rr: Weak<SSData> = get_state::<Red>().expect("rr not found");
		rw = r.clone();

		// let r: Red = r.upgrade().unwrap().downcast_ref();
		let rref = r.upgrade().unwrap();
		let red: Option<&Red> = rref.downcast_ref();

		let gref = g.upgrade().unwrap();
		let green: Option<&Green> = gref.downcast_ref();

		let tref = t.upgrade().unwrap();
		let test: Option<&TestStruct> = tref.downcast_ref();
		
	
		println!("r: {:?} g: {:?} t: {:?}", red, green, test);

		// let redred: &Red = red.downcast_ref().expect();
		// let greengreen: &Green = green.downcast_ref().expect();
		// let testtest: &TestStruct = test.downcast_ref::<TestStruct>().expect();
	}

	println!("DONE!");

	let val = rw.upgrade().unwrap();		
	println!("rw = {:?}", val.downcast_ref::<Red>());
	
}