use zerocopy::{FromBytes, AsBytes, LayoutVerified, ByteSlice, Unaligned};
use zerocopy::byteorder::{U16, U32};
use byteorder::{ByteOrder, LittleEndian};
use core::mem::size_of;
use core::cmp::{Ord, Ordering, min};
use core::char::decode_utf16;
use core::str::from_utf8;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;

use fs_core::{Error};
use crate::{FREE_DIRECTORY_ENTRY, FileAttributes, DOT_DIRECTORY_ENTRY};

pub type ShortName = [u8; 11];
// TODO: May want a method to validate the LongName struct too.
/// Generalized name struct that can be filled from and written onto disk easily.
pub struct LongName {
    pub short_name: ShortName,
    pub long_name: Vec<[u16; 13]>,
}

/// Indicates whether the FATDirectory entry is a FATFile or a FATDirectory
#[derive(Debug, PartialEq)]
pub enum FileType {
    FATFile,
    FATDirectory,
}

impl LongName {
    /// Construct a new long name from a sequence of on disk directory entries.
    pub fn new(vfat_array: Vec<&VFATEntry>, entry: &RawFatDirectory) -> Result<LongName, Error> {
        let mut long_name = LongName::empty();
        // Last entry must contain the short_name bytes.
        if entry.is_vfat() {
            // VFAT long name extension entries do not contain the short name.
            // As such this is an illegal sequence of directories.
            return Err(Error::InconsistentDisk)
        }

        long_name.short_name = entry.name;

        // Note that we must reverse the iterator, since end of string is stored 
        // in the first VFAT entry.
        // IMPROVEMENT validate sequence numbers and checksum
        long_name.long_name = vfat_array.iter().rev().map(
            |vfat_entry| {
                let mut entry_chars = [0; 13];
                // Unsure of order may need to switch this.
                let mut offset = 0;
                for i in 0..5 {
                    entry_chars[offset] = vfat_entry.name1[i].get();
                    offset += 1;
                };

                for i in 0..6 {
                    entry_chars[offset] = vfat_entry.name2[i].get();
                    offset += 1;
                };

                for i in 0..2 {
                    entry_chars[offset] = vfat_entry.name3[i].get();
                    offset += 1;
                };
                entry_chars
            }
        ).collect::<Vec<_>>();

        Ok(long_name)
    }

    pub fn from_short_name(short_name: ShortName) -> LongName {
        let mut long_name = LongName::empty();
        long_name.short_name = short_name;
        long_name
    }

