//! Support for the FAT32 Filesystem.
//! Inspired by the [intel hypervisor firmware written in rust](https://github.com/intel/rust-hypervisor-firmware/)
//! 
//! Limitations (and other notes):
//! Some key performance improvements for most FAT filesystem drivers are not implemented or are only partially implemented in this driver.
//! As such, this crate is not suggested for any heavy workloads without adding some of those tricks.
//! The first improvement would be to cache offset in file -> cluster lookup in memory somewhere. 
//! Without this caching reading from a position in a file is O(position), which is unacceptable in practice.
//! Furthermore, directories only partially implement a lazy reading strategy. 
//! The lazy read will be repeated whenever we try to get a file that doesn't exist since we do not cache the state of the lazy
//! read (instead we just restart if we don't find a file). This wouldn't be a hard fix, but it hasn't been done.
//! 
//! Below is a an example of detecting a fat storage, initializing the file system and doing open and read operations on a file.
//! 
//! ```rust
//! #![no_std]
// #![feature(slice_concat_ext)]

//! #[macro_use] extern crate alloc;
//! #[macro_use] extern crate terminal_print;
//! extern crate device_manager;
//! #[macro_use] extern crate log;
//! extern crate fat32;
//! extern crate ata;
//! extern crate storage_device;
//! extern crate spin;
//! extern crate fs_node;
//!
//! use alloc::sync::{Arc, Weak};
//! use spin::Mutex;
//! use alloc::vec::Vec;
//! use alloc::string::String;
//! use fs_node::File;
//! use fs_node::Directory;
//! use fat32::root_dir;
//!
//!
//!if let Some(controller) = storage_manager::STORAGE_CONTROLLERS.lock().iter().next() {
//!    for sd in controller.lock().devices() {
//!        match fat32::init(sd) {
//!            Ok(fatfs) => {
//!                let fs = Arc::new(Mutex::new(fatfs));
//!                // Creating the root directory first
//!                let mut root_dir = root_dir(fs.clone()).unwrap();
//!                
//!                // Able to list out the names of the folder entries
//!                println!("{:?}", root_dir.list());
//!
//!                root_dir.get("test");
//!
//!                // Uses the path provided and reads the bytes of the file otherwise returns 0 if file can't be found
//!                // The path can be in the format of /hello/poem.txt or \\hello\\poem.txt
//!                let path = format!("\\hello\\poem.txt"); // works for subdirectories and files that span beyond a single cluster
//!                
//!                // This open function create a file structure based on the path if it is found
//!                match fat32::open(fs.clone(), &path) {
//!                    Ok(f) => {
//!                        debug!("file size:{}", f.size);
//!                        let mut bytes_so_far = 0;
//!                        
//!                        // the buffer provided must be a multiple of the cluster size in bytes, so if the cluster is 8 sectors
//!                        // the buffer must be a multiple of 8*512 (4096 bytes)
//!                        let mut data: [u8; 4096*2] = [0;4096*2];
//!                        
//!                        match f.read(&mut data, 0) {
//!                            Ok(bytes) => {
//!                                bytes_so_far += bytes;
//!                            }
//!                            Err(_er) => panic!("the file failed to read"),
//!                            }
//!                        ;
//!                        debug!("{:X?}", &data[..]);
//!                        debug!("{:?}", core::str::from_utf8(&data));
//!
//!                        println!("bytes read: {}", bytes_so_far);
//!                    }
//!                    Err(_) => println!("file doesnt exist"),
//!                        }
//!
//!                let path2 = format!("\\test");
//!                let file2 = fat32::open(fs.clone(), &path2);
//!                println!("name of second file is: {}", file2.unwrap().name);
//!            }
//!            
//!            Err(_) => {
//!                ();
//!            }
//!        }
//!    }
//!}
//!Ok(())
//! ```
//! 

// TODO cat seems work weirdly with the bigfile example I've got -> doesn't print the last few characters.
// ^ Should make some different examples to test with.

// TODO the docs at the top of this crate aren't all that adviseable at this point. I'll try to make some fixes soon.

#![no_std]
#![feature(slice_concat_ext)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate bitflags;
extern crate storage_device;
extern crate storage_manager;
extern crate fs_node;
extern crate spin;
extern crate memory;
extern crate root;
extern crate byteorder;
extern crate zerocopy;
extern crate block_io;
extern crate spawn;
extern crate task;

use storage_device::{BlockBounds};
use zerocopy::{FromBytes, AsBytes};
use byteorder::ByteOrder;
use byteorder::LittleEndian;
use alloc::collections::BTreeMap;
use spin::{Mutex, RwLock};
use fs_node::{DirRef, WeakDirRef, FileRef, Directory, FileOrDir, File, FsNode};
use alloc::sync::{Arc, Weak};
use core::convert::TryInto;
use core::mem::size_of;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use memory::MappedPages;
use block_io::BlockIo;
use spawn::{KernelTaskBuilder, ApplicationTaskBuilder};
use task::TaskRef;

// DO NOT UNDER ANY CIRCUMSTANCE CHECK THIS FOR EQUALITY TO DETERMINE EOC
/// The end-of-cluster value written by this implementation of FAT32.
/// Note that many implementations use different values for EOC
const EOC: u32 = 0x0fff_ffff;
/// Magic byte used to mark a directory table as free. Set as first byte of name field.
const FREE_DIRECTORY_ENTRY: u8 = 0xe5;

/// Byte to mark entry as a "dot" entry.
const DOT_DIRECTORY_ENTRY: u8 = 0x2e;

/// Empty cluster marked as 0.
const EMPTY_CLUSTER: u32 = 0;

// TODO use these at some point.
bitflags! {
    struct FileAttributes: u8 {
        const READ_ONLY = 0x1;
        const HIDDEN = 0x2;
        const SYSTEM = 0x4;
        const VOLUME_LABEL = 0x8;
        const SUBDIRECTORY = 0x10;
        const ARCHIVE = 0x20;
        const DEVICE = 0x40;
        // Combinations of flags that are useful:
        const VFAT = Self::READ_ONLY.bits | Self::HIDDEN.bits | Self::SYSTEM.bits | Self::VOLUME_LABEL.bits;
    }
}

/// Checks if a cluster number is an EOC value:
#[inline]
fn is_eoc(cluster: u32) -> bool {
    cluster_from_raw(cluster) >= 0x0fff_fff8
}

/// Checks if a cluster number counts as free:
#[inline]
fn is_empty_cluster(cluster: u32) -> bool {
    cluster_from_raw(cluster) == EMPTY_CLUSTER
}

// TODO misleading name makes it sound like we're handling the endian-ness
/// Converts a cluster number read raw from disk into the value used for computation.
#[inline]
fn cluster_from_raw(cluster: u32) -> u32 {
    (cluster & 0x0fff_ffff)
}

// REVIEW I'm not sure if these are worth keeping around seeing as anything on the public interface will use static strings.
// Internally EOF is used on non-error paths so it's non-trivial to get rid of this behavior unless we make the returns Result<Option<..>>
/// Internal error types
#[derive(Debug, PartialEq)]
pub enum Error {
    BlockError,
    Unsupported,
    NotFound,
    EndOfFile,
    InvalidOffset,
    IllegalArgument,
    DiskFull,
}

/// Indicates whether the PFSDirectory entry is a PFSFile or a PFSDirectory
#[derive(Debug, PartialEq)]
enum FileType {
    PFSFile,
    PFSDirectory,
}

/// Internal Directory storage type
type ChildList = BTreeMap<String, FileOrDir>;

