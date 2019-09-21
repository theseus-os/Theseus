//! Support for the FAT32 Filesystem.
//! Inspired by the [intel hypervisor firmware written in rust](https://github.com/intel/rust-hypervisor-firmware/)
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
//!                // Reaches the root dir and able to go through each of the entries in the root folder using the next_entry()
//!                // but next_entry should not be used to go through a folder because it mutates the folder
//!                let de = root_dir.next_entry().unwrap();
//!                println!("the name of the next entry is: {:?}", de.name);
//!                println!("the name of the next_entry is: {:?}", core::str::from_utf8(&de.name));
//!
//!                let de = root_dir.next_entry().unwrap();
//!                println!("the name of the next entry is: {:?}", de.name);
//!                println!("the name of the next_entry is: {:?}", core::str::from_utf8(&de.name));
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

use zerocopy::{FromBytes, AsBytes};
use byteorder::ByteOrder;
use byteorder::LittleEndian;
use alloc::collections::BTreeMap;
use spin::Mutex;
use fs_node::{DirRef, WeakDirRef, FileRef, Directory, FileOrDir, File, FsNode};
use alloc::sync::{Arc, Weak};
use core::convert::TryInto;
use core::mem::size_of;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use memory::MappedPages;

/// The end-of-cluster value written by this implementation of FAT32.
/// Note that many implementations use different values for EOC
/// DO NOT UNDER ANY CIRCUMSTANCE CHECK THIS FOR EQUALITY TO DETERMINE EOC
const EOC: u32 = 0x0fff_ffff;
/// Magic byte used to mark a directory table as free. Set as first byte of name field.
const FREE_DIRECTORY_ENTRY: u8 = 0xe5;

/// Byte to mark entry as a "dot" entry.
const DOT_DIRECTORY_ENTRY: u8 = 0x2e;

/// Empty cluster marked as 0.
const EMPTY_CLUSTER: u32 = 0;

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

/// The first 25 bytes of a FAT formatted disk BPB contains these fields and are used to know important information about the disk 
pub struct Header {

    drive: storage_device::StorageDeviceRef,
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
            drive: disk, 
            data: bpb_sector.clone(),
            bytes_per_sector: LittleEndian::read_u16(&bpb_sector[11..13]),
            sectors_per_cluster: bpb_sector[13],
            reserved_sectors: bpb_sector[14],
            fat_count: bpb_sector[16],
            _root_dir_count: LittleEndian::read_u16(&bpb_sector[17..19]),
            legacy_sectors: LittleEndian::read_u16(&bpb_sector[19..21]),
            _media_type: bpb_sector[21],
            legacy_sectors_per_fat: bpb_sector[22],
            _sectors_per_track: LittleEndian::read_u16(&bpb_sector[0x18..0x20]), // TODO why is this indexed with hex? It's not even two bytes
            _head_count: LittleEndian::read_u16(&bpb_sector[26..28]),
            _hidden_sectors: LittleEndian::read_u32(&bpb_sector[28..32]),
            sectors: LittleEndian::read_u32(&bpb_sector[32..36]),
        };

        // debug!("bytes per sector: {}", fatheader.bytes_per_sector);
        // debug!("sectors per cluster: {}", fatheader.sectors_per_cluster);
        // debug!("reserved sectors: {}", fatheader.reserved_sectors);
        // debug!("legacy sectors: {}", fatheader.legacy_sectors);
        // debug!("the media type, should be f8 for hardrive: {:X}", fatheader.media_type);
        // debug!("sectors per track: {}", fatheader._sectors_per_track);
        // debug!("number of sectors: {}", fatheader.sectors);
        
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
            file_type: if self.flags & 0x10 == 0x10 {
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

// TODO do we still need this type?
/// The actual 32-byte FAT directory format that contains metadata about a file/subdirectory inside a FAT32
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
    filesystem: Arc<Mutex<Filesystem>>,
    pub name: String,
    pub parent: WeakDirRef,
    pub start_cluster: u32,
    pub size: u32,
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
        let fs = self.filesystem.lock();

        let cluster_size_in_bytes: usize = fs.sectors_per_cluster() as usize * 512 as usize;
        let mut position: u32 = 0;
        let mut current_cluster = self.start_cluster;
        let mut sector_offset: u32 = 0;
        let mut cluster_offset = 0;

        let mut file_walk = PFSPosition{
            cluster: current_cluster,
            sector_offset: 0,
            entry_offset: 0,
        };

        // The inserted buffer must be a multiple of the cluster size in bytes for clean reads.
        // clusters act as a sort of boundary in FAT for every file/directory and to minimize
        // disk requests, its best to read clusters at a time
        if data.len() as usize % cluster_size_in_bytes != 0{
            return Err("data buffer size must be a multiple of the cluster size in bytes");
        }

        // Represents the number of clusters that must be read
        let num_reads: usize = data.len() as usize/cluster_size_in_bytes;
            
        for _i in 0..num_reads{   

            // The position is used to track how far along the PFSfile it is, and if the position goes
            // beyond the size of the PFSfile OR the array is full, it returns end of PFSfile
            if position >= (data.len() as u32) {
                debug!("buffer is filled");
                break;
            }

            // Reaches end of file
            if position >= self.size {
                debug!("end of file");
                break;
            }
            
            // When the end of the cluster is reached in terms of sector, it will move onto the next cluster
            if sector_offset == u32::from(fs.sectors_per_cluster()) {
                match fs.next_cluster(current_cluster) {
                    Err(_e) => {
                        return Err("Error in moving to next clusters");
                    }
                    Ok(cluster) => {
                        current_cluster = cluster;
                        sector_offset = 0;
                    }
                }
            }
            
            // the byte difference is the difference between the offset (the byte to begin reading at)
            // and the position(the current byte position based upon walking from cluster to cluster)
            let byte_difference = offset as u32 - position;
            let reached_sector = byte_difference/fs.bytes_per_sector;

            // this if statement determines wether moving on to the next cluster would cause it to move beyond the
            // specified offset and if it does, fills in the PFSPosition with info about the specific position of the offset
            if position + cluster_size_in_bytes as u32 > offset as u32 {
                file_walk.cluster = current_cluster;
                file_walk.sector_offset = reached_sector;
                file_walk.entry_offset = byte_difference as usize - reached_sector as usize*fs.bytes_per_sector as usize;
                debug!("the PFSposition cluster: {:?}", file_walk.cluster);
                debug!("the PFSposition sector offset: {:?}", file_walk.sector_offset);
                debug!("the PFSposition entry_offset: {:?}", file_walk.entry_offset);
            }

            let cluster_start = fs.first_sector_of_cluster(current_cluster);

            // the current sector thats to be read at
            let current_sector = cluster_start + sector_offset;

            // Reads at the beginning sector of the cluster
            let sectors_read = match fs.header().drive.lock().read_sectors(
                &mut data[(0+cluster_offset)..(cluster_size_in_bytes as usize + cluster_offset as usize)], 
                current_sector as usize) {
                    Ok(bytes) => bytes,
                    Err(_) => return Err("error reading sector"),
            };
            
            sector_offset += fs.sectors_per_cluster;
            
            if (position + sectors_read as u32) > self.size {
                let bytes_read = self.size - position;
                return Ok(bytes_read as usize);
            } 
            else {
                position += cluster_size_in_bytes as u32;
            }

            cluster_offset += cluster_size_in_bytes as usize;
            // debug!("current cluster {}",current_cluster);
            // debug!("current offset{}",sector_offset);
            // debug!("current position{}",position);
        }
        
        Ok(position as usize)
    }


    fn write(&mut self, _buffer: &[u8], _offset: usize) -> Result<usize, &'static str> {
        Err("write not implemeneted yet")
    }

    /// Returns the size of the file
    fn size(&self) -> usize {
        (self.size as usize)
    }     

    fn as_mapping(&self) -> Result<&MappedPages, &'static str> {
        Err("Mapping a fatfile as a MappedPages object is unimplemented")
    }  
}

