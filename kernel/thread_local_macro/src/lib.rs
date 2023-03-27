//! Provides a `thread_local!()` macro, a helper to instantiate lazily-initialized 
//! thread-local storage (TLS) variables.
//! 
//! The primary difference between using this crate's `thread_local!()` macro
//! and directly using the `#[thread_local]` attribute is that `static` items
//! tagged with the `#[thread_local]` attribute will *never* be dropped,
//! just like all other `static`s.
//! 
//! However, static items defined in a `thread_local!()` macro block will be
//! destructed (e.g., dropped, destroyed) when that task exits.
//! 
//! # Rust std-based implementation notes
//! The code in this crate is adapted from [this version of the `thread_local!()` macro]
//! from the Rust standard library.
//! The main design has been left unchanged, but we have removed most of the configuration blocks
//! for complex platform-specific or OS-specific behavior.
//! Because Theseus supports the `#[thread_local]` attribute, we can directly use the 
//! TLS "fast path", which the Rust standard library refers to as the "FastLocalInnerKey".
//! 
//! ## Unsafety
//! We could probably could remove most of the unsafe code from this implementation,
//! because we don't have to account for the various raw platform-specific interfaces 
//! or using raw libc types like the original Rust std implementation does.
//! However, I have chosen to leave the code as close as possible to the original 
//! Rust std implementation in order to make updates as easy as possible,
//! for if and when the Rust std version changes and we wish to track/merge in those changes.
//! 
//! [this version of the `thread_local!()` macro]: https://github.com/rust-lang/rust/blob/3f14f4b3cec811017079564e16a92a1dc9870f41/library/std/src/thread/local.rs

#![no_std]
#![feature(thread_local)]
#![feature(allow_internal_unstable)]

// The code from Rust std uses unsafe blocks within unsafe functions,
// so we preserve that here (for now).
#![allow(unused_unsafe)]

extern crate alloc;

use core::cell::RefCell;

/// The set of TLS objects that have been initialized by a task
/// and need a destructor to be run after that task exits.
///
/// We store these TLS destructors in a raw TLS object itself
/// which is okay as long as this object itself doesn't require a destructor.
/// We achieve this condition in two parts:
/// 1. We statically assert that the `TlsObjectDestructor` doesn't implement [`Drop`],
///    which makes sense because it only holds raw pointer values and function pointers.
/// 2. The actual `Vec` is drained upon task exit by the task cleanup functions
///    in the `spawn` crate`, ensuring that there is no `Vec` memory itself
///    to actually be deallocated, as the contents of this `Vec` have been cleared.
/// 
/// Note that this will always be safe even if the two conditions **aren't** met, 
/// because the only thing that will happen there is a memory leak.
#[thread_local]
static TLS_DESTRUCTORS: RefCell<Vec<TlsObjectDestructor>> = RefCell::new(Vec::new());

/// A TLS data object that has been initialized and requires a destructor to be run.
/// The destructor should be invoked when the task containing this `TlsObjectDestructor` exits.
#[doc(hidden)]
pub struct TlsObjectDestructor {
    /// The raw pointer to the object that needs to be dropped.
    pub object_ptr: *mut u8,
    /// The destructor function that should be invoked with `object_ptr` as its only parameter.
    /// The function itself must be an unsafe one, as it dereferences raw pointers.
    pub dtor: unsafe extern "C" fn(*mut u8),
}
// See the above [`TLS_DESTRUCTORS`] docs for why this is necessary.
const _: () = assert!(!core::mem::needs_drop::<TlsObjectDestructor>());

/// Takes ownership of the list of [`TlsObjectDestructor`]s
/// for TLS objects that have been initialized in this current task's TLS area.
/// 
/// This is only intended to be used by the task cleanup functions
/// after the current task has exited.
#[doc(hidden)]
pub fn take_current_tls_destructors() -> Vec<TlsObjectDestructor> {
    TLS_DESTRUCTORS.take()
}

/// Adds the given destructor callback to the current task's list of
/// TLS destructors that should be run when that task exits.
/// 
/// # Arguments
/// * `a`: the pointer to the object that will be destructed.
/// * `dtor`: the function that should be invoked to destruct the object pointed to by `a`.
///   When the current task exits, this function will be invoked with `a`
///   as its only argument, at which point the `dtor` function should drop `a`.
/// 
/// Currently the only value of `dtor` that is used is a type-specific monomorphized
/// version of the above [`fast::destroy_value()`] function.
fn register_dtor(object_ptr: *mut u8, dtor: unsafe extern "C" fn(*mut u8)) {
    TLS_DESTRUCTORS.borrow_mut().push(TlsObjectDestructor { object_ptr, dtor });
}


