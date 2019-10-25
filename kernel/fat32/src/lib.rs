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
//! #[macro_use] extern crate terminal_print;
//! #[macro_use] extern crate log;
//! 
//! extern crate device_manager;
//! extern crate fat32;
//! extern crate ata;
//! extern crate storage_device;
//! extern crate spin;
//! extern crate fs_node;
//! extern crate path;
//! extern crate alloc;
//! 
//! use alloc::vec::Vec;
//! use alloc::string::String;
//! use alloc::string::ToString;
//! use alloc::sync::{Arc};
//! use spin::Mutex;
//! use fs_node::{FileOrDir, DirRef};
//! use fat32::{root_dir};
//! use path::Path;
//! 
//! #[no_mangle]
//! pub fn main(_args: Vec<String>) -> isize {
//!     
//!     // Itereate through list of storage devices to find a storage device with a FAT32 filesystem.
//!     if let Some(controller) = storage_manager::STORAGE_CONTROLLERS.lock().iter().next() {
//!         for sd in controller.lock().devices() {
//!             match fat32::init(sd) {
//!                 Ok(fatfs) => {
//!                     let fs = Arc::new(Mutex::new(fatfs));
//! 
//!                     // Since the root_dir function is meant to support creating a RootDirectory for a potential mount point
//!                     // we can give it a name. For now
//!                     let name = "/".to_string();
//! 
//!                     // Since this root is a DirRef, the read-only parts of the FsNode API works for the fat32 crate.
//!                     let fat32_root: DirRef = root_dir(fs.clone(), &name).unwrap();
//! 
//!                     // Note that FAT32 is typically case insensitive with case preserving rules.
//!                     // We'll try to read from this file (on the test disk).
//!                     let path_to_get = Path::new("bigfile/BIGFILE".to_string());
//! 
//!                     // Remember that the path crate only recognizes the system root so we must get relative to our root.
//!                     let file_or_dir = match path_to_get.get(&fat32_root) {
//!                         Some(file_or_dir) => file_or_dir,
//!                         None => {
//!                             warn!("bigfile/BIGFILE not found in fat32 ");
//!                             return 1;
//!                         }
//!                     };
//! 
//!                     let big_file = match file_or_dir {
//!                         FileOrDir::File(f) => {
//!                             f
//!                         },
//!                         FileOrDir::Dir(_) => {
//!                             warn!("bigfile/BIGFILE was a directory. Expected file.");
//!                             return 1;
//!                         }
//!                     };
//! 
//!                     let mut buf = [0; 16];
//!                     let bytes_read = match big_file.lock().read(&mut buf, 0) {
//!                         Ok(bytes) => bytes,
//!                         Err(e) => {
//!                             warn!("Disk read failed: {:?}", e);
//!                             return 1;
//!                         },
//!                     };
//! 
//!                     println!("Read {} bytes. Found text: {:?}", bytes_read, 
//!                         String::from_utf8(buf.to_vec()).unwrap_or("INVALID_UTF8".to_string()));
//!                     return 0;
//!                 }
//!                 
//!                 Err(_) => {
//!                     // Don't need to do anything because not all storage devices contain a FAT32 FS.
//!                     1;
//!                 }
//!             }
//!         }
//!     }
//!     warn!("No valid FAT32 filesystems found.");
//!     0
//! }
//! ```
//! 

// TODO cat seems work weirdly with the bigfile example I've got -> doesn't print the last few characters.
// ^ Should make some different examples to test with.

// TODO the docs at the top of this crate aren't all that adviseable at this point. I'll try to make some fixes soon.

#![no_std]
#![feature(slice_concat_ext)]
#![feature(option_flattening)]

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
use zerocopy::{FromBytes, AsBytes, LayoutVerified, ByteSlice};
use zerocopy::byteorder::{U16, U32};
use byteorder::{ByteOrder, LittleEndian};
use alloc::collections::BTreeMap;
use spin::{Mutex, RwLock};
use fs_node::{DirRef, WeakDirRef, FileRef, Directory, FileOrDir, File, FsNode};
use alloc::sync::{Arc, Weak};
use core::convert::TryInto;
use core::mem::size_of;
use core::cmp::{Ord, Ordering, min};
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use memory::MappedPages;
use block_io::BlockIo;
use spawn::{KernelTaskBuilder};
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

// TODO enforce more fo these (primarily ensure things are files before making PFSFile)
/// Directory entry flags. Mostly ignored in this implementation.
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
    InconsistentDisk,
}