impl FsNode for PFSFile {
    fn get_name(&self) -> String {
        self.name.clone()
    }

    fn get_parent_dir(&self) -> Option<DirRef> {
        self.parent.upgrade()
    }

    fn set_parent_dir(&mut self, _new_parent: WeakDirRef) {
        warn!("set parent for file not supported yet");
    }
}

/// Structure to store the position of a fat32 directory during a walk.
pub struct PFSPosition {
    /// Cluster number of the current position.
    cluster: u32, 
    /// Sector offset into the current cluster
    sector_offset: u32, 
    /// Entry offset into the current sector
    entry_offset: usize, 
}

#[derive(Clone)]
/// Structure for a FAT32 directory. 
pub struct PFSDirectory {
    filesystem: Arc<Mutex<Filesystem>>,
    pub name: String,
    pub parent: WeakDirRef, // the directory that holds the metadata for this directory. The root directory's parent should be itself
    pub cluster: u32, // the cluster number that the directory is stored in
    sector: u32,
    offset: usize,
    size: u32,
}

impl Directory for PFSDirectory {
    fn insert(&mut self, node: FileOrDir) -> Result<Option<FileOrDir>, &'static str> {
        let name = node.get_name();

        // TODO validate that the node is actually an instance of a PFS type.
        // if not then we will not have sufficient information to actually make it.
        

        // Validate that the name is less than the maximum length for a directory entry. TODO

        // TODO this is wrong behavior. We should have an internal search that returns the pos
        // of the file and then edit that file.
        if self.get(&name).is_some() {
            return Err("Matching item already exists.");
        }

        // Must lock after the call to get since that also locks the fs.
        let fs = self.filesystem.lock();

        // Find an empty node or grow the directory as needed. TODO different size.
        let empty_pos = match self.find_empty_or_grow(&fs, 1) {
            Ok(pos) => pos,
            Err(_e) => return Err("Failed to grow directory and couldn't find empty space."), // TODO better errors.
        };

        //match self.write_fat_directory(&fs, pos, node.)
        