// TODO rewrite this structure using zerocopy crate for simplicity of reading from disk.
/// The first 25 bytes of a FAT formatted disk BPB contains these fields and are used to know important information about the disk 
pub struct Header {
    data: Vec<u8>,
    bytes_per_sector: u16, // usually 512 bytes
    sectors_per_cluster: u8, // allowed values are 1,2,4,8...128 dependent on size of drive
    reserved_sectors: u8, // the number of reserved sectors, usually 32 for FAT32
    fat_count: u8, // #of FATs, almost always 2
    _root_dir_count: u16, // the max number of root directors... should be 0 for FAT32 bc root is stored in data
    legacy_sectors: u16, // total logical sectors, can be set to 0 and instead use the sector count at the end of this struc
    _media_type: u8, // defines the type of media, f8 for hard disks
    legacy_sectors_per_fat: u8, // fat32 sets this to 0 and uses sectors per fat on the fat32 header
    _sectors_per_track: u16, // unused for hard drives w/ no CHs access anymore. a nuetral value of 1 can be put here
    _head_count: u16, // number for heads for disk, but unused for hard drives, can put a nuetral 1 in place
    _hidden_sectors: u32, // count of hidden sectors
    sectors: u32, // total logical sectors, should only be used if legacy_sector is = 0 
} 

impl Header {
    
    /// Initializes the header information from the first 512 bytes of the disk
    pub fn new(disk: storage_device::StorageDeviceRef) -> Result<Header, &'static str>  {

        let mut bpb_sector: Vec<u8> = vec![0; disk.lock().sector_size_in_bytes()];
        let _sectors_read = match disk.lock().read_sectors(&mut bpb_sector[..], 0) {
            Ok(bytes) => bytes,
            Err(_) => return Err("not able to read sector"),
        };

        // Since DOS 2.0, valid x86-bootable disks must start with either a short jump followed by a NOP (opstring sequence 0xEB 0x?? 0x90)
        match &bpb_sector[..3] {
            [235, _, 144] => debug!("valid bootable disk"),
            _ => debug!("invalid disk"),  
        }

        let fatheader = Header {
            data: bpb_sector.clone(),
            bytes_per_sector: LittleEndian::read_u16(&bpb_sector[11..13]),
            sectors_per_cluster: bpb_sector[13],
            reserved_sectors: bpb_sector[14],
            fat_count: bpb_sector[16],
            _root_dir_count: LittleEndian::read_u16(&bpb_sector[17..19]),
            legacy_sectors: LittleEndian::read_u16(&bpb_sector[19..21]),
            _media_type: bpb_sector[21],
            legacy_sectors_per_fat: bpb_sector[22],
            _sectors_per_track: LittleEndian::read_u16(&bpb_sector[0x18..0x20]), // TODO why is this indexed with hex?
            _head_count: LittleEndian::read_u16(&bpb_sector[26..28]),
            _hidden_sectors: LittleEndian::read_u32(&bpb_sector[28..32]),
            sectors: LittleEndian::read_u32(&bpb_sector[32..36]),
        };

        debug!("Disk header information:");
        debug!("bytes per sector: {}", fatheader.bytes_per_sector);
        debug!("sectors per cluster: {}", fatheader.sectors_per_cluster);
        debug!("reserved sectors: {}", fatheader.reserved_sectors);
        debug!("legacy sectors: {}", fatheader.legacy_sectors);
        debug!("the media type, should be f8 for hardrive: {:X}", fatheader._media_type);
        debug!("sectors per track: {}", fatheader._sectors_per_track);
        debug!("number of sectors: {}", fatheader.sectors);
        
        Ok(fatheader)
    }
}

/// Fat32 specific extended BPB that contains more information about the disk
/// refer to: https://en.wikipedia.org/wiki/Design_of_the_FAT_file_system
struct Fat32Header {
    sectors_per_fat: u32, // logical sectors per FAT
    _flags: u16, 
    _version: u16, // version #, FAT32 implementations should refuse to mount volumes w/ unknown version numbers
    root_cluster: u32, // cluster # where the root starts, typically 2
    _fsinfo_sector: u16, // the sector number of FS info, used to speed up accesss time for certain operations (mainly to get the amount of free space) typically 1
    _backup_boot_sector: u16, // First logical sector # of a copy of boot sector, value of 0x0000 indicates no backup sector 
    _drive_no: u8, 
    _nt_flags: u8, 
    _signature: u8, // boot signature used to signify the use of serial/volume/id 
    _serial: u32, // considered the volume ID
    
}

impl Fat32Header {

    /// Intializes the fat32 extended header using the first 512 bytes of the disk
    pub fn new(mut bpb_sector: Vec<u8>) -> Result<Fat32Header, &'static str> {
        let fat32header = Fat32Header {
            sectors_per_fat: LittleEndian::read_u32(&mut bpb_sector[36..40]) as u32,
            _flags: LittleEndian::read_u16(&mut bpb_sector[40..42]) as u16,
            _version: LittleEndian::read_u16(&mut bpb_sector[42..44]) as u16,
            root_cluster: LittleEndian::read_u32(&mut bpb_sector[44..48]) as u32,
            _fsinfo_sector: LittleEndian::read_u16(&mut bpb_sector[48..50]) as u16,
            _backup_boot_sector: LittleEndian::read_u16(&mut bpb_sector[50..52]) as u16,
            _drive_no: bpb_sector[64],
            _nt_flags: bpb_sector[65],
            _signature: bpb_sector[66],
            _serial: LittleEndian::read_u32(&mut bpb_sector[67..71]) as u32,
        };
        Ok(fat32header)
    }
}

#[repr(packed)]
#[derive(FromBytes, AsBytes)]
// TODO I'd like to make cluster high and low not-vectors. But the endian-ness package we have only supports
// this sort of operation.
/// A raw 32-byte FAT directory which is packed and has little endian fields.
struct RawFatDirectory {
    name: [u8; 11], // 8 letter entry with a 3 letter ext
    flags: u8, // designate the PFSdirectory as r,w,r/w
    _unused1: [u8; 8], // unused data
    cluster_high: u16, // but contains permissions required to access the PFSfile
    _unused2: [u8; 4], // data including modified time
    cluster_low: u16, // the starting cluster of the PFSfile
    size: u32, // size of PFSfile in bytes, volume label or subdirectory flags should be set to 0
}

impl RawFatDirectory {
    fn to_fat_directory(&self) -> FatDirectory {
        FatDirectory {
            name: self.name,
            flags: self.flags,
            _unused1: self._unused1,
            cluster_high: u16::from_le(self.cluster_high),
            _unused2: self._unused2,
            cluster_low: u16::from_le(self.cluster_low),
            size: u32::from_le(self.size),
        }
    }
    
    fn to_directory_entry(&self) -> DirectoryEntry {
        DirectoryEntry {
            name: self.name,
            file_type: if self.flags & FileAttributes::SUBDIRECTORY.bits == FileAttributes::SUBDIRECTORY.bits {
                FileType::PFSDirectory
            } else {
                FileType::PFSFile
            },
            cluster: (u32::from(u16::from_le(self.cluster_high))) << 16 | u32::from(u16::from_le(self.cluster_low)),
            size: u32::from_le(self.size),
            long_name: [0; 255] // long names not supported. TODO
        } 
    }
    
    fn make_unused() -> RawFatDirectory {
        RawFatDirectory {
            name: [FREE_DIRECTORY_ENTRY; 11],
            flags: 0,
            _unused1: [0; 8],
            cluster_high: 0,
            _unused2: [0; 4],
            cluster_low: 0,
            size: 0,
        }
    }
}

// TODO do we still need this type? (haven't yet migrated away from it I suppose). May still be useful due to not wanting to do operations with RawFatDirectory.
/// formatted filesystem
struct FatDirectory {
    name: [u8; 11], // 8 letter entry with a 3 letter ext
    flags: u8, // designate the PFSdirectory as r,w,r/w
    _unused1: [u8; 8], // unused data
    cluster_high: u16, // but contains permissions required to access the PFSfile
    _unused2: [u8; 4], // data including modified time
    cluster_low: u16, // the starting cluster of the PFSfile
    size: u32, // size of PFSfile in bytes, volume label or subdirectory flags should be set to 0
}

impl FatDirectory {
    #[inline]
    fn is_free(&self) -> bool {
        self.name[0] == 0 ||
            self.name[0] == FREE_DIRECTORY_ENTRY 
            // TODO looks like this doesn't seem to match the implementation that generated my code
    }

    #[inline]
    fn is_dot(&self) -> bool {
        self.name[0] == DOT_DIRECTORY_ENTRY
    }

    #[inline]
    fn is_vfat(&self) -> bool {
        self.flags & FileAttributes::VFAT.bits == FileAttributes::VFAT.bits
    }
    