    pub fn empty() -> LongName {
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
    /// Convert to a string that compares consistently.
    fn to_comp_name(&self) -> String {
        self.name.to_uppercase()
    }

    pub fn from_string(name: &str) -> Result<DiskName, Error> {
        // FIXME validate string names:
        if name.is_empty() {
            return Err(Error::IllegalArgument);
        }

        // TODO this actually needs to go in the to_long_name method
        // since the short name bytes are the only part where trailing spaces get stripped.
        // FIXME properly strip trailing spaces:
        Ok(DiskName {
            name: name.trim_end().to_string()
        })
    }

    pub fn from_long_name(long_name: LongName) -> Result<DiskName, Error> {
        // If the long name is empty emit a short name:
        if long_name.long_name.is_empty() {
            // REVIEW this is kind of an overly permissive way to read the name.
            // Although it should never cause a valid short name to be rejected.
            let name = from_utf8(&long_name.short_name).map_err(|_| Error::InconsistentDisk)?;
            return DiskName::from_string(&name);
        }

        let chars_raw = long_name.long_name.iter().map(
            |chars| chars.iter().map(|x| *x)
        ).flatten()
        .take_while(|x| *x != 0 && *x != 0xffff);

        // Currently we fail if any char fails to decode. Not ideal, but it's OK.
        let utf16_name = decode_utf16(chars_raw).collect::<Result<String, _>>()
            .map_err(|_| Error::IllegalArgument)?;

        if utf16_name.len() > 255 {
            warn!("Found on-disk long name that was over 255 characters: {:}.", utf16_name.len());
            return Err(Error::InconsistentDisk);
        };

        // TODO this name is safe to not check so I should just return it
        // (Although it might not be valid UCS-16 so I could use this function to check the UCS-16)
        // encoding.
        DiskName::from_string(&utf16_name)
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
    pub fn to_long_name(&self) -> LongName {
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


// TODO I'm not sure if there's a better way to do this, but it seems to work.
/// A struct used to read RawFatDirectory directly from bytes safely.
pub struct RawFatDirectoryBytes<B> {
    fat_directory: LayoutVerified<B, RawFatDirectory>,
}

impl<B: ByteSlice> RawFatDirectoryBytes<B> {
    pub fn parse(bytes: B) -> Option<RawFatDirectoryBytes<B>> {
        let fat_directory = LayoutVerified::new(bytes)?;

        Some(RawFatDirectoryBytes{fat_directory})
    }

    /// Convert a bytearray of entries into an array of entries.
    pub fn parse_array(bytes: B, num_entries: usize) -> Option<(Vec<VFATEntryBytes<B>>, 
        RawFatDirectoryBytes<B>)> {
        
        let mut vfat_array = Vec::new();
        let mut byte_suffix = bytes;
        for _ in 0..num_entries {
            //let vfat_entry: LayoutVerified<B, VFATEntry>;
            let (vfat_entry, remainder): (LayoutVerified<B, VFATEntry>, B) = 
                LayoutVerified::new_unaligned_from_prefix(byte_suffix)?;
            byte_suffix = remainder;

            //let vfat_entry_ref: &VFATEntry = &vfat_entry;

            vfat_array.push(VFATEntryBytes{vfat_entry});
        }

        let fat_directory = RawFatDirectoryBytes::parse(byte_suffix)?;

        Some((vfat_array, fat_directory))
    }

    pub fn get_fat_directory(&self) -> &RawFatDirectory {
        &self.fat_directory
    }
}

pub struct VFATEntryBytes<B> {
    vfat_entry: LayoutVerified<B, VFATEntry>,
}

impl<B: ByteSlice> VFATEntryBytes<B> {
    pub fn get_vfat_entry(&self) -> &VFATEntry {
        &self.vfat_entry
    }
}

#[derive(Clone)] // FIXME this shouldn't be necessary, but our current API makes it necessary.
                 // It seems like the only way to tranfer ownership of a RawFatDirectory made from the raw
                 // bytes is to transfer ownership of the bytes. However this would reqire significant
                 // refactoring.
#[repr(packed)]
#[derive(FromBytes, AsBytes)]
/// A raw 32-byte FAT directory as it exists on disk.
pub struct RawFatDirectory {
    pub name: [u8; 11], // 8 letter entry with a 3 letter ext
    flags: u8, // designate the FATdirectory as r,w,r/w
    _unused1: [u8; 8], // unused data
    cluster_high: U16<LittleEndian>, // but contains permissions required to access the FATfile
    _unused2: [u8; 4], // data including modified time
    cluster_low: U16<LittleEndian>, // the starting cluster of the FATfile
    size: U32<LittleEndian>, // size of FATfile in bytes, volume label or subdirectory flags should be set to 0
}

#[repr(packed)]
#[derive(FromBytes, AsBytes, Unaligned)]
/// A raw 32-byte FAT directory as it exists on disk -> Interpreted as a VFAT name
pub struct VFATEntry {
    sequence_number: u8,
    name1: [U16<LittleEndian>; 5],
    attributes: u8,
    filetype: u8,
    checksum: u8,
    name2: [U16<LittleEndian>; 6],
    first_cluster: U16<LittleEndian>,
    name3: [U16<LittleEndian>; 2],
}

impl RawFatDirectory {   
    pub fn to_directory_entry(&self) -> Result<DirectoryEntry, Error> {
        //let name = String::from_utf8(self.name.to_vec()).map_err(|_| Error::IllegalArgument)?;
        let name = LongName::from_short_name(self.name);

        self.to_directory_entry_with_name(name)
    }


    pub fn to_directory_entry_with_name(&self, name: LongName) -> Result<DirectoryEntry, Error> {
        
        let num_entries = name.long_name.len() + 1;
        if num_entries > 20 + 1 { // Max number of on-disk entries is 20 VFAT + 1 normal entry
            return Err(Error::InconsistentDisk);
        }
        
        Ok(DirectoryEntry {
            name: DiskName::from_long_name(name)?,
            file_type: if self.flags & FileAttributes::SUBDIRECTORY.bits == FileAttributes::SUBDIRECTORY.bits {
                FileType::FATDirectory
            } else {
                FileType::FATFile
            },
            cluster: (self.cluster_high.get() as u32) << 16 |(self.cluster_low.get() as u32),
            size: self.size.get(),
            num_entries: num_entries as u8,
        })
    }
    
    pub fn make_unused() -> RawFatDirectory {
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
    pub fn is_free(&self) -> bool {
        self.name[0] == 0 ||
            self.name[0] == FREE_DIRECTORY_ENTRY 
    }

    #[inline]
    pub fn is_dot(&self) -> bool {
        self.name[0] == DOT_DIRECTORY_ENTRY
    }

    #[inline]
    pub fn is_vfat(&self) -> bool {
        self.flags & FileAttributes::VFAT.bits == FileAttributes::VFAT.bits
    }

    #[inline]
    /// Zero name marks end of allocated entries.
    pub fn is_end(&self) -> bool {
        self.name[0] == 0
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
    pub file_type: FileType,
    pub size: u32,
    pub cluster: u32,
    pub num_entries: u8, // Number of table entries that this entry spans.
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
    pub fn to_raw_fat_directory(&self) -> RawFatDirectory {

        let long_name = self.name.to_long_name();

        RawFatDirectory {
            name: long_name.short_name,
            flags: match self.file_type {
                FileType::FATFile => 0x0,
                FileType::FATDirectory => FileAttributes::SUBDIRECTORY.bits,
            }, // FIXME need to store more flags in DirectoryEntry type
            _unused1: [0; 8],
            cluster_high: U16::new((self.cluster >> 16) as u16),
            _unused2: [0; 4],
            cluster_low: U16::new((self.cluster) as u16),
            size: U32::new(self.size)
        }
    }

    pub fn is_dot(&self) -> bool {
        self.name.name == "." || self.name.name == ".."
    }
}