        Err("insert directory not implemented yet")
    }

    fn get(&self, name: &str) -> Option<FileOrDir> {

        let entries = match self.entries() {
            Ok(collection) => collection,
            Err(_) => return Option::None,
        };
        
        for entry in entries {
            // Compares the name of the next entry in the current directory
            if compare_name(name, &entry) {
                match entry.file_type {
                    FileType::PFSDirectory => {
                        // Intializes the different trait objects depedent on whether the file is a file or subdirectory
                        let dir = match self.filesystem.lock().get_directory(entry.cluster, name.to_string(), Arc::new(Mutex::new(self.clone())) as DirRef, self.filesystem.clone(), entry.size) {
                            Ok(dir) => dir,
                            Err(_) => return Option::None,
                        };
                        let dir_ref = Arc::new(Mutex::new(dir)) as DirRef;
                        return Option::Some(FileOrDir::Dir(dir_ref));
                    }
                    
                    FileType::PFSFile => {
                        let file = match self.filesystem.lock().get_file(entry.cluster, entry.size, name.to_string(), Arc::new(Mutex::new(self.clone())) as DirRef, self.filesystem.clone()) {
                            Ok(dir) => dir,
                            Err(_) => return Option::None,
                        };
                        let file_ref = Arc::new(Mutex::new(file)) as FileRef;
                        return Option::Some(FileOrDir::File(file_ref));
                    }
                }
            }
        }

        Option::None
    }

    fn list(&self) -> Vec<String> {

        let mut list_of_names: Vec<String> = Vec::new();
        // Returns a vector of the 32-byte directory structures
        let entries = match self.entries2() {
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
    }

    fn remove(&mut self, _node: &FileOrDir) -> Option<FileOrDir> {
        Option::None
    }
}

impl FsNode for PFSDirectory {
    fn get_name(&self) -> String {
        self.name.clone()
    }

    fn get_parent_dir(&self) -> Option<DirRef> {
        self.parent.upgrade()
    }

    fn set_parent_dir(&mut self, _new_parent: WeakDirRef) {
        // self.parent = new_parent;
        debug!("set parent dir for Directroy is currently not implemented yet")
    }
}

impl PFSDirectory {
    
    // TODO abstract the state in these functions into some sort of directory walk structure?
    // I think this is the best way forward that achieves both performance and general code cleanliness goals.
    // Furthermore it would prevent state spill within the module.
    
    /// Sets the directory state to being the first entry in the directory (used by next_entry)
    pub fn initial_pos(&self) -> PFSPosition {
        PFSPosition {
            cluster: self.cluster,
            sector_offset: 0,
            entry_offset: 0, 
        }
    }
    
    // TODO this function can be abstracted onto the FS to simply take an offset (and this would call the FS implementation with offset=32)
    /// Advances a PFSPosition by one entry and returns the new entry if successful.
    /// Note that if EOC is reached the cluster is set to the EOC value found. I think this may not be ideal API design however so it potentially may need to change.
    pub fn advance_pos(&self, fs: &Filesystem, pos: &PFSPosition) -> Result<PFSPosition, Error> {
        
        let mut new_pos = PFSPosition {
            cluster : pos.cluster,
            entry_offset : pos.entry_offset + 32, // TODO I'd like to pull this constant from FatDirectory. This size is only 32 if the type is #repr(packed) however so that's not really ideal unless we want the type to model directly reading from disk (in which case we'd want something to annotate the numbers as little endian).
            sector_offset : pos.sector_offset, 
        };
        
        if new_pos.entry_offset >= fs.bytes_per_sector as usize {
            new_pos.entry_offset = 0;
            new_pos.sector_offset = new_pos.sector_offset + 1;
        }
        
        if new_pos.sector_offset >= fs.sectors_per_cluster {
            new_pos.sector_offset = 0;
            new_pos.cluster = fs.next_cluster(pos.cluster)?;
            //new_pos.entry_offset = 0;
        }
        
        Ok(new_pos)
    }
    
    
    /// Returns the entry indicated by PFSPosition. TODO allow this to save reads and writes.
    /// the function is used and returns EndOfFile if the next PFSdirectory doesn't exist
    /// Note that this function will happily return unused directory entries since it simply provides a sequential view of the entries on disk.
    fn get_fat_directory(&self, fs: &Filesystem, pos: &PFSPosition) -> Result<FatDirectory, Error> {
        
        let sector = fs.first_sector_of_cluster(pos.cluster) + pos.sector_offset;
        
        // Get the sector on disk corresponding to our sector and cluster:
        let mut data: Vec<u8> = vec![0; fs.bytes_per_sector as usize];
        let _sectors_read = match fs.header.drive.lock().read_sectors(&mut data[..], (sector) as usize) {
            Ok(bytes) => bytes,
            Err(_) => return Err(Error::BlockError),
        };
        
        let entry_offset = pos.entry_offset;
        let fat_dir = FatDirectory {
            name: match data[(entry_offset)..(11+entry_offset)].try_into() {
                        Ok(array) => array,
                        Err(_) => return Err(Error::BlockError),
            },
            flags: data[11+entry_offset],
            _unused1: match data[(12+entry_offset)..(20+entry_offset)].try_into() {
                        Ok(array) => array,
                        Err(_) => return Err(Error::BlockError),
            },
            cluster_high: LittleEndian::read_u16(&mut data[(20+entry_offset)..(22+entry_offset)]) as u16,
            _unused2: match data[(22+entry_offset)..(26+entry_offset)].try_into() {
                Ok(array) => array,
                Err(_) => return Err(Error::BlockError),
            },
            cluster_low: LittleEndian::read_u16(&mut data[(26+entry_offset)..(28+entry_offset)]) as u16,
            size: LittleEndian::read_u32(&mut data[(28+entry_offset)..(32+entry_offset)]) as u32,
        };
        
        // TODO this needs to ensure that unused directory entries are properly marked?
        Ok(fat_dir)
    }
    