// Likely we'll want to add some sort of entry for non-file or directory types. TODO
/// Indicates whether the PFSDirectory entry is a PFSFile or a PFSDirectory
#[derive(Debug, PartialEq)]
enum FileType {
    PFSFile,
    PFSDirectory,
}

/// Internal Directory storage type
type ChildList = BTreeMap<DiskName, FileOrDir>;
type ShortName = [u8; 11];
type FSRef = Arc<Mutex<Filesystem>>;

// May be better to replace the Vec with an array of extensions?
// TODO: May want a method to validate the LongName struct too.
/// Generalized name struct that can be filled from and written onto disk easily.
struct LongName {
    short_name: ShortName,
    long_name: Vec<[u16; 13]>, // FIXME this is probably a wrong size for VFAT extension.
}

impl LongName {
    /// Construct a new long name from a sequence of on disk directory entries.
    fn new(entries: Vec<&RawFatDirectory>) -> Result<LongName, Error> {
        let mut long_name = LongName::empty();
        // Last entry must contain the short_name bytes.
        let last = entries[entries.len() - 1];
        if last.is_vfat() {
            // VFAT long name extension entries do not contain the short name.
            // As such this is an illegal sequence of directories.
            return Err(Error::InconsistentDisk)
        }

        long_name.short_name = last.name;

        Ok(long_name)
    }

    fn empty() -> LongName {
        LongName {
            short_name: [0; 11],
            long_name: vec!(),
        }
    }
}

/// Subset of strings which are checked to be legal to write to disk.
#[derive(Clone)]
#[derive(Debug)]
pub struct DiskName {
    pub name: String,
}

impl DiskName {
    /// Convert to a string that compares consistently for 
    fn to_comp_name(&self) -> String {
        self.name.to_uppercase()
    }

    fn from_string(name: &str) -> Result<DiskName, &'static str> {
        // FIXME validate string names:
        if name.is_empty() {
            return Err("Name cannot be empty");
        }

        // FIXME properly strip trailing spaces:
        Ok(DiskName {
            name: name.trim_end().to_string()
        })
    }

    fn from_long_name(long_name: LongName) -> Result<DiskName, &'static str> {
        Err("Unsupported")
    }

    // TODO support constructing the long_name field using a UCS-2 encoding.
    // ALSO TODO if we construct the canonical short name, it depends on the number of
    // duplicates on disk, so we'll need to figure that out somehow, which is quite
    // a bit outside of the scope of how we're currently doing things.
    // For now I'm probably going to just make things ~1 for all short names if truncated.
    // Another option is to manually modify the long name after construction to account for
    // the number of aliases? Seems less awful, but leads to some less idiomatic code.
    // I think that the most effective solution is to either set a flag or simply
    // check that the long_name field is not empty and then overwrite the short name
    // before interacting with disk based on the number of duplicates.
    fn to_long_name(&self) -> LongName {
        let mut long_name = LongName::empty();

        // Check for . and .. entries separately


        //let bytes_to_copy = min(long_name.short_name.len(), name_bytes.len());
        //long_name.short_name[0..bytes_to_copy].copy_from_slice(&name_bytes[0..bytes_to_copy]);
        // Construct the canonical short name:
        long_name.short_name = match self.name.rsplitn(1, |x| x == '.').collect::<Vec<&str>>()[0..2] {
            [name_without_extension] => {
                let mut short_name: [u8; 11] = *b"           ";
                let num_bytes_to_copy = min(8, name_without_extension.as_bytes().len());
                short_name[0..num_bytes_to_copy].copy_from_slice(&name_without_extension.as_bytes()[0..num_bytes_to_copy]);
                short_name[8..11].copy_from_slice(b"   ");
                short_name
            },
            [base, extension]        => {
                let mut short_name: [u8; 11] = *b"           ";
                let num_bytes_to_copy = min(8, base.as_bytes().len());
                short_name[0..num_bytes_to_copy].copy_from_slice(&base.as_bytes()[0..num_bytes_to_copy]);

                let num_bytes_to_copy = min(3, extension.as_bytes().len());
                short_name[8..(8+num_bytes_to_copy)].copy_from_slice(&extension.as_bytes()[0..num_bytes_to_copy]);

                short_name
            },
            // Cases that should not be possible given behavior for rsplitn
            _ => {
                error!("fat32::DiskName::to_long_name encountered unexpected return from rsplitn");
                *b"           "
            }
        };

        // TODO encode the name into UCS-2 and split into small buffers
        // to construct the UCS-2 encoded long name.
        long_name
    }
}

