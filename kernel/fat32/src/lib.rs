//! Support for the FAT32 Filesystem.
//! Inspired by the [intel hypervisor firmware written in rust](https://github.com/intel/rust-hypervisor-firmware/)
//! 
//! Below is a an example of detecting a fat storage, initializing the file system and doing open and read operations on a file.
//! 
//! ```rust
//!     let fatfs = fat32::init().unwrap();
//!     
//!     /*
//!     // Reaches the root dir and able to go through each of the entries in the root folder using the next_entry function
//!     let mut root_dir = fatfs.root().unwrap();
//!     let de = root_dir.next_entry().unwrap();
//!     println!("the name of the next entry is: {:?}", de.name);
//!     println!("the name of the next_entry is: {:?}", core::str::from_utf8(&de.name));
//!
//!    let de = root_dir.next_entry().unwrap();
//!    println!("the name of the next entry is: {:?}", de.name);
//!    println!("the name of the next_entry is: {:?}", core::str::from_utf8(&de.name));
//!
//!    let de = root_dir.next_entry().unwrap();
//!    println!("the name of the next entry is: {:?}", de.name);
//!    println!("the name of the next_entry is: {:?}", core::str::from_utf8(&de.name));
//!    */
//! 
//!    // Uses the path provided and reads the bytes of the file otherwise returns 0 if file can't be found
//!    // The path can be in the format of /hello/poem.txt or \\hello\\poem.txt
//!    let path = format!("\\hello\\poem.txt"); // works for subdirectories and files that span beyond a single cluster
//! 
//!    // This open function create a file structure based on the path if it is found
//!    match fatfs.open(&path) {
//!        Ok(mut f) => {
//!            debug!("file size:{}", f.size);
//!            let mut bytes_so_far = 0;
//!            // Loops through and reads the data 512 bytes at a time and clears the data buffer after every read of the file
//!            loop {
//!                let mut data: [u8; 512] = [0; 512];
//!             
//!                match f.read(&mut data) {
//!                    Ok(bytes) => {
//!                        // println!("the bytes are:{}", bytes);
//!                        bytes_so_far += bytes;
//!                    }
//!                    Err(fat32::Error::EndOfFile) => {
//!                        println!("reached end of file");
//!                        break;
//!                    }
//!                    Err(_er) => panic!("the file failed to read"),
//!                    }
//!                ;
//!                debug!("{:X?}", &data[..]);
//!                debug!("{:?}", core::str::from_utf8(&data));
//!                }
//!
//!            println!("bytes read: {}", bytes_so_far);
//!         }
//!        Err(_) => println!("file doesnt exist"),
//!                 }
//!
//!    
//!    Ok(())
//! ```
//! 

#![no_std]
#![feature(slice_concat_ext)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate storage_device;
extern crate storage_manager;
extern crate fs_node;
extern crate spin;

// use spin::Mutex;
// use fs_node::{DirRef, FileRef, WeakDirRef, Directory, FileOrDir, File, FsNode};
// use alloc::sync::{Arc, Weak};
use core::convert::TryInto;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;

#[derive(Debug, PartialEq)]
pub enum Error {
    BlockError,
    Unsupported,
    NotFound,
    EndOfFile,
    InvalidOffset,
}

/// Indicates whether the PFSDirectory entry is a PFSFile or a PFSDirectory
#[derive(Debug, PartialEq)]
enum FileType {
    PFSFile,
    PFSDirectory,
}

