use zero::{Pod, read};
use core::slice;
use core::mem;
use spin::Once;
use kernel_config::memory::KERNEL_OFFSET;


static RSDT_CACHED: Once<Rsdt> = Once::new();


pub fn init() {
	info!("\n\nFound RSDP: {:?}", get_rsdp());
}


#[repr(C,packed)]
pub struct SdtHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oemid: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creatorid: u32,
    creator_revision: u32,
}

#[repr(C,packed)]
pub struct Rsdt {
	header: SdtHeader,
	tables_addr: usize,
}


const RSDP_MAGIC: [u8; 8] = [b'R', b'S', b'D', b' ', b'P', b'T', b'R', b' '];

// RDSP doc here: http://wiki.osdev.org/RSDP

#[derive(Debug)]
pub enum Rsdp<'a> {
	V1(&'a Rsdp1),
	V2(&'a Rsdp2),
}

#[derive(Debug)]
#[repr(packed)]
pub struct Rsdp1 {
	signature: [u8; 8],
	checksum: u8,
	oemid: [u8; 6],
	revision: u8,
	rsdt_phys_addr: u32,
}
impl Rsdp1 {
	fn is_valid(&self) -> bool {
		(self.signature == RSDP_MAGIC) 
		&& (checksum(self) & 0xFF == 0)	
	}
}

#[derive(Debug)]
#[repr(C,packed)]
pub struct Rsdp2 {
	v1: Rsdp1,
	// Version 2.0
	length: u32,
	xsdt_phys_addr: u64,
	ext_checksum: u8,
	_resvd1: [u8; 3],
}
impl Rsdp2 {
	fn is_valid(&self) -> bool {
		(self.v1.is_valid()) 
		&& (checksum(self) & 0xFF == 0)
	}
}

unsafe impl Pod for Rsdp1 { }
unsafe impl Pod for Rsdp2 { }


fn find_rsdp<'a>(region: &'a[u8]) -> Option<Rsdp> {
	// the RSDP_MAGIC string is aligned on a 16-byte boundary, so we look at 16-byte chunks
	let end_bounds = region.len() - mem::size_of::<Rsdp1>();
	for i in (0 .. end_bounds).step_by(16) {
		let rsdp: &'a Rsdp1 = read(&region[i..]);
		if !rsdp.is_valid() {
			continue; 
		}
		if rsdp.revision == 0 {
			trace!("found RSDPv1: {:?}", rsdp);
			return Some(Rsdp::V1(rsdp));
		}
		else {
			let rsdp2: &'a Rsdp2 = read(&region[i..]);
			trace!("found RSDPv2: {:?}", rsdp2);
			if rsdp2.is_valid() {
				return Some(Rsdp::V2(rsdp2));
			}
		}
	}
	
	None
}


/// Obtain a reference to the RSDP (will be in the identity mapping area)
fn get_rsdp<'a>() -> Option<Rsdp<'a>> {
	// try the BIOS region first
	// trace!("get_rsdp() trying BIOS region");
	let bios_ver = find_rsdp(unsafe { slice::from_raw_parts((KERNEL_OFFSET + 0xE_0000) as *const u8, 0x2_0000) });
	if bios_ver.is_some() {
		return bios_ver;
	}

	// if no luck, try the older EBDA region
	// trace!("get_rsdp() trying EBDA region");
	let ebda_ver = find_rsdp(unsafe { slice::from_raw_parts((KERNEL_OFFSET + 0x9_FC00) as *const u8, 0x400) });
	if ebda_ver.is_some() {
		return ebda_ver;
	}

	None
}

/// Sums up every individual byte in an entire Pod structure
fn checksum<T: Pod>(s: &T) -> usize
{
	// SAFE: T is Pod and can be read as bytes
	unsafe {
		let ptr = s as *const T as *const u8;
		let vals = slice::from_raw_parts(ptr, mem::size_of::<T>());
		vals.iter().fold(0, |a, &b| { a + (b as usize) } )
	}
}