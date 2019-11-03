extern crate alloc;
extern crate log;
extern crate storage_device;
extern crate storage_manager;
extern crate byteorder;
extern crate zerocopy;
extern crate memory;
extern crate block_io;

use byteorder::{ByteOrder, LittleEndian};
use core::mem::size_of;
use alloc::vec::Vec;
use block_io::{BlockIo};

/// Internal error types.
/// Note that EOF and NotFound are used for some control situations that do not necessarily represent errors.
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
    InternalError,
}

// DO NOT UNDER ANY CIRCUMSTANCE CHECK THIS FOR EQUALITY TO DETERMINE EOC
/// The end-of-cluster value written by this implementation of FAT32.
/// Note that many implementations use different values for EOC
const EOC: u32 = 0x0fff_ffff;

/// Empty cluster marked as 0.
const EMPTY_CLUSTER: u32 = 0;

/// Checks if a cluster number is an EOC value:
#[inline]
pub fn is_eoc(cluster: u32) -> bool {
    cluster_from_raw(cluster) >= 0x0fff_fff8
}

/// Checks if a cluster number counts as free:
#[inline]
pub fn is_empty_cluster(cluster: u32) -> bool {
    cluster_from_raw(cluster) == EMPTY_CLUSTER
}

/// Converts a cluster number read raw from disk into the value used for computation.
#[inline]
pub fn cluster_from_raw(cluster: u32) -> u32 {
    (cluster & 0x0fff_ffff)
}

/// Structure to store the position of a fat32 directory during a walk.
pub struct FATPosition {
    /// Cluster number of the current position.
    pub cluster: u32,
    /// Offset number of clusters
    pub cluster_offset: usize,
    /// Sector offset into the current cluster
    pub sector_offset: usize, 
    /// Byte offset into the current sector
    pub entry_offset: usize, 
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
            _sectors_per_track: LittleEndian::read_u16(&bpb_sector[0x18..0x20]),
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


// TODO I'd like to rethink this code so that it's easier to initialize and so that much of the public (and generally *immutable*) information is not behind a lock.
/// Structure for the filesystem to be used to traverse the disk and run operations on the disk
pub struct Filesystem {
    header: Header, // This is meant to read the first 512 bytes of the virtual drive in order to obtain device information
    pub io: BlockIo, // Cached byte level reader and writer to and from disk.
    pub bytes_per_sector: u32, // default set to 512 // TODO why aren't we using this instead of the drive's value?
    sectors: u32, // depends on number of clusters and sectors per cluster
    fat_type: u8, // support for fat32 only
    pub clusters: u32, // number of clusters
    sectors_per_fat: u32, 
    pub sectors_per_cluster: u32,
    fat_count: u32, 
    root_dir_sectors: u32, 
    first_fat_sector: u32,
    first_data_sector: u32,
    data_sector_count: u32, 
    data_cluster_count: u32,
    pub root_cluster: u32, // Will always be 2 for FAT32
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
        //debug!("First data sector: {}", self.first_data_sector);
        debug!("Bytes per sector: {}", self.bytes_per_sector);
        Ok(self)
    }

    /// This function allows you to jump to the data component when given a cluster number   
    pub fn first_sector_of_cluster(&self, cluster: u32) -> usize {
        // The first sector of a cluster = first portion of data component + the amount of sectors/cluster + accounting for root cluster
        (((cluster - 2) * self.sectors_per_cluster) + self.first_data_sector) as usize
    }

    /// Walks the FAT table to find the next cluster given the current cluster.
    /// Returns Err(Error::EOF) if the next cluster is an EOC indicator.
    pub fn next_cluster(&mut self, cluster: u32) -> Result<u32, Error> {
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

    pub fn pos_to_byte_offset(&self, pos: &FATPosition) -> usize {
        pos.entry_offset + pos.sector_offset * self.bytes_per_sector as usize +
            pos.cluster_offset * self.cluster_size_in_bytes()
    }
    
    /// Walks the FAT to find an empty cluster and returns the number of the first cluster found.
    pub fn find_empty_cluster(&mut self) -> Result<u32,Error>{
        
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
                    
                if is_empty_cluster(cluster_value) {
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
    pub fn extend_chain(&mut self, old_tail: u32) -> Result<u32, Error> {
        
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
    pub fn read_fat_cluster(&mut self, cluster: u32) -> Result<u32, Error> {
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
    pub fn write_fat_cluster(&mut self, cluster: u32, new_value: u32) -> Result<(), Error> {
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
    pub fn cluster_size_in_bytes(&self) -> usize {
        return (self.bytes_per_sector * self.sectors_per_cluster) as usize;
    }
    
    #[inline]
    /// Computes the fat entry in bytes in the FAT corresponding to a given cluster.
    pub fn fat_entry_from_cluster(cluster: u32) -> u32 {
        cluster * 4
    }

    #[inline]
    /// Computes the offset from the start of the disk given a sector
    pub fn sector_to_byte_offset(&self, sector: usize) -> usize {
        sector * self.bytes_per_sector as usize
    }
    
    #[inline]
    /// Computes the disk sector corresponding to a given fat entry to read the FAT.
    pub fn fat_sector_from_fat_entry(&self, fat_entry: u32) -> u32 {
        self.first_fat_sector + (fat_entry / self.bytes_per_sector) // TODO check against max size?
    }

    #[inline]
    /// Computes the size of a file of size `size` in clusters (rounds up)
    pub fn size_in_clusters(&self, size: usize) -> usize {
        (size + self.cluster_size_in_bytes() - 1) / (self.cluster_size_in_bytes())
    }


    #[inline]
    /// Returns the number of sectors per cluster as given by the header
    pub fn sectors_per_cluster(&self) -> u32 {
        self.sectors_per_cluster
    }
    
    // TODO should this function even exist?
    /// Returns a reference to the BPB derived header
    pub fn header(&self) -> &Header {
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
pub fn init_fs(sd: storage_device::StorageDeviceRef) -> Result<Filesystem, &'static str>  {
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