/// Will adapt to fs nodes later but currently using to do read operations on a file
pub trait Read {
    fn read(&mut self, data: &mut [u8]) -> Result<u32, Error>;
    fn get_size(&self) -> u32;
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

impl<'a> Header {
    
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
            bytes_per_sector: multiple_hex_to_int(&mut bpb_sector[11..13]) as u16, 
            sectors_per_cluster: bpb_sector[13],
            reserved_sectors: bpb_sector[14],
            fat_count: bpb_sector[16],
            _root_dir_count: multiple_hex_to_int(&mut bpb_sector[17..19]) as u16,
            legacy_sectors: multiple_hex_to_int(&mut bpb_sector[19..21]) as u16,
            _media_type: bpb_sector[21],
            legacy_sectors_per_fat: bpb_sector[22],
            _sectors_per_track: multiple_hex_to_int(&mut bpb_sector[0x18..0x19]) as u16, // 0x18
            _head_count: multiple_hex_to_int(&mut bpb_sector[26..27]) as u16,
            _hidden_sectors: multiple_hex_to_int(&mut bpb_sector[28..32]) as u32,
            sectors: multiple_hex_to_int(&mut bpb_sector[32..36]) as u32,
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

impl<'a> Fat32Header {

    /// Intializes the fat32 extended header using the first 512 bytes of the disk
    pub fn new(mut bpb_sector: Vec<u8>) -> Result<Fat32Header, &'static str> {
        let fat32header = Fat32Header {
            sectors_per_fat: multiple_hex_to_int(&mut bpb_sector[36..40]) as u32,
            _flags: multiple_hex_to_int(&mut bpb_sector[40..42]) as u16,
            _version: multiple_hex_to_int(&mut bpb_sector[42..44]) as u16,
            root_cluster: multiple_hex_to_int(&mut bpb_sector[44..48]) as u32,
            _fsinfo_sector: multiple_hex_to_int(&mut bpb_sector[48..50]) as u16,
            _backup_boot_sector: multiple_hex_to_int(&mut bpb_sector[50..52]) as u16,
            _drive_no: bpb_sector[64],
            _nt_flags: bpb_sector[65],
            _signature: bpb_sector[66],
            _serial: multiple_hex_to_int(&mut bpb_sector[67..71]) as u32,
        };
        Ok(fat32header)
    }
}

/// The 32-byte FAT directory format that contains metadata about a PFSfile/subdirectory inside a PFSdirectory
#[repr(packed)]
struct FatDirectory {
    name: [u8; 11], // 8 letter entry with a 3 letter ext
    flags: u8, // designate the PFSdirectory as r,w,r/w
    _unused1: [u8; 8], // unused data
    cluster_high: u16, // but contains permissions required to access the PFSfile
    _unused2: [u8; 4], // data including modified time
    cluster_low: u16, // the starting cluster of the PFSfile
    size: u32, // size of PFSfile in bytes, volume label or subdirectory flags should be set to 0
}

/// Structure for a PFSFile entry inside a PFSDirectory 
pub struct PFSFile<'a> {
    filesystem: &'a Filesystem,
    _name: String,
    // parent: String, //WeakDirRef,
    pub start_cluster: u32,
    active_cluster: u32,
    sector_offset: u64,
    pub size: u32,
    position: u32,
}

impl<'a> Read for PFSFile<'a> {
    
    /// Given an empty data buffer and a PFSfile structure, will read off the bytes of that PFSfile and put it into the buffer
    /// Returns the number of bytes read
    fn read(&mut self, data: &mut [u8]) -> Result<u32, Error> {

        // The position is used to track how far along the PFSfile it is, and if the position goes
        // beyond the size of the PFSfile, it returns end of PFSfile
        if self.position >= self.size {
            return Err(Error::EndOfFile);
        }

        // If the current sector position is more than the sectors per cluster, it will move onto the next cluster
        if self.sector_offset == u64::from(self.filesystem.sectors_per_cluster) {
            match self.filesystem.next_cluster(self.active_cluster) {
                Err(e) => {
                    return Err(e);
                }
                Ok(cluster) => {
                    self.active_cluster = cluster;
                    self.sector_offset = 0;
                }
            }
        }

        let cluster_start = self.filesystem.first_sector_of_cluster(self.active_cluster);
        let current_position = cluster_start as u64 + self.sector_offset;

        // Reads at the beginning cluster
        let _sectors_read = match self.filesystem.header.drive.lock().read_sectors(&mut data[..], current_position as usize) {
            Ok(bytes) => bytes,
            Err(_) => return Err(Error::BlockError),
        };
        

        self.sector_offset += 1;
        
        if (self.position + 512) > self.size {
            let bytes_read = self.size - self.position;
            self.position = self.size;
            Ok(bytes_read)
        } 
        else {
            self.position += 512;
            Ok(512)
        }
    }