//////////////////////////////////////////////////////////////////////////////////////
//// Everything below here is a modified version of thread_local!() from Rust std ////
//////////////////////////////////////////////////////////////////////////////////////

use core::cell::{Cell, UnsafeCell};
use core::fmt;
#[doc(hidden)]
pub use core::option;
use core::mem;
use core::hint;
use alloc::vec::Vec;

/// A thread-local storage key which owns its contents.
///
/// This TLS object is instantiated the [`thread_local!`] macro and offers 
/// one primary method to access it: the [`with`] method.
///
/// The [`with`] method yields a reference to the contained value which cannot be
/// sent across threads or escape the given closure.
///
/// # Initialization and Destruction
///
/// Initialization is lazily performed dynamically on the first call to [`with`]
/// within a thread (`Task` in Theseus), and values that implement [`Drop`] get destructed
/// when a thread exits.
///
/// A `LocalKey`'s initializer cannot recursively depend on itself, and using
/// a `LocalKey` in this way will cause the initializer to infinitely recurse
/// on the first call to `with`.
///
/// # Examples
///
/// ```ignore
/// use core::cell::RefCell;
/// use spawn::new_task_builder;
///
/// thread_local!(static FOO: RefCell<u32> = RefCell::new(1));
///
/// FOO.with(|f| {
///     assert_eq!(*f.borrow(), 1);
///     *f.borrow_mut() = 2;
/// });
///
/// // each thread starts out with the initial value of 1
/// let t = new_task_builder(
///     move |_: ()| {
///         FOO.with(|f| {
///             assert_eq!(*f.borrow(), 1);
///             *f.borrow_mut() = 3;
///         });
///     },
///     (), // empty arg
/// ).spawn().unwrap();
///
/// // wait for the new task to exit
/// t.join();
///
/// // we retain our original value of 2 despite the child thread
/// FOO.with(|f| {
///     assert_eq!(*f.borrow(), 2);
/// });
/// ```
///
/// [`with`]: LocalKey::with
pub struct LocalKey<T: 'static> {
    // This outer `LocalKey<T>` type is what's going to be stored in statics,
    // but actual data inside will be tagged with #[thread_local].
    // It's not valid for a true static to reference a #[thread_local] static,
    // so we get around that by exposing an accessor through a layer of function
    // indirection (this thunk).
    //
    // Note that the thunk is itself unsafe because the returned lifetime of the
    // slot where data lives, `'static`, is not actually valid. The lifetime
    // here is actually slightly shorter than the currently running thread!
    //
    // Although this is an extra layer of indirection, it should in theory be
    // trivially devirtualizable by LLVM because the value of `inner` never
    // changes and the constant should be readonly within a crate. This mainly
    // only runs into problems when TLS statics are exported across crates.
    inner: unsafe fn() -> Option<&'static T>,
}

impl<T: 'static> fmt::Debug for LocalKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalKey").finish_non_exhaustive()
    }
}