    fn to_directory_entry(&self) -> DirectoryEntry {
        DirectoryEntry {
            name: self.name,
            file_type: if self.flags & 0x10 == 0x10 {
                FileType::PFSDirectory
            } else {
                FileType::PFSFile
            },
            cluster: (u32::from(self.cluster_high)) << 16 | u32::from(self.cluster_low),
            size: self.size,
            long_name: [0; 255] // long names not supported. TODO
        } 
    }
    
    fn to_raw_fat_directory(&self) -> RawFatDirectory {
        RawFatDirectory {
            name: self.name,
            flags: self.flags,
            _unused1: self._unused1,
            cluster_high: u16::to_le(self.cluster_high),
            _unused2: self._unused2,
            cluster_low: u16::to_le(self.cluster_low),
            size: u32::to_le(self.size),
        }
    }
}

/// Based on the FatDirectory structure and used to describe the files/subdirectories in a FAT32 
/// formatted filesystem, used for convience purposes
pub struct DirectoryEntry {
    pub name: [u8; 11], // TODO note that mapping this to a rust string can be challenging.
    long_name: [u8; 255], // Long-name format currently not supported
    file_type: FileType,
    pub size: u32,
    cluster: u32,
}

/// TODO these methods need to potentially generate a sequence of entries.
impl DirectoryEntry {
    fn to_directory_entry(&self) -> FatDirectory {
        // Janky but I don't know if we want the fat_directory type for much longer anyway?
        // FIXME
        self.to_raw_fat_directory().to_fat_directory()
    }
    
    fn to_raw_fat_directory(&self) -> RawFatDirectory {
        RawFatDirectory {
            name: self.name,
            flags: match self.file_type {
                FileType::PFSFile => 0x0,
                FileType::PFSDirectory => 0x10,
            }, // FIXME need to store more flags in DirectoryEntry type
            _unused1: [0; 8],
            cluster_high: u16::to_le((self.cluster >> 16) as u16),
            _unused2: [0; 4],
            cluster_low: u16::to_le((self.cluster) as u16),
            size: u32::to_le(self.size),
        }
    }
}

/// Structure for a file in FAT32. This key information is used to transverse the
/// drive to find the file data  
pub struct PFSFile {
    cc: ClusterChain,
}

impl PFSFile {
    /// Given an offset, returns the PFSPosition of the file
    pub fn seek(&self, fs: &mut Filesystem, offset: usize) -> Result<PFSPosition, &'static str> {
        let mut position: usize = 0;

        let cluster_size_in_bytes: usize = fs.sectors_per_cluster() as usize * fs.bytes_per_sector as usize;
        let clusters_to_advance: usize =  offset/cluster_size_in_bytes;
        
        // FIXME call cluster_advancer here.
        let mut counter: usize = 0;
        let mut current_cluster = self.cc.cluster;

        while clusters_to_advance != counter {
            match fs.next_cluster(current_cluster) {
                Err(_e) => {
                    return Err("Error in moving to next clusters");
                }
                Ok(cluster) => {
                    counter += 1;
                    current_cluster = cluster;
                }
            }
        }
        
        let nth_cluster = current_cluster;
        let byte_difference = offset - (clusters_to_advance * cluster_size_in_bytes);
        let reached_sector = byte_difference / (fs.bytes_per_sector as usize);
        // debug!("cluster: {}", nth_cluster);
        // debug!("sector: {}", reached_sector);
        // debug!("byte_offset: {}", byte_difference - reached_sector as usize *fs.bytes_per_sector as usize);
        return Ok(PFSPosition{
            cluster: nth_cluster,
            sector_offset: reached_sector,
            entry_offset: byte_difference - reached_sector as usize *fs.bytes_per_sector as usize
        })
    }
}

impl File for PFSFile {
    /// Given an empty data buffer and a FATfile structure, will read off the bytes of that FATfile and place the data into the data buffer
    /// If the given buffer is less than the size of the file, read will return the first bytes read unless
    /// 
    /// # Arguments 
    /// * `data`: the source buffer. The length of the `data` determines how many clusters will be read, 
    ///   and must be an even multiple of the filesystem's cluster size in bytes = [`sector size`]*[`sectors per cluster`].
    /// * `offset`: the byte offset for the file to begin reading at
    /// 
    /// # Returns
    /// 
    /// Returns the number of bytes read 
    fn read(&self, data: &mut [u8], offset: usize) -> Result<usize, &'static str> {
        
        // Place a lock on the filesystem
        let mut fs = self.cc.filesystem.lock();

        info!("Read called with buffer size {:}, position: {:}. File size: {:}", data.len(), offset, self.size());
        
        // Treat our file like a sequence of clusters using the BlockBounds logic.
        let BlockBounds { range, first_block_offset, .. } = BlockBounds::block_bounds(offset, data.len(), self.size(),
            fs.size_in_clusters(self.size()), fs.cluster_size_in_bytes())?;
        let block_size_in_bytes: usize = fs.cluster_size_in_bytes();

        // Read the actual data, one block at a time.
		let mut src_offset = first_block_offset; 
		let mut dest_offset = 0;
        let mut current_cluster = self.cc.cluster;
        let mut current_offset = 0;
        let mut bytes_copied = 0;

		for cluster_offset in range { // Number of clusters into the file
            // Jump to current cluster:
            current_cluster = self.cc.cluster_advancer(&mut fs, current_cluster, cluster_offset - current_offset)
                .map_err(|_| "Read failed")?;

            current_offset = cluster_offset;

			// don't copy past the end of `buffer`
			let num_bytes_to_copy = core::cmp::min(block_size_in_bytes - src_offset, data.len() - dest_offset);

            let temp_buffer = &mut data[dest_offset.. (dest_offset + num_bytes_to_copy)];

            let first_sector = fs.first_sector_of_cluster(current_cluster);
            let offset_of_sector = fs.sector_to_byte_offset(first_sector);

            let _bytes_read = fs.io.read(temp_buffer, offset_of_sector)?;

			trace!("fat32::read(): for cluster {}, copied bytes into buffer[{}..{}] from block[{}..{}]",
				current_cluster, dest_offset, dest_offset + num_bytes_to_copy, src_offset, src_offset + num_bytes_to_copy,
			);
			dest_offset += num_bytes_to_copy;
            bytes_copied += num_bytes_to_copy;
			src_offset = 0;
		}
        
        Ok(bytes_copied as usize)
    }


    fn write(&mut self, _buffer: &[u8], _offset: usize) -> Result<usize, &'static str> {
        Err("write not implemeneted yet")
    }

    /// Returns the size of the file
    fn size(&self) -> usize {
        (self.cc.size as usize)
    }     

    fn as_mapping(&self) -> Result<&MappedPages, &'static str> {
        Err("Mapping a fatfile as a MappedPages object is unimplemented")
    }  
}

impl FsNode for PFSFile {
    fn get_name(&self) -> String {
        self.cc.name.clone()
    }

    fn get_parent_dir(&self) -> Option<DirRef> {
        self.cc.parent.upgrade()
    }

    fn set_parent_dir(&mut self, _new_parent: WeakDirRef) {
        warn!("set parent for file not supported yet");
    }
}

/// Structure to store the position of a fat32 directory during a walk.
pub struct PFSPosition {
    /// Cluster number of the current position.
    cluster: u32, 
    /// Sector offset into the current cluster (should this be 32 bits? Not sure if it really matters)
    sector_offset: usize, 
    /// Entry offset into the current sector
    entry_offset: usize, 
}

/// Structure for a FAT32 directory.
pub struct PFSDirectory {
    /// Underlying strucuture on disk containing the Directory data. 
    cc: ClusterChain,
    // TODO: children should maybe incude some directory entry like information?
    /// Potentially incomplete list of children. TODO make DirRef into our internal type.
    children: RwLock<ChildList>,
    /// Self reference to simplify construction of children.
    dot: WeakDirRef,
}

impl Directory for PFSDirectory {
    fn insert(&mut self, node: FileOrDir) -> Result<Option<FileOrDir>, &'static str> {
        let name = node.get_name();

