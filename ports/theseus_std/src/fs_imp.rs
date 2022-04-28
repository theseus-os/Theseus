//! This module is equivalent to the Rust standard library's 
//! platform-specific "inner" fs implementation.
//! 
//! For example, for Unix-like systems, this module is implemented
//! in the file [library/std/src/sys/unix/fs.rs].
//! 
//! However, we actually based this implementation off of the base version
//! from the [library/sys/unimplemented!/fs.rs] file, which contains the skeleton of all 
//! inner fs implementation components required for a given new platform.
//! 
//! [library/std/src/sys/unix/fs.rs](https://github.com/rust-lang/rust/blob/master/library/std/src/sys/unix/fs.rs)
//! [library/sys/unimplemented!/fs.rs](https://github.com/rust-lang/rust/blob/master/library/std/src/sys/unimplemented!/fs.rs)

use crate::os_str::OsString;
use core::fmt;
use core::hash::Hash;
use core2::io::{self, /*IoSlice, IoSliceMut, ReadBuf,*/ SeekFrom, Read, Write, Seek};
use alloc::sync::Arc;
use crate::path::{Path, PathBuf};
#[cfg(feature = "time")]
use crate::sys::time::SystemTime;
use theseus_fs_node::{File as FileTrait, FileRef};
use theseus_io::{ReaderWriter, LockableIo, KnownLength};
use spin::Mutex;

/// This is a typedef for a Theseus-native `FileRef` (`Arc<Mutex<dyn File>>`)
/// that is wrapped in a series of wrapper types, described below from inner to outer.
/// 
/// 1. The `FileRef` is wrapped in a `LockableIo` object 
///    in order to forward the various I/O traits (`ByteReader` + `ByteWriter`)
///    through the `Arc<Mutex<_>>` wrappers.
/// 2. Then, that `LockableIo` <Arc<Mutex<File>>>` is wrapped in a `ReaderWriter`
///    to provide standard "stateful" I/O that advances a file offset.
/// 3. Then, that `ReaderWriter` is wrapped in another `Mutex` to provide
///    interior mutability, as the `Read` and `Write` traits requires a mutable reference
///    (`&mut self`) but Rust standard library allows you to call those methods on 
///    an immutable reference to its file, `&std::fs::File`.
/// 4. That `Mutex` is then wrapped in another `LockableIo` wrapper 
///    to ensure that the IO traits are forwarded, similar to step 1.
/// 
/// In summary, the total type looks like this:
/// ```rust 
/// LockableIo<Mutex<ReaderWriter<LockableIo<Arc<Mutex<dyn File>>>>>>
/// ```
/// 
/// ... Then we take *that* and wrap it in an authentic parisian crepe 
/// filled with egg, gruyere, merguez sausage, and portabello mushroom
/// ... [tacoooo townnnnn!!!!](https://www.youtube.com/watch?v=evUWersr7pc).
/// 
/// TODO: redesign this to avoid the double Mutex. Options include:
/// * Change the Theseus `FileRef` type to always be wrapped by a `ReaderWriter`.
/// * Use a different wrapper for interior mutability, though Mutex is probably required.
/// * Devise another set of `Read` and `Write` traits that *don't* need `&mut self`.
type OpenFileRef = LockableIo<
    'static,
    ReaderWriter<LockableFileRef>,
    Mutex<ReaderWriter<LockableFileRef>>,
    Mutex<ReaderWriter<LockableFileRef>>,
>;
/// See the above documentation for [`OpenFileRef`].
type LockableFileRef = LockableIo<
    'static,
    dyn FileTrait + Send,
    Mutex<dyn FileTrait + Send>,
    FileRef,
>;

/// In Rust's `std` library, a `File` must represent both 
/// an open file and an open directory.
/// Thus, we must account for either option within this struct.
#[derive(Debug)]
pub struct File(FileOrDirectory);

enum FileOrDirectory {
    OpenFile(OpenFileRef),
    Directory(theseus_fs_node::DirRef),
}

impl fmt::Debug for FileOrDirectory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            Self::OpenFile(file) => write!(
                f, 
                "OpenFile({})",
                file.try_lock()
                    .and_then(|rw| rw.try_lock()
                        .map(|fr| fr.get_absolute_path())
                    )
                    .unwrap_or_else(|| "<Locked>".into())
            ),
            Self::Directory(dir) => write!(
                f, 
                "Directory({})",
                dir.try_lock()
                    .map(|d| d.get_absolute_path())
                    .unwrap_or_else(|| "<Locked>".into())
            ),
        }
    }
}


