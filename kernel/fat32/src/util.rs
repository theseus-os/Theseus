use zerocopy::{FromBytes, AsBytes, LayoutVerified, ByteSlice, Unaligned};
use zerocopy::byteorder::{U16, U32};
use byteorder::{ByteOrder, LittleEndian};
use core::cmp::{Ord, Ordering, min};
use core::char::decode_utf16;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use core::convert::{From, TryFrom};

use fs_core::{Error};
use crate::{FREE_DIRECTORY_ENTRY, FileAttributes, DOT_DIRECTORY_ENTRY};

#[repr(packed)]
pub struct ShortName {
    name: [u8; 11],
}

impl ShortName {

    pub fn new(name: [u8; 11]) -> ShortName {
        ShortName {
            name
        }
    }

    /// Converts a short name into the canonical name that windows would display.
    /// Note that the primary step this code performs is to insert implicit dot
    /// characters and ignore whitespace.
    /// Slight error in implementation (FIXME): I think that I don't properly handle names like "myfile."
    /// where according to spec we should treat as "myfile". This isn't worth it though.
    pub fn canonical_string(&self) -> String {
        // Peek to see if an extension is present:
        let b = self.name[8];
        let c = char::from(b);
        let ext_present = !c.is_ascii_whitespace();

        let b = self.name[0];
        let is_dot = b == b'.';

        let mut chars = Vec::new();
        for i in 0..8 {
            let b = self.name[i];
            let c = char::from(b);

            //debug!("Got new char at pos {:}: {:}", i, c);

            // FIXME actually the spec might allow spaces in a name instead of treating as padding...
            // So instead we need to remove trailing spaces, blah
            if c.is_ascii_whitespace() {
                break
            }

            if c == '.' && !ext_present && !is_dot {
                break;
            }

            chars.push(c);

            if c == '.' && !is_dot {
                break;
            }

            // Implicit '.' to support 12 character and truncated long names.
            if i == 7 && ext_present {
                chars.push('.');
            }
        }

        for i in 0..3 {
            let b = self.name[i + 8];
            let c = char::from(b);
            if c.is_ascii_whitespace() {
                break
            }

            // No '.' allowed in extension.
            if c == '.' {
                break;
            }

            chars.push(c);
        }

        chars.iter().collect()
    }
}

// TODO if we construct the canonical short name, it depends on the number of
// duplicates on disk, so we'll need to figure that out somehow, which is quite
// a bit outside of the scope of how we're currently doing things.
// For now I'm probably going to just make things ~1 for all short names if truncated.
// Another option is to manually modify the long name after construction to account for
// the number of aliases? Seems less awful, but leads to some less idiomatic code.
// I think that the most effective solution is to either set a flag or simply
// check that the long_name field is not empty and then overwrite the short name
// before interacting with disk based on the number of duplicates.
impl From<DiskName> for ShortName {
    fn from(name: DiskName) -> ShortName {
        ShortName::new(
        match name.name.rsplitn(1, |x| x == '.').collect::<Vec<&str>>()[0..2] {
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
        })
    }
}

/// Indicates whether the FATDirectory entry is a FATFile or a FATDirectory
#[derive(Debug, PartialEq)]
pub enum FileType {
    FATFile,
    FATDirectory,
}

// TODO: May want a method to validate the LongName struct too.
/// Generalized name struct that can be filled from and written onto disk easily.
pub struct LongName {
    short_name: ShortName,
    long_name: Vec<[u16; 13]>,
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

        long_name.short_name = ShortName::new(entry.name);

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

