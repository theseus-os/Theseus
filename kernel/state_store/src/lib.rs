#![no_std]

#[cfg(test)] 
#[macro_use] extern crate std;


extern crate alloc;
// #[macro_use] extern crate lazy_static; // for lazy static initialization
extern crate spin;
extern crate atomic_linked_list; 
// #[macro_use] extern crate mopa;

// #[macro_use] mod downcast_rs_no_std;
// use downcast_rs_no_std::Downcast;

// Any trait allows dynamic type checks
use core::any::{Any, TypeId};
use core::sync::atomic::{AtomicPtr, Ordering};
use alloc::boxed::Box;
use spin::{Once};
use alloc::sync::{Arc, Weak};
use atomic_linked_list::atomic_map::AtomicMap;



/// Thanks to Rust's lack of a base type like Java's Object,
/// we have to use this dumbass List structure of Box<Any>,
/// in which what we actually add to the List is not the type T itself, 
/// but rather an Arc<T> that itself is wrapped in the Box.
/// In summary, Any = Arc<T>, and Box<Any> = Box<Arc<T>>
/// in order for us to obtain weak references to that specific type T.
struct SystemStateList( pub AtomicMap<TypeId, Box<dyn Any>> ); 
impl SystemStateList {
	fn new() -> SystemStateList {
		SystemStateList( AtomicMap::new() )
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

/// A key-value store containing all of the system-wide states,
/// of which there is only one instance of each (underlying storage is an AtomicMap, backed by an AtomicLinkedList).
/// Instead of wrapping the entire SYSTEM_STATE map in a coarse-grained global lock (RwLock or Mutex),
/// the caller can use fine-grained locking to protect each element individually.
///
/// This design permits individual modules to retain Weak Arc references to their individual data elements
/// without having to constantly ask the data store module to get and re-insert a module's state.
static SYSTEM_STATE: Once<SystemStateList> = Once::new();

pub fn init() {
	SYSTEM_STATE.call_once( || {
		SystemStateList::new()
	});
}





/// Inserts a new SystemState-implementing type into the map. 
// /// If the map did not previously have a SystemState of this type, `None` is returned.
// /// If the map did previously have one, the value is updated, and the old value is returned.
pub fn insert_state<S: Any>(state: S) -> Option<S> {
	let old_val: Option<Box<dyn Any>> = SYSTEM_STATE.get().expect("SYSTEM_STATE uninited").0.insert(
		TypeId::of::<S>(), 
		Box::new( Arc::new(state) ) // Arc<S> is the type that is represented by Any
	);

	// now we have the old value, we need to downcast it from Any to S, 
	// and then obtain a single-count Arc<S> from that &Arc<S>,
	// while we achieve by cloning the &Arc<S> and then letting it drop out of scope.
	let solo_arc = match old_val {
		Some(g) => g.downcast_ref::<Arc<S>>().map(Arc::clone),
		_ => None,
	};

	// now that this Arc is a single reference, unwrap it to take ownership of its inner S
	solo_arc.and_then( |s_arc| Arc::try_unwrap(s_arc).ok())
}


/// A thread-safe cached reference to a system-wide state.
/// Internally, this contains a Weak pointer to `S`,
/// which is upgraded / updated whenenver the caller invokes `get()`.
pub struct SSCached<S: Any> ( AtomicPtr<Option<Weak<S>>> );
impl<S: Any> SSCached<S> {

	/// Tries to upgrade the internal Weak pointer to a Strong (Arc) pointer.
	/// If successful, the weak pointer is still valid, so we return the `Arc<S>`. 
	/// If not, the current internal Weak reference is None, so we try to reaquire it.
	/// If it cannot be reacquired, there is currently not a system-wide state of type `S`,
	/// so we return None.  
	#[allow(unreachable_code)]
	pub fn get(&self) -> Option<Arc<S>> {
		// this is the VERY common case, simply loading the cached weak pointer and upgrading it
		// SAFE: because we're the only ones able to access this AtomicPtr
		let val: &Option<Weak<S>> = unsafe{ &*self.0.load(Ordering::Acquire) };
		if let Some(ref v) = val {
			if let Some(arc) = v.upgrade() {
				// weird structure, because we only want to return if upgrade works!
				return Some(arc);
			}
		}

		// remove this when we support real fault tolerance
		panic!("In state_store SSCached::get():  reached a fault tolerance condition; cached SystemState was None!");

		// here: cached value was none, so try to get it again
		let new_state = get_state_internal::<S>(); 
		let (new_cached_val, return_val) = match new_state {
			Some(ns_weak) => ( Some(Weak::clone(&ns_weak)), ns_weak.upgrade() ), // NOTE: I haven't been able to fully test this yet
			_ => (None, None)
		};
		
		// update the cached pointer to the new value
		let old_ptr = self.0.swap(Box::into_raw(Box::new(new_cached_val)), Ordering::Release);

		// clean up the old cached value to allow it to be dropped
        // SAFE: we are the only ones touching this AtomicPtr
        unsafe {
            let _ = Box::from_raw(old_ptr);
        }

		return_val
	}
}


/// Returns a Weak reference to the SystemState of the requested type `S`,
/// which the caller can downcast into the specific type `S`
/// by invoking downcast_ref() on the data inside the Weak<> wrapper.
/// It's safe for the caller to save/cache the returned value. 
pub fn get_state<S: Any>() -> SSCached<S> {
	SSCached( AtomicPtr::new(Box::into_raw(Box::new(get_state_internal::<S>()))) )
}



fn get_state_internal<S: Any>() -> Option<Weak<S>> {
	SYSTEM_STATE.get().expect("SYSTEM_STATE uninited").0
		.get(&TypeId::of::<S>())                       // get the Option<Arc<Any>> value
		.and_then( |g| g.downcast_ref::<Arc<S>>())    // if it's Some(g), then downcast g to S
		.map( |dcast_arc| Arc::downgrade(dcast_arc))  // transform result of downcast to weak ptr
}




// --------------- TESTING BELOW  ----------------------

#[cfg(test)] 
#[derive(Debug)]
struct TestStruct {
	data: u32,
	extra: u64,
}

#[cfg(test)] 
#[derive(Debug)]
struct Red (u64);

#[cfg(test)] 
#[derive(Debug)]
struct Green (u64);


// To run this:  cargo test statetest -- --nocapture
#[test]
fn statetest() {

	// add some things to SYSTEM_STATE
	println!("here1 ");
	let res = insert_state(Red(32));
	println!("here2, res = {:?} ", res);
	let res = insert_state(Green(42));
	println!("here3, res = {:?} ", res);
	let oldred = insert_state(Red(55));
	println!("oldred = {:?}", oldred);
	let res = insert_state(TestStruct{
		data: 45,
		extra: 239874,
	});
	println!("here4, res = {:?} ", res);


	// try to retrieve those things from the SYSTEM_STATE
	{
		let red = get_state::<Red>().get();
		println!("here5 ");
		let green = get_state::<Green>().get();
		println!("here6 ");
		let test = get_state::<TestStruct>().get();
		println!("here7 ");

		let rr: Option<Arc<Red>> = get_state().get();
	
		println!("r: {:?} g: {:?} t: {:?}, rr: {:?}", red, green, test, rr);

		// let redred: &Red = red.downcast_ref().expect();
		// let greengreen: &Green = green.downcast_ref().expect();
		// let testtest: &TestStruct = test.downcast_ref::<TestStruct>().expect();
	}

	println!("DONE!");

}