/// Declare a new thread local storage key of type [`LocalKey`].
///
/// # Syntax
///
/// The macro wraps any number of static declarations and makes them thread local.
/// Publicity and attributes for each static are allowed. Example:
///
/// ```
/// use core::cell::RefCell;
/// thread_local! {
///     pub static FOO: RefCell<u32> = RefCell::new(1);
///
///     #[allow(unused)]
///     static BAR: RefCell<f32> = RefCell::new(1.0);
/// }
/// # fn main() {}
/// ```
///
/// See [`LocalKey`] documentation for more information.
#[macro_export]
macro_rules! thread_local {
    // empty (base case for the recursion)
    () => {};

    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty = const { $init:expr }; $($rest:tt)*) => (
        $crate::__thread_local_inner!($(#[$attr])* $vis $name, $t, const $init);
        $crate::thread_local!($($rest)*);
    );

    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty = const { $init:expr }) => (
        $crate::__thread_local_inner!($(#[$attr])* $vis $name, $t, const $init);
    );

    // process multiple declarations
    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty = $init:expr; $($rest:tt)*) => (
        $crate::__thread_local_inner!($(#[$attr])* $vis $name, $t, $init);
        $crate::thread_local!($($rest)*);
    );

    // handle a single declaration
    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty = $init:expr) => (
        $crate::__thread_local_inner!($(#[$attr])* $vis $name, $t, $init);
    );
}

#[doc(hidden)]
#[macro_export]
// This allows the `thread_local!()` macro to be used in a foreign crate
// without that crate having to specify the `#![feature(thread_local)]`.
// This is also done in the Rust standard library, so it's completely fine to do.
#[allow_internal_unstable(thread_local)]
macro_rules! __thread_local_inner {
    // used to generate the `LocalKey` value for const-initialized thread locals
    (@key $t:ty, const $init:expr) => {{
        #[inline]
        unsafe fn __getit() -> $crate::option::Option<&'static $t> {

            // Theseus supports `#[thread_local]`, so use it directly.
            {
                // If a dtor isn't needed we can do something "very raw" and
                // just get going.
                if !$crate::mem::needs_drop::<$t>() {
                    #[thread_local]
                    static mut VAL: $t = $init;
                    unsafe {
                        return Some(&VAL)
                    }
                }

                #[thread_local]
                static mut VAL: $t = $init;
                // 0 == dtor not registered
                // 1 == dtor registered, dtor not run
                // 2 == dtor registered and is running or has run
                #[thread_local]
                static mut STATE: u8 = 0;

                unsafe extern "C" fn destroy(ptr: *mut u8) {
                    let ptr = ptr as *mut $t;

                    unsafe {
                        assert_eq!(STATE, 1);
                        STATE = 2;
                        $crate::ptr::drop_in_place(ptr);
                    }
                }

                unsafe {
                    match STATE {
                        // 0 == we haven't registered a destructor, so do
                        //   so now.
                        0 => {
                            $crate::fast::Key::<$t>::register_dtor(
                                $crate::ptr::addr_of_mut!(VAL) as *mut u8,
                                destroy,
                            );
                            STATE = 1;
                            Some(&VAL)
                        }
                        // 1 == the destructor is registered and the value
                        //   is valid, so return the pointer.
                        1 => Some(&VAL),
                        // otherwise the destructor has already run, so we
                        // can't give access.
                        _ => None,
                    }
                }
            }
        }

        unsafe {
            $crate::LocalKey::new(__getit)
        }
    }};

    // used to generate the `LocalKey` value for `thread_local!`
    (@key $t:ty, $init:expr) => {
        {
            #[inline]
            fn __init() -> $t { $init }

            #[inline]
            unsafe fn __getit() -> $crate::option::Option<&'static $t> {
                #[thread_local]
                static __KEY: $crate::fast::Key<$t> =
                    $crate::fast::Key::new();

                // FIXME: remove the #[allow(...)] marker when macros don't
                // raise warning for missing/extraneous unsafe blocks anymore.
                // See https://github.com/rust-lang/rust/issues/74838.
                #[allow(unused_unsafe)]
                unsafe { __KEY.get(__init) }
            }

            unsafe {
                $crate::LocalKey::new(__getit)
            }
        }
    };
    ($(#[$attr:meta])* $vis:vis $name:ident, $t:ty, $($init:tt)*) => {
        $(#[$attr])* $vis const $name: $crate::LocalKey<$t> =
            $crate::__thread_local_inner!(@key $t, $($init)*);
    }
}

/// An error returned by [`LocalKey::try_with`](struct.LocalKey.html#method.try_with).
#[non_exhaustive]
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AccessError;

impl fmt::Debug for AccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessError").finish()
    }
}

impl fmt::Display for AccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt("already destroyed", f)
    }
}

// The `Error` trait is not in the core library yet, so we can't use it.
// impl Error for AccessError {}