    fn get_size(&self) -> u32 {
        self.size
    }
}

/// Based on the FatDirectory structure and used to describe the files/subdirectories in a PFSDirectory 
pub struct DirectoryEntry {
    pub name: [u8; 11], 
    long_name: [u8; 255], // Long-name format currently not supported
    file_type: FileType,
    pub size: u32,
    cluster: u32,
}

/// Structure for a PFSDirectory
pub struct PFSDirectory<'a> {
    filesystem: &'a Filesystem,
    cluster: u32,
    sector: u32,
    offset: usize,
}

impl<'a> PFSDirectory<'a> {
    
    /// Returns the next PFSdirectory entry in a PFSdirectory and returns EndOfFile if another PFSdirectory doesn't exist
    pub fn next_entry(&mut self) -> Result<DirectoryEntry, Error> {
        
        loop {
            let sector = if self.cluster > 0 {
                
                // Identifies if the sector number is greater than the size of one cluster, and if it is
                // it will move on to the next cluster
                if self.sector >= self.filesystem.sectors_per_cluster {
                    match self.filesystem.next_cluster(self.cluster) {
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
                    + self
                        .filesystem
                        .first_sector_of_cluster(self.cluster)
            } else {
                self.sector
            };

            let mut data: Vec<u8> = vec![0; self.filesystem.header.drive.lock().sector_size_in_bytes()];
            let _sectors_read = match self.filesystem.header.drive.lock().read_sectors(&mut data[..], (sector) as usize) {
                Ok(bytes) => bytes,
                Err(_) => return Err(Error::BlockError),
            };
            
            let mut counter = 0;
            let mut directory_entries: Vec<FatDirectory> = Vec::new(); 

            // This goes through the 8 possible PFSdirectory entries in a PFSdirectory sector 
            // A PFSdirectory entry is 32 bytes, so it simply jumps to the next PFSdirectory entry by adding 32 bytes to the counter
            for _entries in 0..8 { 
                let dir: FatDirectory = FatDirectory {
                    name: data[(0+counter)..(11+counter)].try_into().expect("PFSdirectory entry slice failed"),
                    flags: data[11+counter],
                    _unused1: data[(12+counter)..(20+counter)].try_into().expect("PFSdirectory entry slice failed"),
                    cluster_high: multiple_hex_to_int(&mut data[(20+counter)..(22+counter)]) as u16,
                    _unused2: data[(22+counter)..(26+counter)].try_into().expect("PFSdirectory entry slice failed"),
                    cluster_low: multiple_hex_to_int(&mut data[(26+counter)..(28+counter)]) as u16,
                    size: multiple_hex_to_int(&mut data[(28+counter)..(32+counter)]) as u32
                };
                counter += 32;
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
}

/// Structure for the filesystem to be used to transverse the disk and run operations on the disk
pub struct Filesystem {
    header: Header, // This is meant to read the first 512 bytes of the virtual drive in order to obtain device information
    bytes_per_sector: u32, // default set to 512
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

impl<'a> Filesystem {
    
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

    /// Initializes the PFSDirectory structure for the root PFSdirectory
    pub fn root(&self) -> Result<PFSDirectory, Error> {
        match self.fat_type {
            // For FAT32, the root cluster will always be cluster 2, and it's simply a matter of initializing it as a PFSDirectory 
            32 => Ok(PFSDirectory {
                filesystem: self,
                cluster: self.root_cluster,
                sector: 0,
                offset: 0,
            }),
            _ => Err(Error::Unsupported),
        }
    }

    /// Initializes a PFSDirectory strcuture based on the cluster that the PFSdirectory is stored in
    fn get_directory(&self, cluster: u32) -> Result<PFSDirectory, Error> {
        Ok(PFSDirectory {
            filesystem: self,
            cluster: cluster,
            sector: 0,
            offset: 0,
        })
    }
    
    /// Initializes a PFSFile strcture based on the cluster that the PFSfile is stored as well as it's size
    fn get_file(&self, cluster: u32, size: u32, name: String) -> Result<PFSFile, Error> {
        Ok(PFSFile {
            filesystem: self,
            _name: name,
            // parent: parent,
            start_cluster: cluster,
            active_cluster: cluster,
            sector_offset: 0,
            size,
            position: 0,
        })
    }
    
    /// Used to transverse to the next cluster of a PFSfile/PFSdirectory that spans multiple clusters by utlizing the FAT component
    fn next_cluster(&self, cluster: u32) -> Result<u32, Error> {
        match self.fat_type {
            32 => {

                // debug!("the current cluster is:{}", cluster);
                // FAT32 uses 32 bits per FAT entry, so 1 entry = 4 bytes
                let fat_entry = cluster * 4;
                let fat_sector = self.first_fat_sector + (fat_entry / self.bytes_per_sector);

                let mut data: Vec<u8> = vec![0; self.header.drive.lock().sector_size_in_bytes()];
                let _sectors_read = match self.header.drive.lock().read_sectors(&mut data[..], fat_sector as usize) {
                    Ok(bytes) => bytes,
                    Err(_) => return Err(Error::BlockError),
                };
    
                // Because FAT32 uses 4 bytes per cluster entry, to get to a cluster's exact byte entry in the FAT you can use the cluster number * 4
                // And to get to the next cluster, the FAT must be read in little endian order using the mutliple_hex_to_int function 
                // Read more here: https://en.wikipedia.org/wiki/Design_of_the_FAT_file_system#FAT
                let next_cluster_raw = multiple_hex_to_int(&mut data[fat_entry as usize..(fat_entry as usize +4)]) as u32; 
                let next_cluster = next_cluster_raw & 0x0fff_ffff;
                // debug!("the next cluster is: {}", next_cluster);
                
                if next_cluster >= 0x0fff_fff8 {
                    Err(Error::EndOfFile)
                } else {
                    Ok(next_cluster)
                }
            }

            _ => Err(Error::Unsupported),
        }
    }
    
    // Creates a PFSfile structure based on a provided path
    pub fn open(&self, path: &str) -> Result<PFSFile, Error> {

        // First confirms the validity of the path
        assert_eq!(path.find('/').or_else(|| path.find('\\')), Some(0));

        let mut residual = path;

        // Starts at root PFSdirectory
        let mut current_dir = match self.root() {
            Ok(root) => root,
            Err(_) => return Err(Error::Unsupported),
        };

        // This loop transverses the path until the specified PFSfile is found
        loop {

            // sub is the first PFSdirectory/PFSfile name in the path and residual is what is left
            // this takes the following letters of the string \\hello\\bye\\hello.txt
            let sub = match &residual[1..]
                .find('/')
                .or_else(|| (&residual[1..]).find('\\'))
            {
                None => &residual[1..],
                Some(x) => {
                    let sub: &str = &residual[1..=*x];
                    residual = &residual[(*x + 1)..];
                    sub
                }
            };

            // If the PFSdirectory doesn't have any entries, returns NotFound
            if sub.is_empty() {
                return Err(Error::NotFound);
            }
            
            loop {
                // Keeps going through the entries in the PFSdirectory until it reaches the next sub-PFSdirectory or the specified PFSfile
                
                match current_dir.next_entry() {
                    Err(Error::EndOfFile) => return Err(Error::NotFound),
                    Err(e) => return Err(e),
                    Ok(de) => {
                        // Compares the name of the next destination in the path and the name of the PFSdirectory entry that it's currently looking at
                        if compare_name(sub, &de) {
                            match de.file_type {
                                FileType::PFSDirectory => {
                                    // If the next destination is a PFSdirectory, this will initialize the PFSdirectory structure for that  
                                    // PFSdirectory and set the current PFSdirectory to be that PFSdirectory
                                    current_dir = match self.get_directory(de.cluster) { 
                                        Ok(dir) => dir,
                                        Err(_) => return Err(Error::NotFound) 
                                    };
                                    break;
                                }
                                FileType::PFSFile => return self.get_file(de.cluster, de.size, sub.to_string()),
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Scans each drive for a filesystem and initializes it if it's FAT32 
/// 
/// # Return value
/// 
/// If one of the attached drives contains a fat filesystem, this returns the filesystem structure needed to run operations on the disk 
pub fn init<'a>() -> Result<Filesystem, &'static str>  {
    
    if let Some(controller) = storage_manager::STORAGE_CONTROLLERS.lock().iter().next() {
        for sd in controller.lock().devices() {
            // Once a FAT32 filesystem is detected, this will create the Filesystem structure from the drive
            if detect_fat(&sd) == true {
                let header = Header::new(sd);
                let fatfs = Filesystem::new(header.unwrap());
                match fatfs.init() {
                    Ok(fs) => return Ok(fs),
                    Err(_) => return Err("failed to intialize fat filesystem for disk"),
                }
                
            }
        }
    }
    Err("failed to intialize fat filesystem for disk")
}

/// Detects whether the drive passed into the function is a FAT32 drive
/// 
/// 
/// # Return value
/// 
/// returns true if the storage device passed into the function has a fat filesystem structure
pub fn detect_fat(disk: &storage_device::StorageDeviceRef) -> bool {
    
    let mut initial_buf: Vec<u8> = vec![0; disk.lock().sector_size_in_bytes()];
    let _sectors_read = match disk.lock().read_sectors(&mut initial_buf[..], 0){
        Ok(bytes) => bytes,
        Err(_) => return false,
    };
    
    // The offset at 0x52 in the extended FAT32 BPB is used to detect the Filesystem type 
    match initial_buf[82..87] {
        [0x46,0x41,0x54,0x33,0x32] => return true,
        _ => return false,
    };    
}


/// When filling the Header strcture from the initial BPB sector read, certain values are made up of multiple bytes and
/// must be read in little endian order 
/// 
/// This function converts byte integers into a hex strings then combines multiple hex-value and parses them together to get the integer value
/// 
/// # Examples
/// ```rust
/// let mut array = [02, 00, 00, 00];
/// let value = multiple_hex_to_int(&mut array[0..3])
/// assert_eq!(value, 2);
/// ```
/// Use case: To get the # of sectors in an image, you must get hex-values at 0x20 - 0x24 and combine them to get the actual value, but the values
/// need to be read in little endian order, so the order of the bytes must be reversed and combined to get the actual value
pub fn multiple_hex_to_int(hex_array: &mut [u8]) -> usize {

    let mut combined_hex: String = String::from("");
    for hex in hex_array.iter().rev() {
        let mut hex_str = format!("{:X}", hex);
        if hex_str == "0" {
            hex_str = "00".to_string()
        }
        combined_hex.push_str(&hex_str);
    }

    let combined_int: usize = match usize::from_str_radix(&combined_hex, 16){
        Ok(int) => int,
        Err(_) => 0,
    };

    combined_int 
}

/// A case-insenstive way to compare PFSdirectory entry name which is in [u8;11] and a str literal to be able to 
/// confirm whether the PFSdirectory/PFSfile that you're looking at is the one specified 
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