        // TODO validate that the node is actually an instance of a PFS type.
        // if not then we will not have sufficient information to actually make it.
        

        // Validate that the name is less than the maximum length for a directory entry. TODO
        
        Err("insert directory not implemented yet")
    }

    fn get(&self, name: &str) -> Option<FileOrDir> {
        match name {
            "." => self.dot.upgrade().map(|x| FileOrDir::Dir(x)),
            ".." => self.cc.parent.upgrade().map(|x| FileOrDir::Dir(x)),
            name => self.fat32_get(name).ok(),
        }
    }

    fn list(&self) -> Vec<String> {

        // Ensure we've walked the whole list:
        let mut fs = self.cc.filesystem.lock();
        let mut children = self.children.write();

        //debug!("Listing directory entries");

        match self.walk_until(&mut fs, &mut children, None) {
            Ok(_) => {}, // TODO this case doesn't happen when None is an argument.
            Err(Error::EndOfFile) | // Cases that represent a successful directory walk.
            Err(Error::NotFound) => {},
            Err(_) => {
                warn!("Failed to fully walk directory");
                return children.keys().map(|x| x.clone()).collect::<Vec<String>>();
            }

        }

        return children.keys().map(|x| x.clone()).collect::<Vec<String>>();

        /*
        let mut list_of_names: Vec<String> = Vec::new();

        let fs = self.cc.filesystem.lock();
        // Returns a vector of the 32-byte directory structures
        let entries = match self.entries(&fs) {
            Ok(entries) => entries,
            Err(_) => {
                warn!("Failed to read list of names from directory");
                return list_of_names;
            },
        };

        // Iterates through the 32-byte directory structures and pushes the name of each one into the list
        for entry in entries {
            list_of_names.push(match core::str::from_utf8(&entry.name) {
                Ok(name) => name.to_string(),
                Err(_) => return list_of_names,
            })
        }
        
        list_of_names
        */
    }

    fn remove(&mut self, _node: &FileOrDir) -> Option<FileOrDir> {
        Option::None
    }
}

impl FsNode for PFSDirectory {
    fn get_name(&self) -> String {
        self.cc.name.clone()
    }

    fn get_parent_dir(&self) -> Option<DirRef> {
        self.cc.parent.upgrade()
    }

    fn set_parent_dir(&mut self, _new_parent: WeakDirRef) {
        // self.parent = new_parent;
        debug!("set parent dir for Directroy is currently not implemented yet")
    }
}

impl Drop for PFSDirectory {
    fn drop(&mut self) {
        warn!("PFSDirectory:drop(): {:?}", self.get_name());
    }
}

impl PFSDirectory {

    // FIXME (parallelism improvements)
    // Note that these next methods require holding a mutable reference to the children tree.
    // And so using them prevents any changes to the children tree, I think a good solution
    // if performance/parallelism were desired would be to build a list during the walk and then
    // add the directory entries in a smaller helper method that locks the child tree for a shorter time.

    /// Walk the directory and adds all encountered entries into children tree.
    /// Returns Ok(Entry) if found. Otherwise returns Err(Error::NotFound) if not found (or no name given).
    fn walk_until(&self, fs: &mut Filesystem, children: &mut ChildList, 
        name: Option<&str>) -> Result<DirectoryEntry, Error> {
        
        let mut pos = self.cc.initial_pos();
        
        loop {
            let dir: FatDirectory = self.get_fat_directory(fs, &pos)?;
            //debug!("got fat directory");
            
            // TODO VFAT long name check needs to be done correctly.
            if !dir.is_free() && !dir.is_vfat() && !dir.is_dot() { // Don't add dot directories. Must be handled specially. FIXME
                debug!("Found new entry {:?}", debug_name(&dir.name));
                let entry = dir.to_directory_entry();

                // While we're here let's add the entry to our children.
                self.add_directory_entry(fs, children, &entry)?;

                match name {
                    Some(name) => {
                        if compare_name(name, &entry) {
                            return Ok(entry);
                        }
                    },
                    None => {},
                }
            }

            // If the first byte of the directory is 0 we have reached the end of allocated directory entries
            if dir.name[0] == 0 {
                debug!("Reached end of used files on directory.");
                return Err(Error::NotFound);
            }
            
            pos = match self.cc.advance_pos(fs, &pos, size_of::<RawFatDirectory>()) {
                Err(Error::EndOfFile) => return Err(Error::NotFound),
                Ok(new_pos) => new_pos,
                Err(e) => return Err(e),
            };
            debug!("New position: cluster: {}, sector_off: {}, byte off: {}", pos.cluster, pos.sector_offset, pos.entry_offset);
        }
    }

    /// Add a directory entry into the children list. Does nothing if entry is already in child list.
    /// Does not change any structures on disk. Also will not verify that 
    fn add_directory_entry(&self, fs: &Filesystem, children: &mut ChildList, 
        entry: &DirectoryEntry) -> Result<FileOrDir, Error> {

        // TODO actually check the name:
        let name: String = match core::str::from_utf8(&entry.name) {
            Ok(name) => name.to_string(),
            Err(_) => {
                warn!("Couldn't convert name {:?} to string", entry.name);
                return Err(Error::IllegalArgument);
            }
        };

        if children.contains_key(&name) {
            return match children.get(&name) {
                Some(node) => Ok(node.clone()),
                None => Err(Error::Unsupported) // This should never happen.
            }
        };

        // Create FileOrDir and insert:
        let cc = ClusterChain {
            cluster: entry.cluster,
            _num_clusters: None,
            name: name.clone(), // FIXME this is wrong.
            on_disk_refcount: 1, // FIXME not true for directories. Should rename so that this is fine.
            filesystem: self.cc.filesystem.clone(),
            parent: self.dot.clone(),
            size: entry.size,
        };

        match entry.file_type {
            FileType::PFSDirectory => {


                let dir = PFSDirectory {
                    cc: cc,
                    children: RwLock::new(BTreeMap::new()),
                    dot: Weak::<Mutex<PFSDirectory>>::new(),
                };

                // Dirty shenanigans to set dot weak loop.
                let dir_ref = Arc::new(Mutex::new(dir));
                dir_ref.lock().dot = Arc::downgrade(&dir_ref) as WeakDirRef;
                children.insert(name, FileOrDir::Dir(dir_ref.clone()));

                return Ok(FileOrDir::Dir(dir_ref));
            }
            
            FileType::PFSFile => {
                
                let file = PFSFile {
                    cc: cc,
                };

                let file_ref = Arc::new(Mutex::new(file));
                children.insert(name, FileOrDir::File(file_ref.clone()));

                return Ok(FileOrDir::File(file_ref));
            }
        }
    }

    /// Returns a vector of all the directory entries in a directory without mutating the directory
    pub fn entries(&self, fs: &mut Filesystem) -> Result<Vec<DirectoryEntry>, Error> {
        debug!("Getting entries for {:?}", self.cc.name);

        let mut entry_collection: Vec<DirectoryEntry> = Vec::new(); 
        let mut pos = self.cc.initial_pos();
        
        loop {
            let dir: FatDirectory = self.get_fat_directory(fs, &pos)?;
            
            // TODO VFAT long name check needs to be done correctly.
            if !dir.is_free() && !dir.is_vfat() {
                debug!("Found new entry {:?}", debug_name(&dir.name));
                entry_collection.push(dir.to_directory_entry());
            }

            // If the first byte of the directory is 0 we have reached the end of allocated directory entries
            if dir.name[0] == 0 {
                return Ok(entry_collection);
            }
            
            pos = match self.cc.advance_pos(fs, &pos, size_of::<RawFatDirectory>()) {
                Err(Error::EndOfFile) => return Ok(entry_collection),
                Ok(new_pos) => new_pos,
                Err(e) => return Err(e),
            };
            debug!("New position: cluster: {}, sector_off: {}, byte off: {}", pos.cluster, pos.sector_offset, pos.entry_offset);
        }
    }