    // FIXME use sub byte transactions at some point?
    /// Writes the RawFatDirectory dir to the position pos in the file.
    fn write_fat_directory(&self, fs :&Filesystem, pos: &PFSPosition, dir: &RawFatDirectory) -> Result<usize, Error> {
        let cluster = pos.cluster;
        let base_sector = fs.first_sector_of_cluster(cluster);
        let sector = pos.sector_offset + base_sector;
        let entry = pos.entry_offset;
        
        // Verify that entry is a multiple of the size of object want to write:
        if entry % size_of::<RawFatDirectory>() != 0 {
            return Err(Error::IllegalArgument);
        }
        
        // Fetch the data from the disk:
        let mut data: Vec<u8> = vec![0; fs.bytes_per_sector as usize];
        let _sectors_read = match fs.header.drive.lock().read_sectors(&mut data[..], sector as usize) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Err(Error::BlockError);
            },
        };
        
        // TODO needs to copy into byte array
        data[entry..(entry + size_of::<RawFatDirectory>())].copy_from_slice(dir.as_bytes());
        
        // Write the old out:
        let _sectors_written = match fs.header.drive.lock().write_sectors(&data, sector as usize) {
            Ok(sectors) => sectors,
            Err(_) => {
                warn!("Fat32 disk is in an inconsistent state");
                return Err(Error::BlockError);
            },
        };
        
        Ok(size_of::<RawFatDirectory>())
    }
    

    
    
    // TODO get rid of this function?
    /// Note that the offset used by this function is different from the offset used by the other next entry function.
    /// Returns the next FATDirectory entry in a FATdirectory while mutating the directory object to offset an entry each time 
    /// the function is used and returns EndOfFile if the next PFSdirectory doesn't exist
    #[deprecated]
    pub fn next_entry(&mut self) -> Result<DirectoryEntry, Error> {
        warn!("next_entry called. This corrupts the directory for future use");
        let fs = self.filesystem.lock();
        loop {
            let sector = if self.cluster > 0 {
                
                // Identifies if the sector number is greater than the size of one cluster, and if it is
                // it will move on to the next cluster
                if self.sector >= fs.sectors_per_cluster {
                    match fs.next_cluster(self.cluster) {
                        Ok(new_cluster) => {
                            self.cluster = new_cluster;
                            self.sector = 0;
                            self.offset = 0;
                        }
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }
                self.sector
                    + fs.first_sector_of_cluster(self.cluster)
            } else {
                self.sector // TODO what does this section do?
            };

            // let fs = self.filesystem;

            let mut data: Vec<u8> = vec![0; fs.header.drive.lock().sector_size_in_bytes()];
            let _sectors_read = match fs.header.drive.lock().read_sectors(&mut data[..], (sector) as usize) {
                Ok(bytes) => bytes,
                Err(_) => return Err(Error::BlockError),
            };
            
            let mut counter = 32;
            let mut directory_entries: Vec<FatDirectory> = Vec::new(); 

            // This goes through the 8 possible PFSdirectory entries in a PFSdirectory sector 
            // A PFSdirectory entry is 32 bytes, so it simply jumps to the next PFSdirectory entry by adding 32 bytes to the counter
            for _entries in 0..8 { 
                let dir: FatDirectory = FatDirectory {
                    name: match data[(0+counter)..(11+counter)].try_into() {
                        Ok(array) => array,
                        Err(_) => return Err(Error::BlockError),
                    },
                    flags: data[11+counter],
                    _unused1: match data[(12+counter)..(20+counter)].try_into() {
                        Ok(array) => array,
                        Err(_) => return Err(Error::BlockError),
                    },
                    cluster_high: LittleEndian::read_u16(&mut data[(20+counter)..(22+counter)]) as u16,
                    _unused2: match data[(22+counter)..(26+counter)].try_into() {
                        Ok(array) => array,
                        Err(_) => return Err(Error::BlockError),
                    }, 
                    cluster_low: LittleEndian::read_u16(&mut data[(26+counter)..(28+counter)]) as u16,
                    size: LittleEndian::read_u32(&mut data[(28+counter)..(32+counter)]) as u32
                };
                counter += 64;
                directory_entries.push(dir);
            }

            let dirs: Vec<FatDirectory> = directory_entries;            

            // debug!("sector: {}", self.sector);
            // debug!("offset: {}", self.offset);

            // Loops through the PFSdirectory entries and creates the PFSDirectory Entry strcture based on the FatDirectory fields 
            for i in self.offset..dirs.len() {
                let d = &dirs[i];
                // debug!("size of the PFSdirectory entry: {:?}", d.size);
                // When you reach the last PFSdirectory entry in a PFSdirectory
                if d.name[0] == 0x0 {
                    return Err(Error::EndOfFile);
                }
                // PFSDirectory unused
                if d.name[0] == 0xe5 {
                    continue;
                }
                
                let entry = DirectoryEntry {
                    name: d.name,
                    file_type: if d.flags & 0x10 == 0x10 {
                        FileType::PFSDirectory
                    } else {
                        FileType::PFSFile
                    },
                    cluster: (u32::from(d.cluster_high)) << 16 | u32::from(d.cluster_low),
                    size: d.size,
                    long_name: [0; 255] // long names not supported
                };

                // Offset is the PFSdirectory entry number that you are looking at
                self.offset = i + 1;
                return Ok(entry);
            }
            // Once you reach the final PFSdirectory entry of the sector, you move up one sector and set the offset to 0
            self.sector += 1;
            self.offset = 0;
        }
    }
    
    // TODO: support larger sizes (for placing long name directories)
    /// Walks the directory tables and finds an empty entry then returns a position where that entry can be placed.
    /// Currently does not support sizes larger than 1 entry.
    pub fn find_empty_or_grow(&self, fs: &Filesystem, size_needed: usize) -> Result<PFSPosition, Error> {
        if size_needed == 0 {
            return Err(Error::IllegalArgument);
        }
    
        if size_needed > 1 {
            warn!("find_empty_or_grow called with size larger than 1: {:}. Treating as 1", size_needed);
        }
        let size_needed = 1;
        
        let mut pos = self.initial_pos();
        
        loop {
            let dir: FatDirectory = self.get_fat_directory(fs, &pos)?;
            
            if dir.is_free() {
                return Ok(pos); // Pos is now set to the position of a free directory so we can continue.
            }
            
            pos = match self.advance_pos(fs, &pos) {
                Err(Error::EndOfFile) => return self.grow_directory(fs, &pos),
                Ok(new_pos) => new_pos,
                Err(e) => return Err(e),
            }
        }
    }
    
    // FIXME currently this is called by a function that locks the fs -> double lock?
    // I think the "best" solution is perhaps to make everything that's not part of the public interface pass the fs in as an argument.
    // Make FS an argument I suppose. Seems dumb though.
    /// Grows the directory given a PFSPosition in the last cluster of the file and returns a new position in the new_cluster.
    fn grow_directory(&self, fs: &Filesystem, pos_end: &PFSPosition) -> Result<PFSPosition, Error> {
        
        // Rename for convenience with other existing code.
        let pos = pos_end;
        
        // FIXME This code currently assumes that the size of the directory is always a multiple of cluster size.
        // I don't really know if this is something we can assume in a real filesystem but it's definitely something that any self respecting implementation would want to maintain.
        if self.size % fs.bytes_per_cluster() != 0 {
            warn!("Directory growth assumes size is a multiple of cluster size");
        }
        
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
            self.write_fat_directory(fs, &pos, &unused_entry)?;
            pos = match self.advance_pos(fs, &pos) {
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

    /// TODO should replace the entries function
    pub fn entries2(&self) -> Result<Vec<DirectoryEntry>, Error> {
        debug!("Getting entries for {:?}", self.name);
        let fs = self.filesystem.lock();

        let mut entry_collection: Vec<DirectoryEntry> = Vec::new(); 
        let mut pos = self.initial_pos();
        
        loop {
            let dir: FatDirectory = self.get_fat_directory(&fs, &pos)?;
            
            // TODO VFAT long name check needs to be done correctly.
            if !dir.is_free() && !dir.is_vfat() {
                debug!("Found new entry {:?}", debug_name(&dir.name));
                entry_collection.push(dir.to_directory_entry());
            }

            // If the first byte of the directory is 0 we have reached the end of allocated directory entries
            if dir.name[0] == 0 {
                return Ok(entry_collection);
            }
            
            pos = match self.advance_pos(&fs, &pos) {
                Err(Error::EndOfFile) => return Ok(entry_collection),
                Ok(new_pos) => new_pos,
                Err(e) => return Err(e),
            };
            debug!("New position: cluster: {}, sector_off: {}, byte off: {}", pos.cluster, pos.sector_offset, pos.entry_offset);
        }
    }

    /// Returns a vector of all the directory entries in a directory without mutating the directory
    pub fn entries(&self) -> Result<Vec<DirectoryEntry>, Error> {
         
        let fs = self.filesystem.lock();
        let mut cluster = self.cluster;
        let mut self_sector = self.sector;
        let mut entry_collection: Vec<DirectoryEntry> = Vec::new(); 

        loop {    
            // Looks at current cluster number of the directory and moves to the exact sector
            let sector = if cluster > 0 {
                // Identifies if the sector number is greater than the size of one cluster, and if it is
                // it will move on to the next cluster
                if self_sector >= fs.sectors_per_cluster {
                    match fs.next_cluster(cluster) {
                        Ok(new_cluster) => {
                            cluster = new_cluster;
                            self_sector = 0;
                        }
                        Err(_e) => {
                            return Ok(entry_collection);
                        }
                    }
                }
                self_sector
                    + fs.first_sector_of_cluster(cluster)
            } else {
                self_sector
            };

            let mut data: Vec<u8> = vec![0; fs.header.drive.lock().sector_size_in_bytes()];
            let _sectors_read = match fs.header.drive.lock().read_sectors(&mut data[..], (sector) as usize) {
                Ok(bytes) => bytes,
                Err(_) => return Err(Error::BlockError),
            };
            
            let mut counter = 32;
            let mut directory_entries: Vec<FatDirectory> = Vec::new(); 

            // This goes through the 8 possible directory entries in a FATdirectory sector 
            // A PFSdirectory entry is 32 bytes, so it simply jumps to the next PFSdirectory entry by adding 64 bytes to the counter
            for _entries in 0..8 { 
                // Turns the bytes of data into FATDirectory structures 
                let dir: FatDirectory = FatDirectory {
                    name: match data[(0+counter)..(11+counter)].try_into() {
                        Ok(array) => array,
                        Err(_) => return Err(Error::BlockError),
                    },
                    flags: data[11+counter],
                    _unused1: match data[(12+counter)..(20+counter)].try_into() {
                        Ok(array) => array,
                        Err(_) => return Err(Error::BlockError),
                    },
                    cluster_high: LittleEndian::read_u16(&mut data[(20+counter)..(22+counter)]) as u16,
                    _unused2: match data[(22+counter)..(26+counter)].try_into() {
                        Ok(array) => array,
                        Err(_) => return Err(Error::BlockError),
                    }, 
                    cluster_low: LittleEndian::read_u16(&mut data[(26+counter)..(28+counter)]) as u16,
                    size: LittleEndian::read_u32(&mut data[(28+counter)..(32+counter)]) as u32
                };
                counter += 64;
                directory_entries.push(dir);
            }

            let dirs: Vec<FatDirectory> = directory_entries;            

            // debug!("sector: {}", self.sector);
            // debug!("offset: {}", self.offset);

            // Loops through the PFSdirectory entries and creates the PFSDirectory Entry strcture based on the FatDirectory fields 
            for i in 0..dirs.len() {
                let d = &dirs[i];

                // When you reach the last PFSdirectory entry in a PFSdirectory
                if d.name[0] == 0x0 {
                    break;
                }
                // PFSDirectory unused
                if d.name[0] == 0xe5 {
                    continue;
                }

                let entry = DirectoryEntry {
                    name: d.name,
                    file_type: if d.flags & 0x10 == 0x10 {
                        FileType::PFSDirectory
                    } else {
                        FileType::PFSFile
                    },
                    cluster: (u32::from(d.cluster_high)) << 16 | u32::from(d.cluster_low),
                    size: d.size,
                    long_name: [0; 255] // long names not supported
                };

                entry_collection.push(entry); 
            }
            // Once you reach the final PFSdirectory entry of the sector, you move up one sector
            self_sector += 1;
        }
    }
    
}

/// Structure for the filesystem to be used to transverse the disk and run operations on the disk
pub struct Filesystem {
    header: Header, // This is meant to read the first 512 bytes of the virtual drive in order to obtain device information
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
    pub fn new(header: Header) -> Filesystem {
        debug!("filesystem started");
        let fs = Filesystem {
            header,
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
        fs
    }

    /// Reads the sector and fills in the filesystem fields using information from the specified fat headers
    pub fn init(mut self) -> Result<Filesystem, &'static str> {
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
        Ok(self)
    }

    /// This function allows you to jump to the data component when given a cluster number   
    fn first_sector_of_cluster(&self, cluster: u32) -> u32 {
        // The first sector of a cluster = first portion of data component + the amount of sectors/cluster + accounting for root cluster
        ((cluster - 2) * self.sectors_per_cluster) + self.first_data_sector
    }

    // TODO: this method seems like it should be renamed.
    /// Initializes a PFSDirectory strcuture based on the cluster that the PFSdirectory is stored in
    fn get_directory(&self, cluster: u32, name: String, parent: DirRef, fs: Arc<Mutex<Filesystem>>, size: u32) -> Result<PFSDirectory, Error> {
        // debug!("name of directory:{}", name);
        Ok(PFSDirectory {
            filesystem: fs,
            name: name,
            parent: Arc::downgrade(&parent),
            cluster: cluster,
            sector: 0,
            offset: 0,
            size: size,
        })
    }

    // TODO: rename this method.
    /// Initializes a PFSFile strcture based on the cluster that the PFSfile is stored as well as it's size
    fn get_file(&self, cluster: u32, size: u32, name: String, parent: DirRef, fs: Arc<Mutex<Filesystem>> ) -> Result<PFSFile, Error> {
        debug!("name of file:{}", name);
        Ok(PFSFile {
            filesystem: fs,
            name: name,
            parent: Arc::downgrade(&parent),
            start_cluster: cluster,
            size,
        })
    }

    // TODO: document the behavior for EOF encountered (since this is useful).
    /// Used to transverse to the next cluster of a PFSfile/PFSdirectory that spans multiple clusters by utlizing the FAT component
    fn next_cluster(&self, cluster: u32) -> Result<u32, Error> {
        match self.fat_type {
            32 => {
                // FAT32 uses 32 bits per FAT entry, so 1 entry = 4 bytes
                let fat_entry = Filesystem::fat_entry_from_cluster(cluster);
                let fat_sector = self.fat_sector_from_fat_entry(fat_entry);
                let entry_offset = fat_entry % self.bytes_per_sector;
                debug!("the current cluster is:{}, need to read sector: {}, offset from entry: {}, entry: {}", cluster, fat_sector, entry_offset, fat_entry);

                let mut data: Vec<u8> = vec![0; self.header.drive.lock().sector_size_in_bytes()];
                let _sectors_read = match self.header.drive.lock().read_sectors(&mut data[..], fat_sector as usize) {
                    Ok(bytes) => bytes,
                    Err(_) => return Err(Error::BlockError),
                };
    
                // Because FAT32 uses 4 bytes per cluster entry, to get to a cluster's exact byte entry in the FAT you can use the cluster number * 4
                // And to get to the next cluster, the FAT must be read in little endian order using the mutliple_hex_to_int function 
                // Read more here: https://en.wikipedia.org/wiki/Design_of_the_FAT_file_system#FAT
                let next_cluster_raw = LittleEndian::read_u32(&mut data[entry_offset as usize..(entry_offset as usize +size_of::<u32>())]) as u32;
                let next_cluster = cluster_from_raw(next_cluster_raw);

                let data_test = &data[entry_offset as usize..(entry_offset as usize +size_of::<u32>())];

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
    fn find_empty_cluster(&self) -> Result<u32,Error>{
        
        // Magic number: first cluster that might not be root directory.
        let mut cluster = 3;
        
        
        while cluster < self.clusters {
            let fat_entry = Filesystem::fat_entry_from_cluster(cluster);
            let fat_sector = self.fat_sector_from_fat_entry(fat_entry);
            // Fetch necessary fat_sector
            let mut data: Vec<u8> = vec![0; self.bytes_per_sector as usize];
            let _sectors_read = match self.header.drive.lock().read_sectors(&mut data[..], fat_sector as usize) {
                    Ok(bytes) => bytes,
                    Err(_) => return Err(Error::BlockError),
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
    fn extend_chain(&self, old_tail: u32) -> Result<u32, Error> {
        // Fetch the FAT for the table entry and check if next entry is free.
        let fat_entry = Filesystem::fat_entry_from_cluster(old_tail);
        let fat_sector = self.fat_sector_from_fat_entry(fat_entry);
        let mut data: Vec<u8> = vec![0; self.bytes_per_sector as usize];
        let _sectors_read = match self.header.drive.lock().read_sectors(&mut data[..], fat_sector as usize) {
                Ok(bytes) => bytes,
                Err(_) => return Err(Error::BlockError),
        };
        
        // Verify that the current position is indeed EOC
        let entry_offset = fat_entry % (self.bytes_per_sector / size_of::<u32>() as u32);
        let cluster_value = LittleEndian::read_u32(
            &mut data[entry_offset as usize..(entry_offset as usize +size_of::<u32>())]) as u32;
        let cluster_value = cluster_from_raw(cluster_value);
            
        if !is_eoc(cluster_value) {
            warn!("Tried to extend chain without end of cluster. Found {:x} at cluster {:x}", cluster_value, old_tail);
            return Err(Error::IllegalArgument);
        }
        
        let mut cluster_found = false;
        
        // TODO this sort of logic could easily be written with a PFSPosition struct?
        // In general this code section is pretty ugly. Could be refactored.
        
        // See if we can trivially extend the chain?
        let mut new_cluster_candidate = old_tail + 1;
        let mut new_fat_entry = Filesystem::fat_entry_from_cluster(new_cluster_candidate);
        let mut new_fat_sector = self.fat_sector_from_fat_entry(new_fat_entry);
        let mut new_entry_offset = new_fat_entry % (self.bytes_per_sector / size_of::<u32>() as u32);
        
        // TODO also check in the case where the FAT is not split across a sector boundary.
        if new_fat_sector == fat_sector {
            let next_cluster_value = LittleEndian::read_u32(
                &mut data[new_entry_offset as usize..(new_entry_offset as usize +size_of::<u32>())]) as u32;
            let next_cluster_value = cluster_from_raw(next_cluster_value);
                
            if is_empty_cluster(next_cluster_value) {
                cluster_found = true;
            }
        }
        
        // Handle the case where we need to walk the FAT to find a free cluster:
        if !cluster_found {
            new_cluster_candidate = self.find_empty_cluster()?;
            // Recompute the fat entry,sector etc:
            new_fat_entry = Filesystem::fat_entry_from_cluster(new_cluster_candidate);
            new_fat_sector = self.fat_sector_from_fat_entry(new_fat_entry);
            new_entry_offset = new_fat_entry % (self.bytes_per_sector / size_of::<u32>() as u32);
        }
        
        // TODO the order may affect correctness in cases where disk accesses fail. Consider this at some point.
        // FIXME this code would also greatly benefit from byte granularity transactions
        // Update the disk segments and write to disk:
        LittleEndian::write_u32(
            &mut data[entry_offset as usize..(entry_offset as usize +size_of::<u32>())], new_cluster_candidate);
        if new_fat_sector == fat_sector {
            LittleEndian::write_u32(
                &mut data[new_entry_offset as usize..(new_entry_offset as usize +size_of::<u32>())], 
                EOC);
                
            // Write the data out:
            let _sectors_written = match self.header.drive.lock().write_sectors(&data, fat_sector as usize) {
                Ok(sectors) => sectors,
                Err(_) => return Err(Error::BlockError),
            };
        } else {
            // Write the old out:
            let _sectors_written = match self.header.drive.lock().write_sectors(&data, fat_sector as usize) {
                Ok(sectors) => sectors,
                Err(_) => return Err(Error::BlockError),
            };
            
            // Read the new sector in
            let _sectors_read = match self.header.drive.lock().read_sectors(&mut data[..], new_fat_sector as usize) {
                Ok(bytes) => bytes,
                Err(_) => {
                    warn!("Fat32 disk is in an inconsistent state");
                    return Err(Error::BlockError);
                },
            };
            
            // Overwrite the old entry
            LittleEndian::write_u32(
                &mut data[new_entry_offset as usize..(new_entry_offset as usize +size_of::<u32>())], 
                EOC);
                
            // Write the old out:
            let _sectors_written = match self.header.drive.lock().write_sectors(&data, new_fat_sector as usize) {
                Ok(sectors) => sectors,
                Err(_) => {
                    warn!("Fat32 disk is in an inconsistent state");
                    return Err(Error::BlockError);
                },
            };
        }
        
        Ok(new_cluster_candidate)
    }
    
    #[inline]
    fn bytes_per_cluster(&self) -> u32 {
        return self.bytes_per_sector * self.sectors_per_cluster;
    }
    
    #[inline]
    /// Computes the fat entry in bytes in the FAT corresponding to a given cluster.
    fn fat_entry_from_cluster(cluster: u32) -> u32 {
        cluster * 4
    }
    
    #[inline]
    /// Computes the disk sector corresponding to a given fat entry to read the FAT.
    fn fat_sector_from_fat_entry(&self, fat_entry: u32) -> u32 {
        self.first_fat_sector + (fat_entry / self.bytes_per_sector)
    }

    fn sectors_per_cluster(&self) -> u32 {
        self.sectors_per_cluster
    }
    
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
        let header = match Header::new(sd){
            Ok(header) => header,
            Err(_) => return Err("failed to intialize header"),
        };
        let fatfs = Filesystem::new(header);
        match fatfs.init() {
            Ok(fs) => {
                // return Ok(Arc::new(Mutex::new(fs)));
                return Ok(fs);
            } 
            Err(_) => return Err("failed to intialize fat filesystem for disk"),
        }           
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
    
    let mut initial_buf: Vec<u8> = vec![0; disk.lock().sector_size_in_bytes()];
    let _sectors_read = match disk.lock().read_sectors(&mut initial_buf[..], 0){
        Ok(bytes) => bytes,
        Err(_) => return false,
    };
    info!("Magic sequence: {:X?}", &initial_buf[82..90]);
    // The offset at 0x52 in the extended FAT32 BPB is used to detect the Filesystem type ("FAT32   ")
    match initial_buf[82..90] {
        [0x46,0x41,0x54,0x33,0x32, 0x20, 0x20, 0x20] => return true,
        _ => return false,
    };    
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

/// Creates a FATdirectory structure for the root directory
/// 
/// # Arguments 
/// * `fs`: the filesystem structure wrapped in an Arc and Mutex
/// 
/// # Returns
/// Returns the FATDirectory structure for the root directory 
pub fn root_dir(fs: Arc<Mutex<Filesystem>>) -> Result<RootDirectory, Error> {

    let fatdir = PFSDirectory {
        filesystem: fs.clone(),
        name: "/".to_string(),
        parent: Weak::<Mutex<PFSDirectory>>::new(),
        cluster: 2,
        sector: 0,
        offset: 0,
        size: 0, // TODO this is wrong but this method also just doesn't make much sense because root should be backed by disk.
    };

    let underlying_root = Arc::new(Mutex::new(fatdir));

    // fs.lock();
    let new_root_dir = RootDirectory {
        underlying_dir: underlying_root.clone(),
    };

    // This is pretty weird, but it seems to be necessary to set our own parent as ourselves.
    underlying_root.lock().parent = Arc::downgrade(&(underlying_root.clone() as Arc<Mutex<dyn Directory + Send>>));

    Ok(new_root_dir) 
        
}

// TODO this is a pretty dumb setup but it seems to work for now. I'll try to work out a better solution later.
/// A struct and impl being used to represent the parent of the root directory in FAT, taken from the root kernel
pub struct RootDirectory {
    /// A list of DirRefs or pointers to the child directories   
    underlying_dir: Arc<Mutex<PFSDirectory>>, // TODO should I do something for the parent.
}

impl Directory for RootDirectory {
    fn insert(&mut self, node: FileOrDir) -> Result<Option<FileOrDir>, &'static str> {
        self.underlying_dir.lock().insert(node)
    }

    fn get(&self, name: &str) -> Option<FileOrDir> {
        self.underlying_dir.lock().get(name)
    }

    fn list(&self) -> Vec<String> {
        self.underlying_dir.lock().list()
    }

    fn remove(&mut self, node: &FileOrDir) -> Option<FileOrDir> {
        self.underlying_dir.lock().remove(node)
    }
}

// TODO these are most likely wrong implementations.
impl FsNode for RootDirectory {
    /// Recursively gets the absolute pathname as a String
    fn get_absolute_path(&self) -> String {
        format!("{}/", root::ROOT_DIRECTORY_NAME.to_string()).to_string()
    }

    fn get_name(&self) -> String {
        self.underlying_dir.lock().get_name()
    }

    // TODO I had to break this since the old implementation used the root from .
    // I think during the creation of the root directory we'll want to save a DirRef to it?
    // Could be another field I suppose.
    // Maybe the best solution is to have root_dir return a DirRef and then have the root dir hold onto a weak dir Ref to itself.
    // That sounds dumb but it might just work.
    /// we just return the root itself because it is the top of the filesystem.
    fn get_parent_dir(&self) -> Option<DirRef> {
        None
    }

    fn set_parent_dir(&mut self, _: WeakDirRef) {
        // do nothing
    }
}

/// Function to print the raw byte name as a string. TODO find a better way.
fn debug_name(byte_name: &[u8]) -> &str {
    match core::str::from_utf8(byte_name) {
        Ok(name) => name,
        Err(_) => "Couldn't find name"
    }
}