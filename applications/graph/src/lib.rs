#![no_std]
#![feature(alloc)]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;
#[macro_use] extern crate log;

extern crate memory;
extern crate mod_mgmt;
extern crate frame_buffer;
extern crate acpi;

use core::ops::DerefMut;
use alloc::{Vec, String};
//use alloc::slice::SliceConcatExt;
use alloc::string::ToString;
use memory::{get_module, ModuleArea};
use mod_mgmt::metadata::StrongCrateRef;

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    //println!("Hello, world! Args: {:?}", _args);

    let mut swap_pairs:Vec<(StrongCrateRef, &ModuleArea, Option<String>)> = Vec::with_capacity(1);
    swap_pairs.push(
        (
            mod_mgmt::get_default_namespace().get_crate("frame_buffer").unwrap(),
            get_module("k#frame_buffer_3d").unwrap(),
            Some(String::from("frame_buffer"))
        )
    );

    {
        let kernel_mmi_ref = memory::get_kernel_mmi_ref().unwrap();
        let mut kernel_mmi = kernel_mmi_ref.lock();

        let rs = mod_mgmt::get_default_namespace().swap_crates(
            swap_pairs, 
            kernel_mmi.deref_mut(), 
            false
        ).map_err(|e| e.to_string());
    }


    let rs = frame_buffer::init();
    match rs {
        Ok(_) => {trace!("Wenqiu::the swapping is done");},
        Err(err) => {
            trace!("Wenqiu: the err is {}", err);
            return -2;
        }
    };

    /*let mut size = 100;
    while size <= 600 {
        let mut color:u32 = 0x0000FF;
        let hpet_lock = acpi::get_hpet();
        let STARTING_TIME = unsafe { hpet_lock.as_ref().unwrap().get_counter()};
        while color != 0x00FF00 {
            frame_buffer::fill_rectangle(200, 100, size, size, color);
            color = color - 1 + 0x000100;
        }
        frame_buffer::fill_rectangle(200, 100, size, size, color);
        let hpet_lock = acpi::get_hpet();
        unsafe { 
            let END_TIME = hpet_lock.as_ref().unwrap().get_counter() - STARTING_TIME; 
            trace!("{}", END_TIME);
        }
        size += 50;
    }*/

    0
}