    /// Returns the entry indicated by PFSPosition. TODO allow this to save reads and writes.
    /// the function is used and returns EndOfFile if the next PFSdirectory doesn't exist
    /// Note that this function will happily return unused directory entries since it simply provides a sequential view of the entries on disk.
    fn get_fat_directory(&self, fs: &mut Filesystem, pos: &PFSPosition) -> Result<FatDirectory, Error> {
        let sector = fs.first_sector_of_cluster(pos.cluster) + pos.sector_offset;

        debug!("Getting from cluster: {}, sector: {} (offset: {}), entry_offset: {}", pos.cluster, sector, pos.sector_offset, pos.entry_offset);
        // Get the sector on disk corresponding to our sector and cluster:
        let offset = fs.sector_to_byte_offset(sector) + pos.entry_offset;
        let mut buf = [0; size_of::<RawFatDirectory>()];
        debug!("Trying to read len: {}, from offset: {}", buf.len(), offset);
        let _bytes_read = fs.io.read(&mut buf, offset).map_err(|e| {
            warn!("Disk read failed: {:?}", e);
            Error::BlockError
        })?;
        
        let entry_offset = pos.entry_offset;
        let fat_dir = FatDirectory {
            name: match buf[(0)..(11)].try_into() {
                        Ok(array) => array,
                        Err(_) => return Err(Error::BlockError),
            },
            flags: buf[11],
            _unused1: match buf[(12)..(20)].try_into() {
                        Ok(array) => array,
                        Err(_) => return Err(Error::BlockError),
            },
            cluster_high: LittleEndian::read_u16(&mut buf[(20)..(22)]) as u16,
            _unused2: match buf[(22)..(26)].try_into() {
                Ok(array) => array,
                Err(_) => return Err(Error::BlockError),
            },
            cluster_low: LittleEndian::read_u16(&mut buf[(26)..(28)]) as u16,
            size: LittleEndian::read_u32(&mut buf[(28)..(32)]) as u32,
        };
        
        // TODO this needs to ensure that unused directory entries are properly marked?
        Ok(fat_dir)
    }

    // FIXME use sub byte transactions at some point?
    /// Writes the RawFatDirectory dir to the position pos in the file.
    fn write_fat_directory(&self, fs :&mut Filesystem, pos: &PFSPosition, dir: &RawFatDirectory) -> Result<usize, Error> {
        let cluster = pos.cluster;
        let base_sector = fs.first_sector_of_cluster(cluster);
        let sector = pos.sector_offset + base_sector;
        let entry = pos.entry_offset;
        
        // Verify that entry is a multiple of the size of object want to write:
        if entry % size_of::<RawFatDirectory>() != 0 {
            return Err(Error::IllegalArgument);
        }
        
        let offset = entry + fs.sector_to_byte_offset(sector);
        
        fs.io.write(dir.as_bytes(), offset).map_err(|_| Error::BlockError)
    }
    
    // TODO: support larger sizes (for placing long name directories)
    /// Walks the directory tables and finds an empty entry then returns a position where that entry can be placed.
    /// Currently does not support sizes larger than 1 entry.
    pub fn find_empty_or_grow(&self, fs: &mut Filesystem, size_needed: usize) -> Result<PFSPosition, Error> {
        if size_needed == 0 {
            return Err(Error::IllegalArgument);
        }
    
        if size_needed > 1 {
            warn!("find_empty_or_grow called with size larger than 1: {:}. Treating as 1", size_needed);
        }
        let size_needed = 1;
        
        let mut pos = self.cc.initial_pos();
        
        loop {
            let dir: FatDirectory = self.get_fat_directory(fs, &pos)?;
            
            if dir.is_free() {
                return Ok(pos); // Pos is now set to the position of a free directory so we can continue.
            }
            
            pos = match self.cc.advance_pos(fs, &pos, size_of::<RawFatDirectory>()) {
                Err(Error::EndOfFile) => return self.grow_directory(fs, &pos),
                Ok(new_pos) => new_pos,
                Err(e) => return Err(e),
            }
        }
    }
    
    // I think the "best" solution is perhaps to make everything that's not part of the public interface pass the fs in as an argument.
    // Make FS an argument I suppose. Seems dumb though.
    /// Grows the directory given a PFSPosition in the last cluster of the file and returns a new position in the new_cluster.
    fn grow_directory(&self, fs: &mut Filesystem, pos_end: &PFSPosition) -> Result<PFSPosition, Error> {
        
        // Rename for convenience with other existing code.
        let pos = pos_end;
        
        // TODO grow the node: 
        // Otherwise find an empty cluster.
        let new_cluster = fs.extend_chain(pos.cluster)?;
        
        // Now write all the entries in that cluster to be unused and return a PFSPosition with those entries.
        let mut pos = PFSPosition {
            cluster: new_cluster,
            sector_offset: 0,
            entry_offset: 0,
        };
        
        let unused_entry = RawFatDirectory::make_unused();
        
        loop {
            self.write_fat_directory(fs, &pos, &unused_entry)?; // FIXME write an entry with 0 name instead of unused name.
            pos = match self.cc.advance_pos(fs, &pos, size_of::<RawFatDirectory>()) {
                Ok(p) => p,
                Err(Error::EndOfFile) => break,
                Err(e) => return Err(e),
            }
        }
        
        // Update the size in the parent's directory entry. -> Note this isn't actually necessary for a directory.
        // TODO make sure we don't actually need to update size in dir_entry.
        
        // Update the size now that the directory is at the end.
        // TODO verify that we don't need to do this.
        
        pos.cluster = new_cluster;
        pos.sector_offset = 0;
        pos.entry_offset = 0;
        Ok(pos)
    }

    /// Internal insert function. TODO enforce type on arguments.
    fn fat32_insert(&mut self, node: FileOrDir) -> Result<Option<FileOrDir>, Error> {
        let name = node.get_name();


        // Must lock after the call to get since that also locks the fs.
        let mut fs = self.cc.filesystem.lock();

        // Find an empty node or grow the directory as needed. TODO different size.
        let empty_pos = self.find_empty_or_grow(&mut fs, 1)?;

        // TODO...
        //match self.write_fat_directory(&fs, pos, node.)
        
        Err(Error::Unsupported)
    }

    /// Internal get function. TODO enforce returning PFSType?
    fn fat32_get(&self, name: &str) -> Result<FileOrDir, Error> {

        // TODO need some sort of string typing that ensure it's a valid fat32 name that will match the string we insert into the children list.
        match self.children.read().get(name) {
            Some(child) => return Ok(child.clone()),
            None => {},
        };

        let mut fs = self.cc.filesystem.lock();

        // FIXME: this can be written to only walk the directory once (and also add entries to children that aren't the ones we needed).
        let entries = self.entries(&mut fs)?;
        
        for entry in entries {
            // Compares the name of the next entry in the current directory
            if compare_name(name, &entry) {
                // Found the entry on disk. Need to create a new cluster chain for the object:
                let cc = ClusterChain {
                    cluster: entry.cluster,
                    _num_clusters: None,
                    name: name.to_string(), // FIXME this is wrong.
                    on_disk_refcount: 1, // FIXME not true for directories. Should rename so that this is fine.
                    filesystem: self.cc.filesystem.clone(),
                    parent: self.dot.clone(),
                    size: entry.size,
                };

                // Get a write lock for the children and check if it was found before creating a new object (since we dropped the read lock).
                let mut children = self.children.write();
                match children.get(name) {
                    Some(child) => return Ok(child.clone()),
                    None => {},
                };

                match entry.file_type {
                    FileType::PFSDirectory => {


                        let dir = PFSDirectory {
                            cc: cc,
                            children: RwLock::new(BTreeMap::new()),
                            dot: Weak::<Mutex<PFSDirectory>>::new(),
                        };

                        // Dirty shenanigans to set dot weak loop.
                        let dir_ref = Arc::new(Mutex::new(dir));
                        dir_ref.lock().dot = Arc::downgrade(&dir_ref) as WeakDirRef;
                        children.insert(name.to_string(), FileOrDir::Dir(dir_ref.clone()));

                        return Ok(FileOrDir::Dir(dir_ref));
                    }
                    
                    FileType::PFSFile => {
                        
                        let file = PFSFile {
                            cc: cc,
                        };

                        let file_ref = Arc::new(Mutex::new(file));
                        children.insert(name.to_string(), FileOrDir::File(file_ref.clone()));

                        return Ok(FileOrDir::File(file_ref));
                    }
                }
            }
        }

        Err(Error::NotFound)
    }
}

