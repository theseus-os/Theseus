//! Functions for initializing and bringing up other CPU cores. 
//! 
//! These functions are intended to be invoked from the main core
//! (the BSP -- bootstrap core) in order to jumpstart other cores.

#![no_std]
#![feature(asm)]

extern crate spin;

use spin::Mutex;

// graphic mode information
pub static GRAPHIC_INFO:Mutex<GraphicInfo> = Mutex::new(GraphicInfo{
    width:0,
    height:0,
    physical_address:0,
});

/// A structure to access framebuffer information 
/// that was discovered and populated in the AP's real-mode 
/// initialization seqeunce.
/// TODO FIXME: remove this struct, find another way to obtain framebuffer info.
pub struct GraphicInfo{
    pub width:u64,
    pub height:u64,
    pub physical_address:u64,
}

pub fn init(info: &GraphicInfo) {
    let mut graphic_info = GRAPHIC_INFO.lock();
    *graphic_info = GraphicInfo {
        width:info.width,
        height:info.height,
        physical_address:info.physical_address,
    };
}
