#![no_std]
#![feature(alloc)]

extern crate display;
#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate path;
extern crate spin;
extern crate mod_mgmt;
extern crate memory;
extern crate fs_node;

use display::{VirtualFrameBuffer, Display};
use alloc::vec::Vec;
use alloc::string::String;
use alloc::sync::Arc;
use mod_mgmt::{CrateNamespace, get_default_namespace, get_namespaces_directory, NamespaceDirectorySet};
use memory::{get_kernel_mmi_ref, MappedPages};
use spin::{Once, Mutex};
use path::Path;
use core::ops::DerefMut;
use fs_node::FileOrDir; 

type FillRectangleFun = fn(&Arc<Mutex<VirtualFrameBuffer>>, usize, usize, usize, usize, u32);


#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    let vf = match VirtualFrameBuffer::new(200,700){
        Ok(vf) => {vf},
        Err(err) => {return 1},
    };


    let mut vf = frame_buffer::map(700, 100, vf).unwrap();
    //vf.lock().fill_rectangle(0,0,100,100, 0xff0000);
    match frame_buffer::display(&vf) {
        Ok(_) => {},
        Err(err) => {
            println!("{}", err);
            return -1;
        }
    }

    match personality(&mut vf) {
        Ok(_) => {return 0},
        Err(err) => {
            println!("{}", err);
            return -1;
        }
    }

	// let frame_buffer_test = frame_buffer_namespace.get_kernel_file_starting_with("display_3d-").unwrap();
    
    // frame_buffer_namespace.enable_fuzzy_symbol_matching();
	// match frame_buffer_namespace.load_kernel_crate(&frame_buffer_test, Some(normal_namespace), kernel_mmi_ref.lock().deref_mut(), false){
    //     Ok(_) => {

    //     },
    //     Err(err) => {
    //         println!("Fail to load modules to new namespace: {}", err);
    //         return -1;
    //     }
    // }

	// frame_buffer_namespace.disable_fuzzy_symbol_matching();

    

    // let mut space = 0;
    // let (fill_rectangle_pages, offset) = match get_function_pages(&frame_buffer_namespace, "diaplay_3d::Display::fill_rectangle::"){
    //     Ok(pages) => {pages},
    //     Err(err) => {
    //         println!("couldn't find func fill_rectangle()");
    //         return -1;
    //     }
    // };
	// let fill_rectangle: &FillRectangleFun = match fill_rectangle_pages.lock().as_func(offset, &mut space){
    //     Ok(func) => {func},
    //     Err(err) => {            
    //         println!("Fail to transmute func fill_rectangle()");
    //         return -1;
    //     }
    // };

}

fn personality(vf: &mut Arc<Mutex<VirtualFrameBuffer>>) -> Result<(), &'static str> {
    let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or_else(|| "couldn't get kernel mmi")?;
	let backup_namespace = get_default_namespace().ok_or("default crate namespace wasn't yet initialized")?;

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

	// load the actual crate that we want to run in the simd namespace, "simd_test"
	let display_3d_file = namespace_3d.get_kernel_file_starting_with("display_3d-")
		.ok_or_else(|| "couldn't find a single 'simd_test' object file in simd_personality")?;
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

	let fill_rec_2d_ref = backup_namespace.get_symbol_starting_with("display::fill_rectangle")
		.upgrade()
		.ok_or("no single symbol matching \"display::fill_rectangle\"")?;
	let mut space_2d = 0;	
	let (mapped_pages_2d, mapped_pages_offset_2d) = { 
		let section = fill_rec_2d_ref.lock();
		(section.mapped_pages.clone(), section.mapped_pages_offset)
	};
	let fill_rectangle_2d: &FillRectangleFun = mapped_pages_2d.lock().as_func(mapped_pages_offset_2d, &mut space_2d)?;

    fill_rectangle_3d(vf, 0,300,100,100, 0xffff00);
    frame_buffer::display(&vf)?;

    fill_rectangle_2d(vf, 0,100,100,100, 0x00ff00);
    frame_buffer::display(&vf)?;
  

    Ok(())
}
fn get_function_pages(namespace:&CrateNamespace, fname_pre:&'static str) -> Result<(Arc<Mutex<MappedPages>>, usize), &'static str> {
    let section_ref = 
        match namespace.get_symbol_starting_with(fname_pre).upgrade() {
            Some(symbol) => {symbol},
            None => { 
                println!("no single symbol matching \"{}\"", fname_pre);
                return Err("Fail to get the symbol");
            }
        };

	let section = section_ref.lock();
	Ok((section.mapped_pages.clone(), section.mapped_pages_offset)) 
}