impl PartialEq for DiskName {
    fn eq(&self, other: &Self) -> bool {
        self.to_comp_name().eq(&other.to_comp_name())
    }
}

impl Eq for DiskName {}

impl Ord for DiskName {
    fn cmp(&self, other: &Self) -> Ordering {
        self.to_comp_name().cmp(&other.to_comp_name())
    }
}

impl PartialOrd for DiskName {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.to_comp_name().partial_cmp(&other.to_comp_name())
    }
}

impl ToString for DiskName {
    fn to_string(&self) -> String {
        self.name.clone()
    }
}

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

// TODO I'm not sure if there's a better way to do this, but it seems to work.
/// A struct used to read RawFatDirectory directly from bytes safely.
struct RawFatDirectoryBytes<B> {
    fat_directory: LayoutVerified<B, RawFatDirectory>,
}

impl<B: ByteSlice> RawFatDirectoryBytes<B> {
    pub fn parse(bytes: B) -> Option<RawFatDirectoryBytes<B>> {
        let fat_directory = LayoutVerified::new(bytes)?;

        Some(RawFatDirectoryBytes{fat_directory})
    }
}

#[derive(Clone)] // FIXME this shouldn't be necessary, but our current API makes it necessary.
                 // It seems like the only way to tranfer ownership of a RawFatDirectory made from the raw
                 // bytes is to transfer ownership of the bytes. However this would reqire significant
                 // refactoring.
#[repr(packed)]
#[derive(FromBytes, AsBytes)]
/// A raw 32-byte FAT directory as it exists on disk.
struct RawFatDirectory {
    name: [u8; 11], // 8 letter entry with a 3 letter ext
    flags: u8, // designate the PFSdirectory as r,w,r/w
    _unused1: [u8; 8], // unused data
    cluster_high: U16<LittleEndian>, // but contains permissions required to access the PFSfile
    _unused2: [u8; 4], // data including modified time
    cluster_low: U16<LittleEndian>, // the starting cluster of the PFSfile
    size: U32<LittleEndian>, // size of PFSfile in bytes, volume label or subdirectory flags should be set to 0
}

impl RawFatDirectory {    
    fn to_directory_entry(&self) -> Result<DirectoryEntry, Error> {
        let name = String::from_utf8(self.name.to_vec()).map_err(|_| Error::IllegalArgument)?;
        let name = DiskName::from_string(&name).map_err(|_| Error::IllegalArgument)?;

        Ok(DirectoryEntry {
            name,
            file_type: if self.flags & FileAttributes::SUBDIRECTORY.bits == FileAttributes::SUBDIRECTORY.bits {
                FileType::PFSDirectory
            } else {
                FileType::PFSFile
            },
            cluster: (self.cluster_high.get() as u32) << 16 |(self.cluster_low.get() as u32),
            size: self.size.get(),
        })
    }
    
    fn make_unused() -> RawFatDirectory {
        RawFatDirectory {
            name: [FREE_DIRECTORY_ENTRY; 11],
            flags: 0,
            _unused1: [0; 8],
            cluster_high: U16::new(0),
            _unused2: [0; 4],
            cluster_low: U16::new(0),
            size: U32::new(0),
        }
    }

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
}

// REVIEW: I think at some point this type needs a mapping to and from ClusterChain
// but once that happens it almost seems like this type might be a bit redundant.
// In practice the only information this struct doesn't need is some of the caching that CC contains.
// So I'm not realy sure.

/// Based on the FatDirectory structure and used to describe the files/subdirectories in a FAT32 
/// formatted filesystem, used for convience purposes
pub struct DirectoryEntry {
    pub name: DiskName,
    file_type: FileType,
    pub size: u32,
    cluster: u32,
}

// TODO these methods need to potentially generate a sequence of entries.
impl DirectoryEntry {    
    // FIXME method has wrong implementation for long names on disk.
    // Once DiskName::to_long_name is preopery done then this will be pretty straightforward.
    // I think what we can do is rename this method to convert a Directory Entry into
    // the on disk Directory entry that isn't used for the VFAT long name and add another method
    // to deal with the long name.
    /// Converts this directory entry to the RawFatDirectory that will represent this object on disk.
    /// Note that this function will not return the sequence of entries used for the VFAT long name.
    fn to_raw_fat_directory(&self) -> RawFatDirectory {

        let long_name = self.name.to_long_name();

        RawFatDirectory {
            name: long_name.short_name,
            flags: match self.file_type {
                FileType::PFSFile => 0x0,
                FileType::PFSDirectory => 0x10,
            }, // FIXME need to store more flags in DirectoryEntry type
            _unused1: [0; 8],
            cluster_high: U16::new((self.cluster >> 16) as u16),
            _unused2: [0; 4],
            cluster_low: U16::new((self.cluster) as u16),
            size: U32::new(self.size)
        }
    }
}