impl<T: 'static> LocalKey<T> {
    #[doc(hidden)]
    pub const unsafe fn new(inner: unsafe fn() -> Option<&'static T>) -> LocalKey<T> {
        LocalKey { inner }
    }

    /// Acquires a reference to the value in this TLS key.
    ///
    /// This will lazily initialize the value if this thread has not referenced
    /// this key yet.
    ///
    /// # Panics
    ///
    /// This function will `panic!()` if the key currently has its
    /// destructor running, and it **may** panic if the destructor has
    /// previously been run for this thread.
    pub fn with<F, R>(&'static self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        self.try_with(f).expect(
            "cannot access a Thread Local Storage value \
             during or after destruction",
        )
    }

    /// Acquires a reference to the value in this TLS key.
    ///
    /// This will lazily initialize the value if this thread has not referenced
    /// this key yet. If the key has been destroyed (which may happen if this is called
    /// in a destructor), this function will return an [`AccessError`].
    ///
    /// # Panics
    ///
    /// This function will still `panic!()` if the key is uninitialized and the
    /// key's initializer panics.
    #[inline]
    pub fn try_with<F, R>(&'static self, f: F) -> Result<R, AccessError>
    where
        F: FnOnce(&T) -> R,
    {
        unsafe {
            let thread_local = (self.inner)().ok_or(AccessError)?;
            Ok(f(thread_local))
        }
    }
}

mod lazy {
    use crate::UnsafeCell;
    use crate::hint;
    use crate::mem;

    pub struct LazyKeyInner<T> {
        inner: UnsafeCell<Option<T>>,
    }

    impl<T> LazyKeyInner<T> {
        pub const fn new() -> LazyKeyInner<T> {
            LazyKeyInner { inner: UnsafeCell::new(None) }
        }

        pub unsafe fn get(&self) -> Option<&'static T> {
            // SAFETY: The caller must ensure no reference is ever handed out to
            // the inner cell nor mutable reference to the Option<T> inside said
            // cell. This make it safe to hand a reference, though the lifetime
            // of 'static is itself unsafe, making the get method unsafe.
            unsafe { (*self.inner.get()).as_ref() }
        }

        /// The caller must ensure that no reference is active: this method
        /// needs unique access.
        pub unsafe fn initialize<F: FnOnce() -> T>(&self, init: F) -> &'static T {
            // Execute the initialization up front, *then* move it into our slot,
            // just in case initialization fails.
            let value = init();
            let ptr = self.inner.get();

            // SAFETY:
            //
            // note that this can in theory just be `*ptr = Some(value)`, but due to
            // the compiler will currently codegen that pattern with something like:
            //
            //      ptr::drop_in_place(ptr)
            //      ptr::write(ptr, Some(value))
            //
            // Due to this pattern it's possible for the destructor of the value in
            // `ptr` (e.g., if this is being recursively initialized) to re-access
            // TLS, in which case there will be a `&` and `&mut` pointer to the same
            // value (an aliasing violation). To avoid setting the "I'm running a
            // destructor" flag we just use `mem::replace` which should sequence the
            // operations a little differently and make this safe to call.
            //
            // The precondition also ensures that we are the only one accessing
            // `self` at the moment so replacing is fine.
            unsafe {
                let _ = mem::replace(&mut *ptr, Some(value));
            }

            // SAFETY: With the call to `mem::replace` it is guaranteed there is
            // a `Some` behind `ptr`, not a `None` so `unreachable_unchecked`
            // will never be reached.
            unsafe {
                // After storing `Some` we want to get a reference to the contents of
                // what we just stored. While we could use `unwrap` here and it should
                // always work it empirically doesn't seem to always get optimized away,
                // which means that using something like `try_with` can pull in
                // panicking code and cause a large size bloat.
                match *ptr {
                    Some(ref x) => x,
                    None => hint::unreachable_unchecked(),
                }
            }
        }

        /// The other methods hand out references while taking &self.
        /// As such, callers of this method must ensure no `&` and `&mut` are
        /// available and used at the same time.
        #[allow(unused)]
        pub unsafe fn take(&mut self) -> Option<T> {
            // SAFETY: See doc comment for this method.
            unsafe { (*self.inner.get()).take() }
        }
    }
}

#[doc(hidden)]
pub mod fast {
    use super::lazy::LazyKeyInner;
    use crate::Cell;
    use crate::fmt;
    use crate::mem;
    use super::register_dtor;

    #[derive(Copy, Clone)]
    enum DtorState {
        Unregistered,
        Registered,
        RunningOrHasRun,
    }

