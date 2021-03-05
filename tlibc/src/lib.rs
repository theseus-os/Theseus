//! A libc implementation that is targeted for and runs atop Theseus. 

#![no_std]
#![feature(ptr_internals)]
#![feature(c_variadic)]
#![feature(untagged_unions)]


// Allowances for C-style syntax.
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]


extern crate alloc;
#[macro_use] extern crate log;
extern crate libc; // for C types
extern crate memory;
extern crate task;
extern crate cstr_core;
extern crate bare_io;


mod errno;
mod types;
mod io;
mod globals;
mod printf;


use alloc::vec::Vec;
use cstr_core::CString;


pub use errno::*;
pub use types::*;



#[no_mangle]
pub fn _start(args: &[&str], env: &[&str]) -> c_int {
    debug!("args: {:?}\n env: {:?}", args, env);
    let (args_cstrings, args_char_ptrs) = to_cstring_vec(args);
    let (env_cstrings,  env_char_ptrs)  = to_cstring_vec(env);

    // set the global pointers to the args and the environment
    let args_ptr = args_char_ptrs.as_mut_ptr();
    let env_ptr  = env_char_ptrs .as_mut_ptr();
    unsafe {
        globals::argv          = args_ptr;
        globals::inner_argv    = args_char_ptrs;
        globals::environ       = env_ptr;
        globals::inner_environ = env_char_ptrs;
    }

    let retval: c_int = unsafe {
        main(args.len() as isize, args_ptr, env_ptr)
    };

    debug!("main returned {:?}", retval);

    retval
}


fn to_cstring_vec(slice_of_strs: &[&str]) -> (Vec<CString>, Vec<*mut c_char>) {
    let mut cstrings = Vec::with_capacity(slice_of_strs.len()); 
    cstrings.extend(slice_of_strs.iter().filter_map(|&s| CString::new(s).ok()));
    let mut cstr_ptrs = Vec::with_capacity(cstrings.len());
    cstr_ptrs.extend(cstrings.iter().map(|c| c.as_ptr() as *mut _));
    (cstrings, cstr_ptrs)
}


extern "C" {
    fn main(argc: isize, argv: *mut *mut c_char, envp: *mut *mut c_char) -> c_int;
}