#[derive(Clone, Copy, Debug)]
pub struct FileAttr {
    size: u64,
    // `true` if file, `false` if directory
    is_file: bool,
    symlink: bool,
}

pub struct ReadDir();

pub struct DirEntry();

#[derive(Clone, Debug)]
pub struct OpenOptions {
    // Includes only the system-generic flags for now.
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
}

/// Theseus doesn't yet have any notion of file permissions,
/// so all permissions are always granted for every file.
pub struct FilePermissions;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileType {
    typ: FileTypeInner,
    symlink: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FileTypeInner {
    File,
    Dir,
}

#[derive(Debug)]
pub struct DirBuilder {}

impl FileAttr {
    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn perm(&self) -> FilePermissions {
        FilePermissions { }
    }

    pub fn file_type(&self) -> FileType {
        FileType {
            typ: match self.is_file {
                true  => FileTypeInner::File,
                false => FileTypeInner::Dir,
            },
            symlink: self.symlink,
        }
    }

    #[cfg(feature = "time")]
    pub fn modified(&self) -> io::Result<SystemTime> {
        self.0
    }

    #[cfg(feature = "time")]
    pub fn accessed(&self) -> io::Result<SystemTime> {
        self.0
    }

    #[cfg(feature = "time")]
    pub fn created(&self) -> io::Result<SystemTime> {
        self.0
    }
}

impl FilePermissions {
    pub fn readonly(&self) -> bool {
        // Theseus gives all files all permissions at the moment
        false
    }

    pub fn set_readonly(&mut self, _readonly: bool) {
        // do nothing, Theseus doesn't have file permissions yet
    }
}

impl Clone for FilePermissions {
    fn clone(&self) -> FilePermissions {
        FilePermissions { }
    }
}

impl PartialEq for FilePermissions {
    fn eq(&self, _other: &FilePermissions) -> bool {
        true
    }
}

impl Eq for FilePermissions {}

impl fmt::Debug for FilePermissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FilePermissions {{ }}")
    }
}

impl FileType {
    pub fn is_dir(&self) -> bool {
        match self.typ {
            FileTypeInner::File => false,
            FileTypeInner::Dir => true,
        }
    }

    pub fn is_file(&self) -> bool {
        match self.typ {
            FileTypeInner::File => true,
            FileTypeInner::Dir => false,
        }
    }

    pub fn is_symlink(&self) -> bool {
        self.symlink
    }
}

impl fmt::Debug for ReadDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReadDir()")
    }
}

impl Iterator for ReadDir {
    type Item = io::Result<DirEntry>;

    fn next(&mut self) -> Option<io::Result<DirEntry>> {
        None
    }
}

impl DirEntry {
    pub fn path(&self) -> PathBuf {
        unimplemented!()
    }

    pub fn file_name(&self) -> OsString {
        unimplemented!()
    }

    pub fn metadata(&self) -> io::Result<FileAttr> {
        unimplemented!()
    }

    pub fn file_type(&self) -> io::Result<FileType> {
        unimplemented!()
    }
}

impl OpenOptions {
    pub fn new() -> OpenOptions {
        OpenOptions {
            read: false,
            write: false,
            append: false,
            truncate: false,
            create: false,
            create_new: false,
        }
    }

    pub fn read(&mut self, read: bool) {
        self.read = read;
    }
    pub fn write(&mut self, write: bool) {
        self.write = write;
    }
    pub fn append(&mut self, append: bool) {
        self.append = append;
    }
    pub fn truncate(&mut self, truncate: bool) {
        self.truncate = truncate;
    }
    pub fn create(&mut self, create: bool) {
        self.create = create;
    }
    pub fn create_new(&mut self, create_new: bool) {
        self.create_new = create_new;
    }
}

impl File {
    pub fn open(path: &Path, _opts: &OpenOptions) -> io::Result<File> {
        let working_dir = crate::env::current_dir()?;
        theseus_path::Path::new(path.to_string_lossy().into()).get(&working_dir)
            .ok_or(io::ErrorKind::NotFound.into())
            .map(|theseus_file_or_dir| match theseus_file_or_dir {
                theseus_fs_node::FileOrDir::File(f) => FileOrDirectory::OpenFile(
                    LockableIo::from(Mutex::new(ReaderWriter::new(LockableIo::from(f))))
                ),
                theseus_fs_node::FileOrDir::Dir(d) => FileOrDirectory::Directory(d),
            }).map(File) // wrap it in the main `File` struct
    }