    // This data structure has been carefully constructed so that the fast path
    // only contains one branch on x86. That optimization is necessary to avoid
    // duplicated tls lookups on OSX.
    //
    // LLVM issue: https://bugs.llvm.org/show_bug.cgi?id=41722
    pub struct Key<T> {
        // If `LazyKeyInner::get` returns `None`, that indicates either:
        //   * The value has never been initialized
        //   * The value is being recursively initialized
        //   * The value has already been destroyed or is being destroyed
        // To determine which kind of `None`, check `dtor_state`.
        //
        // This is very optimizer friendly for the fast path - initialized but
        // not yet dropped.
        inner: LazyKeyInner<T>,

        // Metadata to keep track of the state of the destructor. Remember that
        // this variable is thread-local, not global.
        dtor_state: Cell<DtorState>,
    }

    impl<T> fmt::Debug for Key<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Key").finish_non_exhaustive()
        }
    }

    impl<T> Key<T> {
        pub const fn new() -> Key<T> {
            Key { inner: LazyKeyInner::new(), dtor_state: Cell::new(DtorState::Unregistered) }
        }

        /*
         * Theseus Note: I'm not sure why this exists, but I don't think we need it in Theseus.
         *
        // note that this is just a publically-callable function only for the
        // const-initialized form of thread locals, basically a way to call the
        // free `register_dtor` function defined elsewhere in libstd.
        pub unsafe fn register_dtor(a: *mut u8, dtor: unsafe extern "C" fn(*mut u8)) {
            unsafe {
                register_dtor(a, dtor);
            }
        }
        */

        pub unsafe fn get<F: FnOnce() -> T>(&self, init: F) -> Option<&'static T> {
            // SAFETY: See the definitions of `LazyKeyInner::get` and
            // `try_initialize` for more informations.
            //
            // The caller must ensure no mutable references are ever active to
            // the inner cell or the inner T when this is called.
            // The `try_initialize` is dependant on the passed `init` function
            // for this.
            unsafe {
                match self.inner.get() {
                    Some(val) => Some(val),
                    None => self.try_initialize(init),
                }
            }
        }

        // `try_initialize` is only called once per fast thread local variable,
        // except in corner cases where thread_local dtors reference other
        // thread_local's, or it is being recursively initialized.
        //
        // Macos: Inlining this function can cause two `tlv_get_addr` calls to
        // be performed for every call to `Key::get`.
        // LLVM issue: https://bugs.llvm.org/show_bug.cgi?id=41722
        #[inline(never)]
        unsafe fn try_initialize<F: FnOnce() -> T>(&self, init: F) -> Option<&'static T> {
            // SAFETY: See comment above (this function doc).
            if !mem::needs_drop::<T>() || unsafe { self.try_register_dtor() } {
                // SAFETY: See comment above (his function doc).
                Some(unsafe { self.inner.initialize(init) })
            } else {
                None
            }
        }

        // `try_register_dtor` is only called once per fast thread local
        // variable, except in corner cases where thread_local dtors reference
        // other thread_local's, or it is being recursively initialized.
        unsafe fn try_register_dtor(&self) -> bool {
            match self.dtor_state.get() {
                DtorState::Unregistered => {
                    // SAFETY: dtor registration happens before initialization.
                    // Passing `self` as a pointer while using `destroy_value<T>`
                    // is safe because the function will build a pointer to a
                    // Key<T>, which is the type of self and so find the correct
                    // size.
                    unsafe { register_dtor(self as *const _ as *mut u8, destroy_value::<T>) };
                    self.dtor_state.set(DtorState::Registered);
                    true
                }
                DtorState::Registered => {
                    // recursively initialized
                    true
                }
                DtorState::RunningOrHasRun => false,
            }
        }
    }

    unsafe extern "C" fn destroy_value<T>(ptr: *mut u8) {
        let ptr = ptr as *mut Key<T>;

        // SAFETY:
        //
        // The pointer `ptr` has been built just above and comes from
        // `try_register_dtor` where it is originally a Key<T> coming from `self`,
        // making it non-NUL and of the correct type.
        //
        // Right before we run the user destructor be sure to set the
        // `Option<T>` to `None`, and `dtor_state` to `RunningOrHasRun`. This
        // causes future calls to `get` to run `try_initialize_drop` again,
        // which will now fail, and return `None`.
        unsafe {
            let value = (*ptr).inner.take();
            (*ptr).dtor_state.set(DtorState::RunningOrHasRun);
            drop(value);
        }
    }
}
