//! Support for catching a panic while a panicked `Task` is being unwound.
//! 
#![no_std]
#![feature(core_intrinsics)]
#![feature(manually_drop_take)]

extern crate alloc; 
extern crate task;

use core::mem::ManuallyDrop;

/// Invokes the given closure `f`, catching a panic as it is unwinding the stack.
/// 
/// Returns `Ok(R)` if the closure `f` executes and returns successfully,
/// otherwise returns `Err(cause)` if the closure panics, where `cause` is the original cause of the panic. 
///  
/// This function behaves similarly to the libstd version, 
/// so please see its documentation here: <https://doc.rust-lang.org/std/panic/fn.catch_unwind.html>.
pub fn catch_unwind_with_arg<F, A, R>(f: F, arg: A) -> Result<R, task::KillReason>
    where F: FnOnce(A) -> R,
{
    // The `try()` intrinsic accepts this as its only argument 
    let mut ti_arg = TryIntrinsicArg {
        func: ManuallyDrop::new(f),
        arg:  ManuallyDrop::new(arg),
        ret:  unsafe { core::mem::MaybeUninit::uninit().assume_init() },
    };

    // The `try` intrinsic will set this to the pointer passed around in unwind routines,
    // which in Theseus is the pointer to the uwninding context.
    let mut unwind_context_ptr_out: usize = 0;

    // Invoke the actual try() intrinsic, which will jump to `call_func_with_arg`
    let try_val = unsafe { 
        core::intrinsics::r#try(
            try_intrinsic_trampoline::<F, A, R>,
            &mut ti_arg as *mut _ as *mut u8,
            &mut unwind_context_ptr_out as *mut _ as *mut u8,
        )
    };
    match try_val {
        // When `try` returns zero, it means the function ran successfully without panicking.
        0 => Ok(ManuallyDrop::into_inner(ti_arg.ret)),
        // When `try` returns non-zero, it means the function panicked.
        _ => {
            let cause = unsafe { 
                unwind::unwinding_context_ptr_into_cause(unwind_context_ptr_out as *mut unwind::UnwindingContext) };
            Err(cause)
        }
    }
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
    ret:  ManuallyDrop<R>,
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
            f(a) // actually invoke the function
        );
    }
}