    pub fn file_attr(&self) -> io::Result<FileAttr> {
        let (size, is_file) = match &self.0 {
            FileOrDirectory::OpenFile(f) => (f.lock().len(), true),
            FileOrDirectory::Directory(_) => (0, false),
        };
        Ok(FileAttr {
            size: size as u64,
            is_file,
             // Theseus doesn't support symlinks yet
            symlink: false,
        })
    }

    pub fn fsync(&self) -> io::Result<()> {
        Ok(())
    }

    pub fn datasync(&self) -> io::Result<()> {
        Ok(())
    }

    pub fn truncate(&self, _size: u64) -> io::Result<()> {
        todo!("Theseus doesn't yet support truncate of files")
    }

    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        match &self.0 {
            FileOrDirectory::Directory(_) => Err(io::Error::new(
                io::ErrorKind::Other,
                "Is A Directory (TODO: use IsADirectory)"
            )),
            FileOrDirectory::OpenFile(f) => f.lock().read(buf),
        }
    }

    #[cfg(feature = "ioslice")]
    pub fn read_vectored(&self, _bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        self.0
    }

    #[cfg(feature = "ioslice")]
    pub fn is_read_vectored(&self) -> bool {
        self.0
    }

    #[cfg(feature = "readbuf")]
    pub fn read_buf(&self, _buf: &mut ReadBuf<'_>) -> io::Result<()> {
        self.0
    }

    pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
        match &self.0 {
            FileOrDirectory::Directory(_) => Err(io::Error::new(
                io::ErrorKind::Other,
                "Is A Directory (TODO: use IsADirectory)"
            )),
            FileOrDirectory::OpenFile(f) => f.lock().write(buf),
        }
    }

    #[cfg(feature = "ioslice")]
    pub fn write_vectored(&self, _bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        self.0
    }

    #[cfg(feature = "ioslice")]
    pub fn is_write_vectored(&self) -> bool {
        self.0
    }

    pub fn flush(&self) -> io::Result<()> {
        match &self.0 {
            FileOrDirectory::Directory(_) => Err(io::Error::new(
                io::ErrorKind::Other,
                "Is A Directory (TODO: use IsADirectory)"
            )),
            FileOrDirectory::OpenFile(f) => f.lock().flush(),
        }
    }

    pub fn seek(&self, pos: SeekFrom) -> io::Result<u64> {
        match &self.0 {
            FileOrDirectory::Directory(_) => Err(io::Error::new(
                io::ErrorKind::Other,
                "Is A Directory (TODO: use IsADirectory)"
            )),
            FileOrDirectory::OpenFile(f) => f.lock().seek(pos),
        }
    }

    pub fn duplicate(&self) -> io::Result<File> {
        unimplemented!("duplicate is unimplemented for Theseus files")
    }

    pub fn set_permissions(&self, _perm: FilePermissions) -> io::Result<()> {
        Ok(())
    }
}

impl DirBuilder {
    pub fn new() -> DirBuilder {
        DirBuilder {}
    }

    pub fn mkdir(&self, _p: &Path) -> io::Result<()> {
        unimplemented!()
    }
}

pub fn readdir(_p: &Path) -> io::Result<ReadDir> {
    unimplemented!()
}

pub fn unlink(_p: &Path) -> io::Result<()> {
    unimplemented!()
}

pub fn rename(_old: &Path, _new: &Path) -> io::Result<()> {
    unimplemented!()
}

pub fn set_perm(_p: &Path, _perm: FilePermissions) -> io::Result<()> {
    unimplemented!()
}

pub fn rmdir(_p: &Path) -> io::Result<()> {
    unimplemented!()
}

pub fn remove_dir_all(_path: &Path) -> io::Result<()> {
    unimplemented!()
}

pub fn try_exists(_path: &Path) -> io::Result<bool> {
    unimplemented!()
}

pub fn readlink(_p: &Path) -> io::Result<PathBuf> {
    unimplemented!()
}

pub fn symlink(_original: &Path, _link: &Path) -> io::Result<()> {
    unimplemented!()
}

pub fn link(_src: &Path, _dst: &Path) -> io::Result<()> {
    unimplemented!()
}

pub fn stat(_p: &Path) -> io::Result<FileAttr> {
    unimplemented!()
}

pub fn lstat(_p: &Path) -> io::Result<FileAttr> {
    unimplemented!()
}

pub fn canonicalize(_p: &Path) -> io::Result<PathBuf> {
    unimplemented!()
}

pub fn copy(_from: &Path, _to: &Path) -> io::Result<u64> {
    unimplemented!()
}