/// A wrapper around a FAT32 file object.
pub struct PFSFile {
    cc: ClusterChain,
}

impl PFSFile {
    // TODO I don't remember if this method works.
    // It isn't used and doesn't really serve any purpose right now since PFSPosition isn't a part
    // of the rest of the public facing API I think and the relevant logic is used elsewhere.
    /// Converts a position in a file into a current_cluster-sector_offset-byte_offset struct.
    pub fn seek(&self, fs: &mut Filesystem, offset: usize) -> Result<PFSPosition, &'static str> {

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
        let mut bytes_copied = 0;

		for cluster_offset in range { // Number of clusters into the file
            // Jump to current cluster:
            current_cluster = self.cc.cluster_advancer(&mut fs, cluster_offset)
                .map_err(|_| "Read failed")?;

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
        self.cc.name.to_string().clone()
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
    /// Sector offset into the current cluster
    sector_offset: usize, 
    /// Byte offset into the current sector
    entry_offset: usize, 
}

/// Structure for a FAT32 directory.
pub struct PFSDirectory {
    /// Underlying strucuture on disk containing the Directory data. 
    cc: ClusterChain,
    // TODO: children should maybe incude some directory entry like information?
    /// Potentially incomplete list of children.
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
        let name = DiskName::from_string(name).ok()?;

        debug!("PFSDirectory::get called for {:?}", name.to_comp_name());

        match name.name.as_str() {
            // 
            "." => self.dot.upgrade().map(|x| FileOrDir::Dir(x)),
            ".." => self.cc.parent.upgrade().map(|x| FileOrDir::Dir(x)),
            _ => self.fat32_get(&name).ok(),
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
                return children.keys().map(|x| x.name.clone()).collect::<Vec<String>>();
            }

        }

        let mut entries = children.keys().map(|x| x.name.clone()).collect::<Vec<String>>();
        entries.push(".".to_string());
        entries.push("..".to_string());
        entries
    }

    fn remove(&mut self, _node: &FileOrDir) -> Option<FileOrDir> {
        Option::None
    }
}

