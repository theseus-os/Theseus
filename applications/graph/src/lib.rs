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
    
/*    let color = match _args.get(0) {
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
*/
    let timeslice = 41666667/round as u64;
    let (width, height) = frame_buffer_display::get_resolution();

    //println!("The screen is {}*{}", width, height);

    let mut mode_3d = true;

    let hpet_lock = acpi::get_hpet();
    let mut STARTING_TIME = unsafe { hpet_lock.as_ref().unwrap().get_counter()};
    let mut END_TIME = STARTING_TIME;

    let mut color_2d = 0x00FF;
    let mut color_3d = 0x00FF;

    let START = STARTING_TIME;
    while (color_2d <= 0xFF00 || color_3d <= 0xFF00) {
        
        if (color_2d <= 0xFF00 && color_3d <= 0xFF00 ) {
            let END_TIME = match hpet_lock.as_ref() {
                Some(hpet) => {
                    hpet.get_counter()
                },
                None => {
                    return -2;
                }
            };
            
            if (END_TIME - STARTING_TIME > timeslice) {
                STARTING_TIME = END_TIME;
                mode_3d = !mode_3d;
                if mode_3d {
                    match swap_frame_buffer("frame_buffer_display"){
                        Ok(_) =>{},
                        Err(_) =>{ return -2; }
                    };
                } else {
                    match swap_frame_buffer("frame_buffer_display_2d"){
                        Ok(_) =>{},
                        Err(_) =>{ return -2; }
                    };
                }
            }
        }

        if mode_3d {
            frame_buffer_display::fill_rectangle(100, 500, 800, 800, color_3d);
            color_3d += 0x100 - 1;
            if color_3d > 0xFF00 {
                frame_buffer_display::fill_rectangle(100, 500, 800, 800, 0xFF0000);
                mode_3d = !mode_3d;
                match swap_frame_buffer("frame_buffer_display_2d"){
                    Ok(_) =>{},
                    Err(_) =>{ return -2;}
                }
            }
        } else {
            frame_buffer_display::fill_rectangle(1000, 500, 800, 800, color_2d);
            color_2d += 0x100 - 1;
            if color_2d > 0xFF00 {
                frame_buffer_display::fill_rectangle(1000, 500, 800, 800, 0xFF0000);
                mode_3d = !mode_3d;
                match swap_frame_buffer("frame_buffer_display"){
                    Ok(_) =>{},
                    Err(_) =>{ return -2;}
                }
            }
        }
    }

    let END_TIME = match hpet_lock.as_ref() {
        Some(hpet) => {
            hpet.get_counter()
        },
        None => {
            return -2;
        }
    };

    println!("SWAP: The time is {}", END_TIME - START);

//====================================

    match swap_frame_buffer("frame_buffer_display"){
        Ok(_) =>{},
        Err(_) =>{ return -2; }
    };

    let mut mode_3d = true;

    let hpet_lock = acpi::get_hpet();
    let mut STARTING_TIME = unsafe { hpet_lock.as_ref().unwrap().get_counter()};
    let mut END_TIME = STARTING_TIME;

    let mut color_2d = 0x00FF;
    let mut color_3d = 0x00FF;

    let START = STARTING_TIME;
    while (color_2d <= 0xFF00 || color_3d <= 0xFF00) {
        
        if (color_2d <= 0xFF00 && color_3d <= 0xFF00 ) {
            let END_TIME = match hpet_lock.as_ref() {
                Some(hpet) => {
                    hpet.get_counter()
                },
                None => {
                    return -2;
                }
            };
            
            if (END_TIME - STARTING_TIME > timeslice) {
                STARTING_TIME = END_TIME;
                mode_3d = !mode_3d;
            }
        }

        if mode_3d {
            frame_buffer_display::fill_rectangle(100, 500, 800, 800, color_3d);
            color_3d += 0x100 - 1;
            if color_3d > 0xFF00 {
                frame_buffer_display::fill_rectangle(100, 500, 800, 800, 0xFF0000);
                mode_3d = !mode_3d;
            }
        } else {
            frame_buffer_display::fill_rectangle(1000, 500, 800, 800, color_2d);
            color_2d += 0x100 - 1;
            if color_2d > 0xFF00 {
                frame_buffer_display::fill_rectangle(1000, 500, 800, 800, 0xFF0000);
                mode_3d = !mode_3d;
            }
        }
    }

    let END_TIME = match hpet_lock.as_ref() {
        Some(hpet) => {
            hpet.get_counter()
        },
        None => {
            return -2;
        }
    };

    println!("SWAP: The time is {}", END_TIME - START); 
 
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

fn render(round:usize, width:usize, height:usize) {
    //let size = 600;
    let mut color = 0x00FF;
    for i in 0..round {
        //let mut color:u32 = 0x0000FF;

        //while color < 0xFF00 {
            frame_buffer_display::fill_rectangle(0, 0, width, height, color);
            color = color - 1 + 0x000100;
        //}
        //size += 50;
    }
            //frame_buffer::fill_rectangle(200, 100, size, size, color);
}
