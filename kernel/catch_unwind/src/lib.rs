//! Support for catching a panic while a panicked `Task` is being unwound.
//! 
#![no_std]
#![feature(core_intrinsics)]

extern crate alloc; 
extern crate task;

use core::mem::ManuallyDrop;
use alloc::boxed::Box;
use task::KillReason;

/// Invokes the given closure `f`, catching a panic as it is unwinding the stack.
/// 
/// Returns `Ok(R)` if the closure `f` executes and returns successfully,
/// otherwise returns `Err(cause)` if the closure panics, where `cause` is the original cause of the panic. 
///  
/// This function behaves similarly to the libstd version, 
/// so please see its documentation here: <https://doc.rust-lang.org/std/panic/fn.catch_unwind.html>.
pub fn catch_unwind_with_arg<F, A, R>(f: F, arg: A) -> Result<R, KillReason>
    where F: FnOnce(A) -> R,
{
    // The `try()` intrinsic accepts only one "data" pointer as its only argument.
    let mut ti_arg = TryIntrinsicArg {
        func: ManuallyDrop::new(f),
        arg:  ManuallyDrop::new(arg),
        // The initial value of `ret` doesn't matter. It will get replaced in all code paths. 
        ret:  ManuallyDrop::new(Err(KillReason::Exception(0))),
    };

    // Invoke the actual try() intrinsic, which will jump to `call_func_with_arg`
    let _try_val = unsafe { 
        core::intrinsics::r#try(
            try_intrinsic_trampoline::<F, A, R>,
            &mut ti_arg as *mut _ as *mut u8,
            panic_callback::<F, A, R>,
        )
    };

    // When `try` returns zero, it means the function ran successfully without panicking.
    // The `Ok(R)` value was assigned to `ret` at the end of `try_intrinsic_trampoline()` below.
    // When `try` returns non-zero, it means the function panicked.
    // The `panic_callback()` would have already been invoked, and it would have set `ret` to `Err(KillReason)`.
    //
    // In both cases, we can just return the value `ret` field, which has been assigned the proper value.
    ManuallyDrop::into_inner(ti_arg.ret)
}


/// This function will be automatically invoked by the `try` intrinsic above
/// upon catching a panic. 
/// # Arguments
/// * a pointer to the 
/// * a pointer to the arbitrary object passed around during the unwinding process,
///   which in Theseus is a pointer to the `UnwindingContext`. 
fn panic_callback<F, A, R>(data_ptr: *mut u8, exception_object: *mut u8) where F: FnOnce(A) -> R {
    let data = unsafe { &mut *(data_ptr as *mut TryIntrinsicArg<F, A, R>) };
    let unwinding_context_boxed = unsafe { Box::from_raw(exception_object as *mut unwind::UnwindingContext) };
    let unwinding_context = *unwinding_context_boxed;
    let (_stack_frame_iter, cause, _taskref) = unwinding_context.into();
    data.ret = ManuallyDrop::new(Err(cause));
}


/// A struct to accommodate the weird signature of `core::intrinsics::try`, 
/// which accepts only a single pointer to this structure. 
/// We model this after Rust libstd's wrappers around gcc-based unwinding, but modify it to contain one argument. 
struct TryIntrinsicArg<F, A, R> where F: FnOnce(A) -> R {
    /// The function that will be invoked in the `try()` intrinsic.
    func: ManuallyDrop<F>,
    /// The argument that will be passed into the above function.
    arg:  ManuallyDrop<A>,
    /// The return value of the above function, which is an output parameter. 
    /// Note that this is only filled in by the `try()` intrinsic if the function returns successfully.
    ret:  ManuallyDrop<Result<R, KillReason>>,
}

/// This is the function that the `try()` intrinsic will jump to. 
/// Since that intrinsic requires a `fn` ptr, we can't just directly call a closure `F` here because it's a `FnOnce` trait.
/// 
/// This function should not be called directly in our code. 
fn try_intrinsic_trampoline<F, A, R>(try_intrinsic_arg: *mut u8) where F: FnOnce(A) -> R {
    unsafe {
        let data = try_intrinsic_arg as *mut TryIntrinsicArg<F, A, R>;
        let data = &mut *data;
        let f = ManuallyDrop::take(&mut data.func);
        let a = ManuallyDrop::take(&mut data.arg);
        data.ret = ManuallyDrop::new(
            Ok(f(a)) // actually invoke the function
        );
    }
}


/// Resumes the unwinding procedure after it was caught with [`catch_unwind_with_arg()`].
/// 
/// This is analogous to the Rust's [`std::panic::resume_unwind()`] in that it is
/// intended to be used to continue unwinding after a panic was caught.
/// 
/// The argument is a [`KillReason`] instead of a typical Rust panic "payload"
/// (which is usually `Box<Any + Send>`) for two reasons:
/// 1. `KillReason` is the type returned by [`catch_unwind_with_arg()`] upon failure, 
///    so it makes sense to continue unwinding with that same error type.
/// 2. It's more flexible than the standard Rust panic info type because it must also
///    represent the possibility of a non-panic failure, e.g., a machine exception.
/// 
/// [`std::panic::resume_unwind()`]: https://doc.rust-lang.org/std/panic/fn.resume_unwind.html
pub fn resume_unwind(caught_panic_reason: KillReason) -> ! {
    // We can skip up to 2 frames here: `unwind::start_unwinding` and `resume_unwind` (this function)
    let result = unwind::start_unwinding(caught_panic_reason, 2);

    // `start_unwinding` should not return
    panic!("BUG: start_unwinding() returned {:?}. This is an unexpected failure, as no unwinding occurred. Task: {:?}.",
        result,
        task::get_my_current_task()
    );
}