// TODO: should also consider how to relate these to directory entries for write.
/// Underlying disk object that is common to Files and Directories. In many ways this maps to a directory entry.
#[derive(Clone)]
struct ClusterChain {
    filesystem: Arc<Mutex<Filesystem>>,
    cluster: u32,
    pub name: String,
    pub parent: WeakDirRef, // the directory that holds the metadata for this directory. The root directory's parent should be itself
    on_disk_refcount: usize, // Number of references on disk. For a file must always be one. For a directory this is variable.
    _num_clusters: Option<u32>, // Unknown without traversing FAT table. I consider this useful information, but I'm not sure if I'll ever use it.
    pub size: u32,
}

impl ClusterChain {
    /// Returns a cluster walk position at the initial position.
    pub fn initial_pos(&self) -> PFSPosition {
        PFSPosition {
            cluster: self.cluster,
            sector_offset: 0,
            entry_offset: 0, 
        }
    }
    
    /// Advances a PFSPosition by offset and returns a new position if successful.
    pub fn advance_pos(&self, fs: &mut Filesystem, pos: &PFSPosition, offset: usize) -> Result<PFSPosition, Error> {
        
        let mut new_pos = PFSPosition {
            cluster : pos.cluster,
            entry_offset : pos.entry_offset + offset,
            sector_offset : pos.sector_offset, 
        };
        
        if new_pos.entry_offset >= fs.bytes_per_sector as usize {
            new_pos.sector_offset += new_pos.entry_offset / fs.bytes_per_sector as usize; // TODO need to be careful about this actually working.
            new_pos.entry_offset = new_pos.entry_offset % fs.bytes_per_sector as usize;
        }
        
        if new_pos.sector_offset >= fs.sectors_per_cluster as usize {
            let cluster_advance = new_pos.sector_offset / fs.sectors_per_cluster as usize;
            new_pos.cluster = self.cluster_advancer(fs, pos.cluster, cluster_advance)?;
            new_pos.sector_offset = new_pos.sector_offset % fs.sectors_per_cluster as usize;
        }
        
        Ok(new_pos)
    }

    /// Advance "cluster_advance" number of clusters.
    pub fn cluster_advancer(&self, fs: &mut Filesystem, start_cluster: u32, cluster_advance: usize) -> Result<u32, Error> {
        
        let mut counter: usize = 0;
        let mut current_cluster = start_cluster;
        while cluster_advance != counter {
            current_cluster = fs.next_cluster(current_cluster)?;
            counter += 1;
        }
        return Ok(current_cluster);
    }
}

// TODO I'd like to rethink this code so that it's easier to initialize and so that much of the public (and generally *immutable*) information is not behind a lock.
/// Structure for the filesystem to be used to traverse the disk and run operations on the disk
pub struct Filesystem {
    header: Header, // This is meant to read the first 512 bytes of the virtual drive in order to obtain device information
    io: BlockIo, // Cached byte level reader and writer to and from disk.
    bytes_per_sector: u32, // default set to 512 // TODO why aren't we using this instead of the drive's value?
    sectors: u32, // depends on number of clusters and sectors per cluster
    fat_type: u8, // support for fat32 only
    clusters: u32, // number of clusters
    sectors_per_fat: u32, 
    sectors_per_cluster: u32,
    fat_count: u32, 
    root_dir_sectors: u32, 
    pub first_fat_sector: u32,
    first_data_sector: u32,
    data_sector_count: u32, 
    data_cluster_count: u32,
    root_cluster: u32, // Will always be 2 for FAT32
}

impl Filesystem {
    
    // Initiate a new FAT filesystem with all 0 values
    pub fn new(header: Header, sd: storage_device::StorageDeviceRef) -> Result<Filesystem, &'static str> {
        debug!("filesystem started");
        let io = BlockIo::new(sd);

        let fs = Filesystem {
            header,
            io, 
            bytes_per_sector: 0,
            sectors: 0,
            fat_type: 0,
            clusters: 0,
            sectors_per_fat: 0,
            sectors_per_cluster: 0,
            fat_count: 0,
            root_dir_sectors: 0,
            first_fat_sector: 0,
            first_data_sector: 0,
            data_sector_count: 0,
            data_cluster_count: 0,
            root_cluster: 0,
        };
        