    pub fn empty() -> LongName {
        LongName {
            short_name: ShortName::new([0; 11]),
            long_name: Vec::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> LongName {
        LongName {
            short_name: ShortName::new([0; 11]),
            long_name: Vec::with_capacity(capacity)
        }
    }
}

impl From<ShortName> for LongName {
    fn from(short_name: ShortName) -> LongName {
        let mut long_name = LongName::empty();
        long_name.short_name = short_name;
        long_name
    }
}

impl From<DiskName> for LongName {
    // FIXME this is implementation is mostly what we'd need for 
    fn from(name: DiskName) -> LongName {
        let long_name = LongName::from(ShortName::from(name));

        // TODO encode the name into UCS-2 and split into small buffers
        // to construct the UCS-2 encoded long name.

        unimplemented!();
    }
}

/// Subset of strings which are checked to be legal to write to disk.
#[derive(Clone)]
#[derive(Debug)]
pub struct DiskName {
    name: String,
}

impl DiskName {
    /// Convert to a string that compares consistently.
    fn to_comp_name(&self) -> String {
        self.name.to_uppercase()
    }
}

impl TryFrom<LongName> for DiskName {
    type Error = Error;

    fn try_from(long_name: LongName) -> Result<DiskName, Error> {
                // If the long name is empty emit a short name:
        // This requires following some specific rules as given in 
        // https://en.wikipedia.org/wiki/8.3_filename
        if long_name.long_name.is_empty() {
            return DiskName::try_from(&long_name.short_name.canonical_string() as &str);
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
        DiskName::try_from(&utf16_name as &str)
    }
}

impl TryFrom<&str> for DiskName {
    type Error = Error;

    fn try_from(name: &str) -> Result<DiskName, Error> {
        // FIXME better validate string names:
        if name.is_empty() {
            warn!("DiskName::from_string called with empty string");
            return Err(Error::IllegalArgument);
        }

        // TODO this actually needs to go in the to_long_name method
        // since the short name bytes are the only part where trailing spaces get stripped.
        // FIXME properly strip trailing spaces:
        Ok(DiskName {
            name: name.trim_end().to_string()
        })
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

impl RawFatDirectory {
    /// Create a directory entry from this raw entry. Note that this method should not be called for 
    /// entries which have a VFAT long name, but right now there is no check in the code for this.
    pub fn to_directory_entry(&self) -> Result<DirectoryEntry, Error> {
        //let name = String::from_utf8(self.name.to_vec()).map_err(|_| Error::IllegalArgument)?;
        let name: LongName = ShortName::new(self.name).into();

        // Note that this doesn't detect if this is part of a VFAT entry, but unfortunately there isn't much of a check.
        // (Although we could check for a trimmed name)
        // TODO
        if self.is_vfat() {
            warn!("RawFatDirectory::to_directory_entry called for VFAT directory.")
        }

        self.to_directory_entry_with_name(name)
    }


    pub fn to_directory_entry_with_name(&self, name: LongName) -> Result<DirectoryEntry, Error> {
        
        let num_entries = name.long_name.len() + 1;
        if num_entries > 20 + 1 { // Max number of on-disk entries is 20 VFAT + 1 normal entry
            return Err(Error::InconsistentDisk);
        }
        
        Ok(DirectoryEntry {
            name: DiskName::try_from(name)?,
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
    
    /// Make a directory entry that is free (but not necessarily end of allocated entries)
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

    /// Make a directory entry that marks the end of allocated entries.
    pub fn make_end() -> RawFatDirectory {
        RawFatDirectory {
            name: [0; 11],
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

impl VFATEntry {
    fn empty() -> VFATEntry {
        VFATEntry {
            sequence_number: 0,
            name1: [U16::new(0); 5],
            attributes: 0x0F, // Always the attribute according to wikipedia
            filetype: 0, // Generally 0, may have some reserved meaning
            checksum: 0, // Need to compute this and fill it in. FIXME
            name2: [U16::new(0); 6],
            first_cluster: U16::new(0),
            name3: [U16::new(0); 2],
        }
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

    /// NOTE That the unimplemented checksum in theory requires the long name that we use
    /// to be the known on disk one. Which is sort of out of the scope of our current type setup.
    /// Converts this directory entry to the raw entries that will represent this object on disk.
    // I think in the long run we'll have to have DiskNames carry around the long name as well.
    pub fn to_raw_entries(&self) -> (RawFatDirectory, Vec<VFATEntry>) {
        let long_name: LongName = self.name.clone().into();

        let short_name_entry = RawFatDirectory {
            name: long_name.short_name.name,
            flags: match self.file_type {
                FileType::FATFile => 0x0,
                FileType::FATDirectory => FileAttributes::SUBDIRECTORY.bits,
            }, // FIXME need to store more flags in DirectoryEntry type
            _unused1: [0; 8],
            cluster_high: U16::new((self.cluster >> 16) as u16),
            _unused2: [0; 4],
            cluster_low: U16::new((self.cluster) as u16),
            size: U32::new(self.size)
        };

        // Spec requires at most 20 entries (for a length 255 name).
        // In theory this has already been checked, but is relatively recoverable.
        let dir_count: u8 = if long_name.long_name.len() > 20 {
            error!("DirectoryEntry::to_raw_entries called with long name having more than 20 entries");
            20
        } else {
            long_name.long_name.len() as u8
        };

        let mut vfat_entries = Vec::with_capacity(dir_count as usize);

        let checksum = 0; // FIXME need LongName::checksum first...

        // Want entries in order we write onto disk. So sequence number counts down:
        for (i, name_segment) in long_name.long_name.iter().take(dir_count as usize).enumerate() {
            let sequence_number: u8 = dir_count - i as u8;
            let sequence_number_raw = if sequence_number == dir_count {
                // FIXME bitfields here
                sequence_number | 0x40
            } else {
                sequence_number
            };

            let mut entry = VFATEntry::empty();
            entry.sequence_number = sequence_number_raw;
            entry.checksum = checksum;

            // FIXME do this idiomatically:
            for i in 0..5 {
                entry.name1[i] = U16::new(name_segment[i]);
            }
            for i in 0..6 {
                entry.name2[i] = U16::new(name_segment[i + 5]);
            }
            for i in 0..2 {
                entry.name3[i] = U16::new(name_segment[i + 11]);
            }

            vfat_entries.push(entry);           
        };

        (short_name_entry, vfat_entries)
    }

    pub fn is_dot(&self) -> bool {
        self.name.name == "." || self.name.name == ".."
    }
}