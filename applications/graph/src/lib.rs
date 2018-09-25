#![no_std]
#![feature(alloc)]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;
#[macro_use] extern crate log;

extern crate memory;
extern crate mod_mgmt;
extern crate frame_buffer_display;
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
    let round = match _args.get(0) {
        Some(round) => {
            match round.parse::<usize>() {
                Ok(i) => i,
                Err(_) => {
                    println!("The round parameter should be a positive interger.\n
                        Try graph <round:usize> <color:u32>");
                    return -1;
                }
            }
        },
        None => {
            println!("Missing the round parameter.\nTry graph <round:usize> <color:u32>");
            return -1;
        }
    };
    
    let color = match _args.get(0) {
        Some(color) => {
            match color.parse::<usize>() {
                Ok(i) => i,
                Err(_) => {
                    println!("The round parameter should be a positive interger.\n
                        Try graph <round:usize> <color:u32>");
                    return -1;
                }
            }
        },
        None => {
            println!("Missing the round parameter.\nTry graph <round:usize> <color:u32>");
            return -1
        }
    };

    let (width, height) = frame_buffer_display::get_resolution();

    let hpet_lock = acpi::get_hpet();
    let STARTING_TIME = unsafe { hpet_lock.as_ref().unwrap().get_counter()};
    frame_buffer_display::fill_rectangle(0, 0, width, height, 0xe3b4b8);
    //render(round, color);
    unsafe {
        let hpet_lock = acpi::get_hpet();
        let TIME = hpet_lock.as_ref().unwrap().get_counter() - STARTING_TIME;
        println!("3D: {}", TIME);
    }

    let hpet_lock = acpi::get_hpet();
    let STARTING_TIME = unsafe { hpet_lock.as_ref().unwrap().get_counter()};
    match swap_frame_buffer("frame_buffer_display_2d"){
        Ok(_) =>{},
        Err(_) =>{ return -2; }
    };
//    render(round, color);
    frame_buffer_display::fill_rectangle(0, 0, width, height, 0xe3b4b8);
    unsafe {
        let hpet_lock = acpi::get_hpet();
        let TIME = hpet_lock.as_ref().unwrap().get_counter() - STARTING_TIME; 
        println!("SWAP+2D: {}", TIME);
    }

    match swap_frame_buffer("frame_buffer_display"){
        Ok(_) =>{},
        Err(_) =>{ println!("Fail to recover the display mode"); return -2; }
    };
    0
}

fn swap_frame_buffer(new_module:&str) -> Result<(), &'static str>{
    let mut swap_pairs:Vec<(StrongCrateRef, &ModuleArea, Option<String>)> = Vec::with_capacity(1);
    swap_pairs.push(
        (
            match mod_mgmt::get_default_namespace().get_crate("frame_buffer_display"){
                Some(old_crate) => {old_crate},
                None => {
                    println!("Fail to get the old frame_buffer_display crate");
                    return Err("Fail to get the new module");
                } 
            },
            match get_module(&format!("k#{}", new_module)){
                Some(new_module) => {new_module},
                None => {
                    println!("Fail to get the new {} module", new_module);
                    return Err("Fail to get the new module");
                }

            },
            Some(String::from("frame_buffer_display"))
        )
    );


    let kernel_mmi_ref = memory::get_kernel_mmi_ref().unwrap();
    let mut kernel_mmi = kernel_mmi_ref.lock();

    let rs = mod_mgmt::get_default_namespace().swap_crates(
        swap_pairs, 
        kernel_mmi.deref_mut(), 
        false
    );
    
    return rs;
}   

fn render(round:usize, color_max:u32) {
    let size = 600;
    for i in 0..round {
        let mut color:u32 = 0x0000FF;

        while color < 0xFF00 {
            frame_buffer_display::fill_rectangle(200, 100, size, size, color);
            color = color - 1 + 0x000100;
        }
        //size += 50;
    }
            //frame_buffer::fill_rectangle(200, 100, size, size, color);
}