        fs.init()
    }

    /// Reads the sector and fills in the filesystem fields using information from the specified fat headers
    fn init(mut self) -> Result<Filesystem, &'static str> {
        const FAT12_MAX: u32 = 0xff5;
        const FAT16_MAX: u32 = 0xfff5;

        let h = &self.header;

        // Assigns the values of the filesystem strcutre based on the data obtained from the FAT header
        self.bytes_per_sector = u32::from(h.bytes_per_sector);
        self.fat_count = u32::from(h.fat_count);
        self.sectors_per_cluster = u32::from(h.sectors_per_cluster);
        self.sectors = if h.legacy_sectors == 0 {
            h.sectors
        } else {
            u32::from(h.legacy_sectors)
        };

        self.clusters = self.sectors / u32::from(h.sectors_per_cluster);

        // Based on the maximum # of clusters that each fat type has, assigns the fat_type
        self.fat_type = if self.clusters < FAT12_MAX {
            12
        } else if self.clusters < FAT16_MAX {
            16
        } else {
            32
        };

        // If the fat_type is 32, you have to use the FAT32Header information to fill in the rest of the FS strcture
        if self.fat_type == 32 {
            // Initiates a FAT32 header
            let h32 = match Fat32Header::new(self.header.data.clone()) {
                Ok(fat32header) => fat32header,
                Err(_err) => return Err("failed to initialize fat32header")
            }; 
            self.sectors_per_fat = h32.sectors_per_fat;
            self.root_cluster = h32.root_cluster;
        } 
        else {
            self.sectors_per_fat = u32::from(h.legacy_sectors_per_fat);
        }

        // Formulas used to know important sector numbers that different components start at 
        self.first_fat_sector = u32::from(h.reserved_sectors);
        self.first_data_sector =
            self.first_fat_sector + (self.fat_count * self.sectors_per_fat) + self.root_dir_sectors;
        self.data_sector_count = self.sectors - self.first_data_sector;
        self.data_cluster_count = self.data_sector_count / self.bytes_per_sector;
        //debug!("First data sector: {}", self.first_data_sector);
        debug!("Bytes per sector: {}", self.bytes_per_sector);
        Ok(self)
    }

    /// This function allows you to jump to the data component when given a cluster number   
    fn first_sector_of_cluster(&self, cluster: u32) -> usize {
        // The first sector of a cluster = first portion of data component + the amount of sectors/cluster + accounting for root cluster
        (((cluster - 2) * self.sectors_per_cluster) + self.first_data_sector) as usize
    }

    /// Walks the FAT table to find the next cluster given the current cluster.
    /// Returns Err(Error::EOF) if the next cluster is an EOC indicator.
    fn next_cluster(&mut self, cluster: u32) -> Result<u32, Error> {
        match self.fat_type {
            32 => {
                // FAT32 uses 32 bits per FAT entry, so 1 entry = 4 bytes
                let fat_entry = Filesystem::fat_entry_from_cluster(cluster);
                let fat_sector = self.fat_sector_from_fat_entry(fat_entry);
                let entry_offset = fat_entry % self.bytes_per_sector;
                debug!("the current cluster is:{}, need to read sector: {}, offset from entry: {}, entry: {}", cluster, fat_sector, entry_offset, fat_entry);
    
                // Because FAT32 uses 4 bytes per cluster entry, to get to a cluster's exact byte entry in the FAT you can use the cluster number * 4
                // And to get to the next cluster, the FAT must be read in little endian order using the mutliple_hex_to_int function 
                // Read more here: https://en.wikipedia.org/wiki/Design_of_the_FAT_file_system#FAT
                let mut buf = [0; 4];
                let offset = fat_entry as usize + self.sector_to_byte_offset(self.first_fat_sector as usize);
                match self.io.read(&mut buf, offset) {
                    Ok(_) => (),
                    Err(_) => return Err(Error::BlockError),

                }
                let next_cluster_raw = u32::from_le_bytes(buf);
                let next_cluster = cluster_from_raw(next_cluster_raw);

                debug!("the next cluster is: {:x}, raw: {:x}", next_cluster, next_cluster_raw);
                
                if is_eoc(next_cluster) {
                    Err(Error::EndOfFile)
                } else {
                    Ok(next_cluster)
                }
            }

            _ => {
                warn!("next_cluster called for unsupported FS");
                Err(Error::Unsupported)
            },
        }
    }
    
    /// Walks the FAT to find an empty cluster and returns the number of the first cluster found.
    fn find_empty_cluster(&mut self) -> Result<u32,Error>{
        
        // Magic number: first cluster that might not be root directory.
        let mut cluster = 3;
        
        
        while cluster < self.clusters {
            let fat_entry = Filesystem::fat_entry_from_cluster(cluster);
            let fat_sector = self.fat_sector_from_fat_entry(fat_entry);
            // Fetch necessary fat_sector
            let mut data: Vec<u8> = vec![0; self.bytes_per_sector as usize];
            let _bytes_read = match self.io.read(&mut data, self.sector_to_byte_offset(fat_sector as usize)) {
                Ok(bytes_read) => Ok(bytes_read),
                Err(_) => Err(Error::BlockError),
            };
            // Read each of the clusters in the sector we fetched and see if any are empty.
            loop {
                let fat_entry = Filesystem::fat_entry_from_cluster(cluster);
                let entry_offset = fat_entry % (self.bytes_per_sector / size_of::<u32>() as u32);
                
                // Read from our slice of the table:
                let cluster_value = LittleEndian::read_u32(
                    &mut data[entry_offset as usize..(entry_offset as usize +size_of::<u32>())]) as u32;
                    
                if cluster_value == EMPTY_CLUSTER {
                    return Ok(cluster);
                }
                
                // Update cluster and check if we need to fetch another piece of data:
                cluster = cluster + 1;
                let next_entry = Filesystem::fat_entry_from_cluster(cluster);
                let next_sector = self.fat_sector_from_fat_entry(next_entry);
                if next_sector != fat_sector && cluster < self.clusters {
                    break;
                }
            }
        }
        
        return Err(Error::DiskFull);
    }
    
    /// Extends a cluster chain by one cluster and returns the new cluster of the extended chain.
    fn extend_chain(&mut self, old_tail: u32) -> Result<u32, Error> {
        
        // Confirm that the old tail is actually the end of the chain.
        let cluster_value = self.read_fat_cluster(old_tail)?;
        let cluster_value = cluster_from_raw(cluster_value);
            
        if !is_eoc(cluster_value) {
            warn!("Tried to extend chain without end of cluster. Found {:x} at cluster {:x}", cluster_value, old_tail);
            return Err(Error::IllegalArgument);
        }
        
        let mut cluster_found = false;

        // See if we can trivially extend the chain?
        let mut new_cluster_candidate = old_tail + 1;

        let next_cluster_value = self.read_fat_cluster(new_cluster_candidate)?;
        let next_cluster_value = cluster_from_raw(next_cluster_value);
                
        if is_empty_cluster(next_cluster_value) {
            cluster_found = true;
        }
        
        // Handle the case where we need to walk the FAT to find a free cluster:
        if !cluster_found {
            new_cluster_candidate = self.find_empty_cluster()?;
        }
        

        // Write the old out:
        self.write_fat_cluster(old_tail, new_cluster_candidate)?;
        self.write_fat_cluster(new_cluster_candidate, EOC)?;

        Ok(new_cluster_candidate)
    }

    /// Reads the value at cluster from the FAT table
    fn read_fat_cluster(&mut self, cluster: u32) -> Result<u32, Error> {
        let mut buf = [0; 4];
        let fat_entry = Self::fat_entry_from_cluster(cluster);
        let sector = self.fat_sector_from_fat_entry(fat_entry);
        let offset = self.sector_to_byte_offset(self.first_fat_sector as usize) + fat_entry as usize;

        return match self.io.read(&mut buf, offset as usize) {
            Ok(_bytes) => {
                Ok(u32::from_le_bytes(buf))
            },
            Err(_) => Err(Error::BlockError),
        }
    }

    /// Writes the cluster in the FAT table with new_value.
    fn write_fat_cluster(&mut self, cluster: u32, new_value: u32) -> Result<(), Error> {
        let mut buf = u32::to_le_bytes(new_value);
        let fat_entry = Self::fat_entry_from_cluster(cluster);
        let sector = self.fat_sector_from_fat_entry(fat_entry);
        let offset = self.sector_to_byte_offset(self.first_fat_sector as usize) + fat_entry as usize;

        return match self.io.write(&mut buf, offset as usize) {
            Ok(_bytes) => {
                Ok(())
            },
            Err(_) => Err(Error::BlockError),
        }
    }

    #[inline]
    /// Computes the number of bytes per cluster.
    fn cluster_size_in_bytes(&self) -> usize {
        return (self.bytes_per_sector * self.sectors_per_cluster) as usize;
    }
    
    #[inline]
    /// Computes the fat entry in bytes in the FAT corresponding to a given cluster.
    fn fat_entry_from_cluster(cluster: u32) -> u32 {
        cluster * 4
    }

    #[inline]
    /// Computes the offset from the start of the disk given a sector
    fn sector_to_byte_offset(&self, sector: usize) -> usize {
        sector * self.bytes_per_sector as usize
    }
    
    #[inline]
    /// Computes the disk sector corresponding to a given fat entry to read the FAT.
    fn fat_sector_from_fat_entry(&self, fat_entry: u32) -> u32 {
        self.first_fat_sector + (fat_entry / self.bytes_per_sector) // TODO check against max size?
    }

    #[inline]
    /// Computes the size of a file of size `size` in clusters (rounds up)
    fn size_in_clusters(&self, size: usize) -> usize {
        (size + self.cluster_size_in_bytes() - 1) / (self.cluster_size_in_bytes())
    }


    #[inline]
    /// Returns the number of sectors per cluster as given by the header
    fn sectors_per_cluster(&self) -> u32 {
        self.sectors_per_cluster
    }
    
    // TODO should this function even exist?
    /// Returns a reference to the BPB derived header
    fn header(&self) -> &Header {
        &self.header
    }
}

/// Takes in a drive for a filesystem and initializes it if it's FAT32 
/// 
/// # Arguments
/// * `sd`: the storage device that contains the FAT32 filesystem structure
/// 
/// # Return
/// If the drive passed in contains a fat filesystem, this returns the filesystem structure needed to run operations on the disk 
pub fn init(sd: storage_device::StorageDeviceRef) -> Result<Filesystem, &'static str>  {
    info!("Attempting to initialize a fat32 filesystem");
    // Once a FAT32 filesystem is detected, this will create the Filesystem structure from the drive
    if detect_fat(&sd) == true {
        let header = match Header::new(sd.clone()){
            Ok(header) => header,
            Err(_) => return Err("failed to intialize header"),
        };
        match Filesystem::new(header, sd) {
            Ok(fs) => {
                // return Ok(Arc::new(Mutex::new(fs)));
                return Ok(fs);
            } 
            Err(_) => return Err("failed to intialize fat filesystem for disk"),
        };        
    }   
    Err("failed to intialize fat filesystem for disk")
}

/// Detects whether the drive passed into the function is a FAT32 drive
/// 
/// # Arguments
/// * `disk`: the storage device that contains the FAT32 filesystem structure
/// 
/// # Return value
/// Returns true if the storage device passed into the function has a fat filesystem structure
pub fn detect_fat(disk: &storage_device::StorageDeviceRef) -> bool {
    const fat32_magic: [u8; 8] = *b"FAT32   ";
    
    let mut initial_buf: Vec<u8> = vec![0; disk.lock().sector_size_in_bytes()];
    let _sectors_read = match disk.lock().read_sectors(&mut initial_buf[..], 0){
        Ok(bytes) => bytes,
        Err(_) => return false,
    };
    info!("Magic sequence: {:X?}", &initial_buf[82..90]);
    // The offset at 0x52 in the extended FAT32 BPB is used to detect the Filesystem type ("FAT32   ")
    initial_buf[82..90] == fat32_magic
}


