//!This application display in 3d and 2D modes with personality

#![no_std]
#[warn(stable_features)] 

extern crate display;
extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate spin;
extern crate mod_mgmt;
extern crate memory;
extern crate fs_node;
extern crate frame_buffer;

use display::{Display};
use frame_buffer::FrameBuffer;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::sync::Arc;
use mod_mgmt::{CrateNamespace, get_default_namespace, get_namespaces_directory, NamespaceDirectorySet};
use spin::{Mutex};
use fs_node::FileOrDir; 

type FillRectangleFun = fn(&Arc<Mutex<FrameBuffer>>, usize, usize, usize, usize, u32);

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    let vf = match FrameBuffer::new(200,700, None){
        Ok(vf) => {vf},
        Err(err) => {
            println!("{}", err);
            return -1
        },
    };


    //let mut vf = frame_buffer::map(700, 100, vf).unwrap();
    //vf.lock().fill_rectangle(0,100,100,100, 0xff);
    
    // match frame_buffer::display(&vf) {
    //     Ok(_) => {},
    //     Err(err) => {
    //         println!("{}", err);
    //         return -1;
    //     }
    // }

    //Display two rectangles in two modes with personality
    // match personality(&mut vf) {
    //     Ok(_) => {return 0},
    //     Err(err) => {
    //         println!("{}", err);
    //         return -1;
    //     }
    // }

	return 0;
}

fn personality(vf: &mut Arc<Mutex<FrameBuffer>>) -> Result<(), &'static str> {
    let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or_else(|| "couldn't get kernel mmi")?;
	let backup_namespace = get_default_namespace().ok_or("default crate namespace wasn't yet initialized")?;

    //Create a 3D namespace
    let namespace_name = "default";
    let namespaces_dir = get_namespaces_directory().ok_or("top-level namespaces directory wasn't yet initialized")?;
	    
    let base_dir = match namespaces_dir.lock().get(namespace_name) {
		Some(FileOrDir::Dir(d)) => d,
		_ => return Err("couldn't find directory at given path"),
	};
	let mut namespace_3d = CrateNamespace::new(
		String::from(namespace_name), 
		NamespaceDirectorySet::from_existing_base_dir(base_dir).map_err(|e| {
			error!("Couldn't find expected namespace directory {:?}", namespace_name);
			e
		})?,
	);

	//Get display functions in the 3D namespace
	let display_3d_file = namespace_3d.get_kernel_file_starting_with("display_3d-")
		.ok_or_else(|| "couldn't find a single 'display_3d' object file in simd_personality")?;
	namespace_3d.enable_fuzzy_symbol_matching();
	namespace_3d.load_kernel_crate(&display_3d_file, Some(backup_namespace), &kernel_mmi_ref, false)?;
	namespace_3d.disable_fuzzy_symbol_matching();

	let fill_rec_3d_ref = namespace_3d.get_symbol_starting_with("display_3d::fill_rectangle")
		.upgrade()
		.ok_or("no single symbol matching \"display_3d::fill_rectangle\"")?;
	let mut space_3d = 0;	
	let (mapped_pages_3d, mapped_pages_offset_3d) = { 
		let section = fill_rec_3d_ref.lock();
		(section.mapped_pages.clone(), section.mapped_pages_offset)
	};
	let fill_rectangle_3d: &FillRectangleFun = mapped_pages_3d.lock().as_func(mapped_pages_offset_3d, &mut space_3d)?;

	//Get display functions in the 2D namespace
	let fill_rec_2d_ref = backup_namespace.get_symbol_starting_with("display::fill_rectangle")
		.upgrade()
		.ok_or("no single symbol matching \"display::fill_rectangle\"")?;
	let mut space_2d = 0;	
	let (mapped_pages_2d, mapped_pages_offset_2d) = { 
		let section = fill_rec_2d_ref.lock();
		(section.mapped_pages.clone(), section.mapped_pages_offset)
	};
	let fill_rectangle_2d: &FillRectangleFun = mapped_pages_2d.lock().as_func(mapped_pages_offset_2d, &mut space_2d)?;

	//Display in both modes
    fill_rectangle_3d(vf, 0,300,100,100, 0xffff00);
    //frame_buffer::display(&vf)?;

    fill_rectangle_2d(vf, 0,500,100,100, 0x00ff00);
    //frame_buffer::display(&vf)?;
  

    Ok(())
}