impl FsNode for PFSDirectory {
    fn get_name(&self) -> String {
        self.cc.name.to_string().clone()
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
        // FIXME warn in case of invalid disk name
        let name = name.map(|name| DiskName::from_string(name).ok()).flatten();
        
        loop {
            let dir = self.get_fat_directory(fs, &pos)?;
            //debug!("got fat directory");
            
            // TODO VFAT long name check needs to be done correctly.
            if !dir.is_free() && !dir.is_vfat() && !dir.is_dot() { // Don't add dot directories. Must be handled specially. FIXME
                debug!("Found new entry {:?}", debug_name(&dir.name));
                let entry = dir.to_directory_entry()?;

                // While we're here let's add the entry to our children.
                self.add_directory_entry(fs, children, &entry)?;

                match &name {
                    Some(name) => {
                        if name == &entry.name {
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
            
            // FIXME this method is now broken. I think advance pos is just unfixable
            // unless we path a PFS_POS to also include a cluster offset.
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
        let name = &entry.name;

        match children.get(name) {
                Some(node) => return Ok(node.clone()),
                None => {},
        };

        // Create FileOrDir and insert:
        let cc = ClusterChain::new(entry.cluster, self.cc.filesystem.clone(), 
            name.clone(), self.dot.clone(), entry.size);
        // let cc = ClusterChain {
        //     cluster: entry.cluster,
        //     _num_clusters: None,
        //     name: name.clone(), // FIXME this is wrong.
        //     parent_count: 1, // FIXME not true for directories. Should rename so that this is fine.
        //     filesystem: self.cc.filesystem.clone(),
        //     parent: self.dot.clone(),
        //     size: entry.size,
        // };

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
                children.insert(name.clone(), FileOrDir::Dir(dir_ref.clone()));

                return Ok(FileOrDir::Dir(dir_ref));
            }
            
            FileType::PFSFile => {
                
                let file = PFSFile {
                    cc: cc,
                };

                let file_ref = Arc::new(Mutex::new(file));
                children.insert(name.clone(), FileOrDir::File(file_ref.clone()));

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
            let dir = self.get_fat_directory(fs, &pos)?;
            
            // TODO VFAT long name check needs to be done correctly.
            if !dir.is_free() && !dir.is_vfat() {
                debug!("Found new entry {:?}", debug_name(&dir.name));
                // FIXME also don't just fail if we can't make an entry
                entry_collection.push(dir.to_directory_entry()?);
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

    /// Returns the entry indicated by PFSPosition.
    /// the function is used and returns EndOfFile if the next PFSdirectory doesn't exist
    /// Note that this function will happily return unused directory entries since it simply provides a sequential view of the entries on disk.
    fn get_fat_directory(&self, fs: &mut Filesystem, pos: &PFSPosition) -> Result<RawFatDirectory, Error> {
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
        
        let fat_dir = RawFatDirectoryBytes::parse(&buf[..]).ok_or(Error::BlockError)?;
        
        Ok(fat_dir.fat_directory.clone())
    }

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
            let dir = self.get_fat_directory(fs, &pos)?;
            
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

    /// Internal get function.
    fn fat32_get(&self, name: &DiskName) -> Result<FileOrDir, Error> {

        match self.children.read().get(name) {
            Some(child) => return Ok(child.clone()),
            None => {},
        };

        let mut fs = self.cc.filesystem.lock();

        // FIXME: this can be written to only walk the directory once (and also add entries to children that aren't the ones we needed).
        let entries = self.entries(&mut fs)?;
        
        for entry in entries {
            // Compares the name of the next entry in the current directory
            if name == &entry.name {
                // Get a write lock for the children and check if it was found before creating a new object (since we dropped the read lock).
                let mut children = self.children.write();
                match children.get(&name) {
                    Some(child) => return Ok(child.clone()),
                    None => {},
                };

                let cc = ClusterChain::new(entry.cluster, self.cc.filesystem.clone(), entry.name, self.dot.clone(), entry.size);
                // Found the entry on disk. Need to create a new cluster chain for the object:
                // let cc = ClusterChain {
                //     cluster: entry.cluster,
                //     _num_clusters: None,
                //     name: name.clone(),
                //     parent_count: 1, // FIXME not true for directories. Should rename so that this represents a "parent count"
                //     filesystem: self.cc.filesystem.clone(),
                //     parent: self.dot.clone(),
                //     size: entry.size,
                // };

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
                        children.insert(name.clone(), FileOrDir::Dir(dir_ref.clone()));

                        return Ok(FileOrDir::Dir(dir_ref));
                    }
                    
                    FileType::PFSFile => {
                        
                        let file = PFSFile {
                            cc: cc,
                        };

                        let file_ref = Arc::new(Mutex::new(file));
                        children.insert(name.clone(), FileOrDir::File(file_ref.clone()));

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
//#[derive(Clone)]
struct ClusterChain {
    filesystem: Arc<Mutex<Filesystem>>,
    cluster: u32,
    pub name: DiskName,
    pub parent: WeakDirRef, // the directory that holds the metadata for this directory.
    parent_count: usize, // Number of references on disk. For a file must always be one. For a directory this is variable.
    _num_clusters: Option<u32>, // Unknown without traversing FAT table. I consider this useful information, but I'm not sure if I'll ever use it.
    pub size: u32,
    cluster_cache: Mutex<Vec<u32>>, // Cache of cluster offset to on_disk clusters. REVIEW: if we allow shrinking a file this is sketchy but this isn't legal as best I can tell.
}

impl ClusterChain {
    pub fn new(cluster: u32, fs: FSRef, name: DiskName, parent: WeakDirRef, size: u32) -> ClusterChain {
        ClusterChain {
            filesystem: fs,
            cluster,
            name,
            parent: parent.clone(),
            parent_count: parent.upgrade().map(|_| 1).unwrap_or(0), // REVIEW this seems sketchy, but I think it's fair to do.
                                    // Downside is that this approach obscures the fact that a parent ref could in theory be dropped
                                    // while the disk reference still exists. Maybe passing an Option<DirRef> would be better.
                                    // However if the parent is dropped it causes other severe issues for the FS and I think 
                                    // that our FS structure cannot possibly be in a valid state if the parent has been dropped.
            _num_clusters: None,
            size,
            cluster_cache: Mutex::new(vec![cluster]),
        }
    }

    /// Returns a cluster walk position for the start of this CC.
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
            new_pos.cluster = self.cluster_advancer(fs, cluster_advance)?;
            new_pos.sector_offset = new_pos.sector_offset % fs.sectors_per_cluster as usize;
        }
        
        Ok(new_pos)
    }

    /// Advance "cluster_advance" number of clusters.
    /// Acquires lock for an on-disk cache of cluster positions.
    pub fn cluster_advancer(&self, fs: &mut Filesystem, cluster_advance: usize) -> Result<u32, Error> {

        // IMPROVEMENT: in theory we could only lock the tail of the cluster cache, but I'm not sure
        // about the best way to do that in Rust and it's not a critical improvment.
        let mut cluster_cache = self.cluster_cache.lock();

        let mut counter = min(cluster_advance, cluster_cache.len() - 1); // Guaranteed to be at least 1.
        let mut current_cluster = cluster_cache[counter]; // Guaranteed to exist since we're bounded by len.


        while cluster_advance != counter {
            current_cluster = fs.next_cluster(current_cluster)?;
            cluster_cache.push(current_cluster);
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


// FIXME need to move some of this logic into the DiskName type.
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
// pub fn compare_name(name: &str, de: &DirectoryEntry) -> bool {
//     compare_short_name(name, de) || &de.long_name[0..name.len()] == name.as_bytes()
// }

// fn compare_short_name(name: &str, de: &DirectoryEntry) -> bool {
//     // 8.3 (plus 1 for the separator)
//     if name.len() > 12 {
//         return false;
//     }

//     let mut i = 0;
//     for (_, a) in name.as_bytes().iter().enumerate() {
//         // Handle cases which are 11 long but not 8.3 (e.g "loader.conf")
//         if i == 11 {
//             return false;
//         }

//         // Jump to the extension
//         if *a == b'.' {
//             i = 8;
//             continue;
//         }

//         let b = de.name[i];
//         if a.to_ascii_uppercase() != b.to_ascii_uppercase() {
//             return false;
//         }

//         i += 1;
//     }
//     true
// }

// TODO is this mount name argument dumb? It seems like we want a semi-arbitrary name to support names for mount purposes.
/// Creates a FATdirectory structure for the root directory
/// 
/// # Arguments 
/// * `fs`: the filesystem structure wrapped in an Arc and Mutex
/// * `mount_name`: the name to used to refer to the new entry. Set to / if not mounting.
/// 
/// # Returns
/// Returns the FATDirectory structure for the root directory 
pub fn root_dir(fs: Arc<Mutex<Filesystem>>, mount_name: &str) -> Result<Arc<Mutex<RootDirectory>>, &'static str> {

    // TODO right now this function can violate the singleton blahdy blah and risks making two cluster chains for this dir.
    // We also can't just use something like a lazy static, since it's a per-FS basis. Maybe the FS needs some information to prevent this.
    let new_name = DiskName::from_string(mount_name)?;
    let mut cc = ClusterChain::new(2, fs.clone(), new_name, 
        Weak::<Mutex<PFSDirectory>>::new(), 0);
    cc.parent_count = 2; // TODO what number to choose. We definitely don't want the on disk
                         // resources getting dropped
    // let cc = ClusterChain {
    //     filesystem: fs.clone(),
    //     cluster: 2,
    //     name: DiskName::from_string(mount_name)?,
    //     parent: Weak::<Mutex<PFSDirectory>>::new(),
    //     parent_count: 2 + 1, // TODO should this be the case?
    //     _num_clusters: None,
    //     size: 0, // According to wikipedia File size for directories is always 0.
    // };

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
    // As such we have to bypass any sort of on-disk changes that might be used from a
    // set parent dir method in cc.
    // TODO make cc parent dir method that works well for this.
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
    KernelTaskBuilder::new(test_insert, None)
        .name("fat32_insert_test".to_string())
        .spawn()?;

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


                    let fat32_root = root_dir(fs.clone(), name.clone()).unwrap();


                    let true_root_ref = root::get_root();
                    let mut true_root = true_root_ref.lock();

                    match true_root.insert(FileOrDir::Dir(fat32_root.clone())) {
                        Ok(_) => trace!("Successfully mounted fat32 FS"),
                        Err(_) => trace!("Failed to mount fat32 FS"),
                    };
                    
                    fat32_root.lock().set_parent_dir(Arc::downgrade(&true_root_ref));


                    // Now let's try a couple simple things:
                    //let test_root = true_root.get_dir(&name).unwrap();
                    //debug!("Root directory entries: {:?}", test_root.lock().list());    

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