/// A case-insenstive way to compare FATdirectory entry name which is in [u8;11] and a str literal to be able to 
/// confirm whether the directory/file that you're looking at is the one specified 
/// 
/// # Arguments
/// * `name`: the name of the next destination in the path
/// * `de`: the directory entry that's currently being looked at
/// 
/// # Examples
/// ```rust
/// let str = "fat32";
/// let de.name: [u8;11] = [0x46, 0x41, 0x54, 0x33, 0x32, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20];
/// assert_eq!(compare_name(&str, de), true)
/// ```
pub fn compare_name(name: &str, de: &DirectoryEntry) -> bool {
    compare_short_name(name, de) || &de.long_name[0..name.len()] == name.as_bytes()
}

fn compare_short_name(name: &str, de: &DirectoryEntry) -> bool {
    // 8.3 (plus 1 for the separator)
    if name.len() > 12 {
        return false;
    }

    let mut i = 0;
    for (_, a) in name.as_bytes().iter().enumerate() {
        // Handle cases which are 11 long but not 8.3 (e.g "loader.conf")
        if i == 11 {
            return false;
        }

        // Jump to the extension
        if *a == b'.' {
            i = 8;
            continue;
        }

        let b = de.name[i];
        if a.to_ascii_uppercase() != b.to_ascii_uppercase() {
            return false;
        }

        i += 1;
    }
    true
}

// TODO is this mount name argument dumb? It seems like we want a semi-arbitrary name to support names for mount purposes.
/// Creates a FATdirectory structure for the root directory
/// 
/// # Arguments 
/// * `fs`: the filesystem structure wrapped in an Arc and Mutex
/// * `mount_name`: the name to used to refer to the new entry. Set to / if not mounting.
/// 
/// # Returns
/// Returns the FATDirectory structure for the root directory 
pub fn root_dir(fs: Arc<Mutex<Filesystem>>, mount_name: String) -> Result<Arc<Mutex<RootDirectory>>, Error> {

    // TODO right now this function can violate the singleton blahdy blah and risks making two cluster chains for this dir.
    // We also can't just use something like a lazy static, since it's a per-FS basis. Maybe the FS needs some information to prevent this.

    let cc = ClusterChain {
        filesystem: fs.clone(),
        cluster: 2,
        name: mount_name,
        parent: Weak::<Mutex<PFSDirectory>>::new(),
        on_disk_refcount: 2 + 1, // TODO should this be the case?
        _num_clusters: None,
        size: 0, // According to wikipedia File size for directories is always 0.
    };

    let mut underlying_root = PFSDirectory {
        cc: cc,
        children: RwLock::new(BTreeMap::new()),
        dot: Weak::<Mutex<PFSDirectory>>::new(),
    };

    let new_root_dir = RootDirectory {
        underlying_dir: underlying_root,
    };

    let true_root = Arc::new(Mutex::new(new_root_dir));
    let true_root_as_dir: DirRef = true_root.clone();

    true_root.lock().underlying_dir.dot = Arc::downgrade(&true_root_as_dir);
    true_root.lock().underlying_dir.set_parent_dir(Arc::downgrade(&true_root_as_dir));

    Ok(true_root) 
        
}

// Note that right now this additional structure does not really do much.
// Likely this structure may be vestigial and will be remoovable.
// But I'm inclined to leave it if we want to do any special treatments of mount points.
/// Root directory for FAT32 filesystem.
pub struct RootDirectory {
    /// The PFSDirectory doing the work for the root directory.
    underlying_dir: PFSDirectory,
}

impl Directory for RootDirectory {
    fn insert(&mut self, node: FileOrDir) -> Result<Option<FileOrDir>, &'static str> {
        self.underlying_dir.insert(node)
    }

    fn get(&self, name: &str) -> Option<FileOrDir> {
        self.underlying_dir.get(name)
    }

    fn list(&self) -> Vec<String> {
        self.underlying_dir.list()
    }

    fn remove(&mut self, node: &FileOrDir) -> Option<FileOrDir> {
        self.underlying_dir.remove(node)
    }
}

impl FsNode for RootDirectory {

    fn get_name(&self) -> String {
        self.underlying_dir.get_name()
    }

    /// Since this might be a mount point we end up 
    fn get_parent_dir(&self) -> Option<DirRef> {
        self.underlying_dir.get_parent_dir()
    }

    // In this case we want to update the parent dir, but don't do anything on disk:
    fn set_parent_dir(&mut self, new_parent: WeakDirRef) {
        self.underlying_dir.cc.parent = new_parent;
    }
}

impl Drop for RootDirectory {
    fn drop(&mut self) {
        warn!("RootDirectory:drop(): {:?}", self.get_name());
    }
}

/// Function to print the raw byte name as a string. TODO find a better way.
fn debug_name(byte_name: &[u8]) -> &str {
    match core::str::from_utf8(byte_name) {
        Ok(name) => name,
        Err(_) => "Couldn't find name"
    }
}

#[deprecated]
/// A debug function used to create a DirRef from the RootDirectory (since root_dir used to return a RootDirectory object)
/// Exists because creating the Arc<Mutex<..>> in a user applications causes page faults for some reason.
pub fn make_dir_ref(dir: RootDirectory) -> DirRef {
    Arc::new(Mutex::new(dir))
}

// TODO move these functions elsewhere. Mostly for debugging some confusing issues.
/// Spawns some applications processes to perform some mount operations and tests for FAT32 support.
pub fn test_module_init() -> Result<(), &'static str> {

    // start a task that tries to find the module in root.
    // let taskref = KernelTaskBuilder::new(test_find, ())
    //     .name("fat32_find_test".to_string())
    //     .block()
    //     .spawn()?;

    // start a task that tries to find the module in root.
    // KernelTaskBuilder::new(test_insert, None)
    //     .name("fat32_insert_test".to_string())
    //     .spawn()?;

    Ok(())
}

/// Attempts to mount a fat32 FS and mount to the root directory as "fat32"
/// Additionally unblocks a task that needs to wait until after the mount is complete.
fn test_insert(taskref: Option<TaskRef>) ->  Result<(), &'static str> {
    if let Some(controller) = storage_manager::STORAGE_CONTROLLERS.lock().iter().next() {
        for sd in controller.lock().devices() {
            match init(sd) {
                Ok(fatfs) => {

                    let fs = Arc::new(Mutex::new(fatfs));
                    // TODO if we change the root dir creation approach this will also change.
                    // Take as read only for now?

                    let name = "fat32";


                    let fat32_root = root_dir(fs.clone(), name.clone().to_string()).unwrap();


                    let true_root_ref = root::get_root();
                    let mut true_root = true_root_ref.lock();

                    match true_root.insert(FileOrDir::Dir(fat32_root.clone())) {
                        Ok(_) => trace!("Successfully mounted fat32 FS"),
                        Err(_) => trace!("Failed to mount fat32 FS"),
                    };
                    
                    fat32_root.lock().set_parent_dir(Arc::downgrade(&true_root_ref));


                    // Now let's try a couple simple things:
                    let test_root = true_root.get_dir(&name).unwrap();
                    debug!("Root directory entries: {:?}", test_root.lock().list());    

                    // Unblock the find task now that we've finished our insert.
                    taskref.map(|x| x.unblock());  

                    return Ok(());      
                }
                
                Err(_) => {
                }
            }
        }
    }
    Err("Couldn't initialize FAT32 FS")
}

/// A debug method. Attempts to find the fat32 entry from the root directory (created by test_insert).
/// Verifies that the fat32 entry exists and prints the contents.
fn test_find(_: ()) ->  Result<(), &'static str> {
    
    let root = root::get_root().lock();

    let dir = match root.get_dir("fat32") {
        Some(dir) => dir,
        None => return Err("Couldn't find fat32 FS"),
    };

    let entries = dir.lock().list();
    debug!("Entries in fat32 dir: {:?}", entries);
    debug!("Successfully found and printed fat32 dir");
    Ok(())
}