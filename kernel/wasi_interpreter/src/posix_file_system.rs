//! Types for interacting with Theseus standard I/O and file system via POSIX-style abstraction.
//!
//! This module provides APIs to:
//! * interact with a file descriptor table (open path, close fd, get underlying Theseus handle).
//! * read, write, or seek a file system node or standard I/O
//!
//! This abstraction is necessary as WASI assumes a POSIX-style file descriptor table interface.
//!

use alloc::string::String;
use alloc::vec::Vec;
use core::{cmp, convert::TryFrom as _};
use fs_node::{DirRef, FileOrDir, FileRef, FsNode};
use hashbrown::HashMap;
use memfs::MemFile;
use path::Path;

const FIRST_NONRESERVED_FD: wasi::Fd = 3;

/// File types that can be accessed through file descriptor table.
pub enum PosixNodeOrStdio {
    /// Standard input.
    Stdin,
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
    /// An underlying file system node.
    Inode(PosixNode),
}

impl PosixNodeOrStdio {
    /// Writes data from the given `buffer` to this file.
    ///
    /// The number of bytes written is dictated by the length of the given `buffer`.
    ///
    /// # Arguments
    /// * `buffer`: the buffer from which data will be written.
    ///
    /// # Return
    /// If successful, returns the number of bytes written to this file.
    /// Otherwise, returns a wasi::Errno.
    pub fn write(&mut self, buffer: &[u8]) -> Result<usize, wasi::Errno> {
        match self {
            PosixNodeOrStdio::Stdin => Err(wasi::ERRNO_NOTSUP),
            PosixNodeOrStdio::Stdout => match app_io::stdout().unwrap().write_all(buffer) {
                Ok(_) => Ok(buffer.len()),
                Err(_) => Err(wasi::ERRNO_IO),
            },
            PosixNodeOrStdio::Stderr => match app_io::stderr().unwrap().write_all(buffer) {
                Ok(_) => Ok(buffer.len()),
                Err(_) => Err(wasi::ERRNO_IO),
            },
            PosixNodeOrStdio::Inode(posix_node) => posix_node.write(buffer),
        }
    }

    /// Reads data from this file into the given `buffer`.
    ///
    /// The number of bytes read is dictated by the length of the given `buffer`.
    ///
    /// # Arguments
    /// * `buffer`: the buffer into which the data will be read.
    ///
    /// # Return
    /// If successful, returns the number of bytes read into this file.
    /// Otherwise, returns a wasi::Errno.
    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize, wasi::Errno> {
        match self {
            PosixNodeOrStdio::Stdin => match app_io::stdin().unwrap().read(buffer) {
                Ok(bytes_read) => Ok(bytes_read),
                Err(_) => Err(wasi::ERRNO_IO),
            },
            PosixNodeOrStdio::Stdout => Err(wasi::ERRNO_NOTSUP),
            PosixNodeOrStdio::Stderr => Err(wasi::ERRNO_NOTSUP),
            PosixNodeOrStdio::Inode(posix_node) => posix_node.read(buffer),
        }
    }

    /// Move the offset of this file.
    ///
    /// # Arguments
    /// * `offset`: the number of bytes to move.
    /// * `whence`: the base from which the offset is relative.
    ///
    /// # Return
    /// If successful, returns the resulting offset of this file.
    /// Otherwise, returns a wasi::Errno.
    pub fn seek(
        &mut self,
        offset: wasi::Filedelta,
        whence: wasi::Whence,
    ) -> Result<usize, wasi::Errno> {
        match self {
            PosixNodeOrStdio::Stdin => Err(wasi::ERRNO_NOTSUP),
            PosixNodeOrStdio::Stdout => Err(wasi::ERRNO_NOTSUP),
            PosixNodeOrStdio::Stderr => Err(wasi::ERRNO_NOTSUP),
            PosixNodeOrStdio::Inode(posix_node) => posix_node.seek(offset, whence),
        }
    }
}

/// A wrapper around Theseus FileOrDir to provide WASI-expected POSIX features.
pub struct PosixNode {
    /// Underlying Theseus FileOrDir.
    pub theseus_file_or_dir: FileOrDir,
    /// File system ights that apply to this file descriptor.
    fs_rights_base: wasi::Rights,
    /// Maximum set of rights applied to file descriptors opened through this file descriptor.
    fs_rights_inheriting: wasi::Rights,
    /// File descriptor flags.
    /// NOTE: contains unused flags for synchornized I/O, non-blocking mode.
    fs_flags: wasi::Fdflags,
    /// Offset of this file descriptor.
    offset: usize,
}

impl PosixNode {
    /// Instantiates a new PosixNode.
    ///
    /// # Arguments
    /// * `file_or_dir`: underlying Theseus FileOrDir.
    /// * `fs_rights`: rights applying to this file descriptor.
    /// * `fs_rights_inheriting`: rights applying to inherting file descriptors.
    /// * `fs_flags`: file descriptor flags.
    ///
    /// # Return
    /// Returns a PosixNode of a FileOrDir with specified permissions.
    pub fn new(
        file_or_dir: FileOrDir,
        fs_rights_base: wasi::Rights,
        fs_rights_inheriting: wasi::Rights,
        fs_flags: wasi::Fdflags,
    ) -> PosixNode {
        PosixNode {
            theseus_file_or_dir: file_or_dir,
            fs_rights_base,
            fs_rights_inheriting,
            fs_flags,
            offset: 0,
        }
    }

    /// Get path relative to working directory of this file descriptor.
    ///
    /// # Return
    /// Returns relative path of file descriptor as a string.
    pub fn get_relative_path(&self) -> String {
        let absolute_path = Path::new(self.theseus_file_or_dir.get_absolute_path());
        let wd_path = task::with_current_task(|t|
            Path::new(t.get_env().lock().cwd())
        ).expect("couldn't get current task");

        let relative_path: Path = absolute_path.relative(&wd_path).unwrap();
        String::from(relative_path)
    }

    /// Get file system rights of this file descriptor.
    ///
    /// # Return
    /// Returns file system rights of this file descriptor.
    pub fn fs_rights_base(&self) -> wasi::Rights {
        self.fs_rights_base
    }

    /// Get inheriting file system rights of this file descriptor.
    ///
    /// # Return
    /// Returns inheriting file system rights of this file descriptor.
    pub fn fs_rights_inheriting(&self) -> wasi::Rights {
        self.fs_rights_inheriting
    }

    /// Get file descriptor flags of this file descriptor.
    ///
    /// # Return
    /// Returns file descriptor flags of this file descriptor.
    pub fn fs_flags(&self) -> wasi::Fdflags {
        self.fs_flags
    }

    /// Set file descriptor flags of this file descriptor if allowed.
    ///
    /// # Return
    /// If successful, returns ().
    /// Otherwise, returns a wasi::Errno.
    pub fn set_fs_flags(&mut self, new_flags: wasi::Fdflags) -> Result<(), wasi::Errno> {
        // Verify that file descriptor has right to set flags.
        if self.fs_rights_base() & wasi::RIGHTS_FD_FDSTAT_SET_FLAGS == 0 {
            return Err(wasi::ERRNO_ACCES);
        }

        self.fs_flags = new_flags;
        Ok(())
    }

    /// Writes data from the given `buffer` to this file system node if allowed.
    ///
    /// The number of bytes written is dictated by the length of the given `buffer`.
    ///
    /// # Arguments
    /// * `buffer`: the buffer from which data will be written.
    ///
    /// # Return
    /// If successful, returns the number of bytes written to this file system node.
    /// Otherwise, returns a wasi::Errno.
    /// NOTE: Returns wasi::ERRNO_NOBUFS on Theseus file write error.
    pub fn write(&mut self, buffer: &[u8]) -> Result<usize, wasi::Errno> {
        // Verify that file descriptor has right to write.
        if self.fs_rights_base() & wasi::RIGHTS_FD_WRITE == 0 {
            return Err(wasi::ERRNO_ACCES);
        }

        match self.theseus_file_or_dir.clone() {
            FileOrDir::File(file_ref) => {
                // Check flags for append mode.
                let is_append_mode: bool = (self.fs_flags() & wasi::FDFLAGS_APPEND) != 0;

                if is_append_mode {
                    // Write to end of file.
                    let end_of_file_offset: usize = file_ref.lock().len();
                    match file_ref.lock().write_at(buffer, end_of_file_offset) {
                        Ok(bytes_written) => Ok(bytes_written),
                        Err(_) => Err(wasi::ERRNO_NOBUFS),
                    }
                } else {
                    // Write at offset of file and update offset.
                    let offset = self.offset;
                    match file_ref.lock().write_at(buffer, offset) {
                        Ok(bytes_written) => {
                            self.offset = self.offset.checked_add(bytes_written).unwrap();
                            Ok(bytes_written)
                        }
                        Err(_) => Err(wasi::ERRNO_NOBUFS),
                    }
                }
            }
            FileOrDir::Dir { .. } => Err(wasi::ERRNO_ISDIR),
        }
    }

    /// Reads data from this file system node into the given `buffer` if allowed.
    ///
    /// The number of bytes read is dictated by the length of the given `buffer`.
    ///
    /// # Arguments
    /// * `buffer`: the buffer into which the data will be read.
    ///
    /// # Return
    /// If successful, returns the number of bytes read into this file system node.
    /// Otherwise, returns a wasi::Errno.
    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize, wasi::Errno> {
        // Verify that file descriptor has right to read.
        if self.fs_rights_base() & wasi::RIGHTS_FD_READ == 0 {
            return Err(wasi::ERRNO_ACCES);
        }

        match self.theseus_file_or_dir.clone() {
            FileOrDir::File(file_ref) => {
                // Read at offset of file and update offset.
                let offset = self.offset;
                match file_ref.lock().read_at(buffer, offset) {
                    Ok(bytes_read) => {
                        self.offset = self.offset.checked_add(bytes_read).unwrap();
                        Ok(bytes_read)
                    }
                    Err(_) => Err(wasi::ERRNO_NOBUFS),
                }
            }
            FileOrDir::Dir { .. } => Err(wasi::ERRNO_ISDIR),
        }
    }

    /// Move the offset of this file system node.
    ///
    /// # Arguments
    /// * `offset`: the number of bytes to move.
    /// * `whence`: the base from which the offset is relative.
    ///
    /// # Return
    /// If successful, returns the resulting offset of this file system node.
    /// Otherwise, returns a wasi::Errno.
    pub fn seek(
        &mut self,
        delta: wasi::Filedelta,
        whence: wasi::Whence,
    ) -> Result<usize, wasi::Errno> {
        // Verify that file descriptor has right to seek.
        if self.fs_rights_base() & wasi::RIGHTS_FD_SEEK == 0 {
            return Err(wasi::ERRNO_ACCES);
        }

        match self.theseus_file_or_dir.clone() {
            FileOrDir::File(file_ref) => {
                let max_offset: usize = file_ref.lock().len();

                let signed_to_file_offset = |x: i64| -> usize {
                    cmp::min(usize::try_from(cmp::max(0, x)).unwrap(), max_offset)
                };

                let new_offset: usize = match whence {
                    wasi::WHENCE_CUR => {
                        signed_to_file_offset(i64::try_from(self.offset).unwrap() + delta)
                    }
                    wasi::WHENCE_END => {
                        signed_to_file_offset(i64::try_from(max_offset).unwrap() + delta)
                    }
                    wasi::WHENCE_SET => signed_to_file_offset(delta),
                    _ => {
                        return Err(wasi::ERRNO_SPIPE);
                    }
                };

                self.offset = new_offset;
                Ok(new_offset)
            }
            FileOrDir::Dir { .. } => Err(wasi::ERRNO_ISDIR),
        }
    }
}

/// File descriptor table.
pub struct FileDescriptorTable {
    /// HashMap from file descriptor number to POSIX-style file.
    fd_table: HashMap<wasi::Fd, PosixNodeOrStdio>,
}

impl FileDescriptorTable {
    /// Instantiates a new FileDescriptorTable with stdio entries filled.
    ///
    /// # Returns
    /// Returns a new FileDescriptorTable consisting of stdio entries.
    pub fn new() -> FileDescriptorTable {
        let mut fd_table = HashMap::new();
        fd_table.insert(wasi::FD_STDIN, PosixNodeOrStdio::Stdin);
        fd_table.insert(wasi::FD_STDOUT, PosixNodeOrStdio::Stdout);
        fd_table.insert(wasi::FD_STDERR, PosixNodeOrStdio::Stderr);
        FileDescriptorTable { fd_table }
    }

    /// Open file or directory at path in accordance to given open flags and insert in fd table.
    ///
    /// # Arguments
    /// * `path`: &str representing path of file or directory to open.
    /// * `starting_dir`: Theseus directory from which to search path from.
    /// * `lookup_flags`: flags determining behavior of path resolution.
    /// * `open_flags`: flags determining behavior of opening a file or directory.
    /// * `fs_rights`: rights applying to this file descriptor.
    /// * `fs_rights_inheriting`: rights applying to inherting file descriptors.
    /// * `fs_flags`: file descriptor flags.
    ///
    /// # Return
    /// If successful, returns resulting file descriptor number.
    /// Otherwise, returns a wasi::Errno.
    #[allow(clippy::too_many_arguments)]
    pub fn open_path(
        &mut self,
        path: &str,
        starting_dir: DirRef,
        lookup_flags: wasi::Lookupflags,
        open_flags: wasi::Oflags,
        fs_rights_base: wasi::Rights,
        fs_rights_inheriting: wasi::Rights,
        fs_flags: wasi::Fdflags,
    ) -> Result<wasi::Fd, wasi::Errno> {
        // NOTE: https://docs.rs/wasi/0.9.0+wasi-snapshot-preview1/wasi/constant.LOOKUPFLAGS_SYMLINK_FOLLOW.html
        // Unused as symlinks are currently not implemented.
        let _symlink_follow: bool = (lookup_flags & wasi::LOOKUPFLAGS_SYMLINK_FOLLOW) != 0;

        // Parse open flags.
        let create_file_if_no_exist: bool = (open_flags & wasi::OFLAGS_CREAT) != 0;
        let fail_if_not_dir: bool = (open_flags & wasi::OFLAGS_DIRECTORY) != 0;
        let fail_if_file_exists: bool = (open_flags & wasi::OFLAGS_EXCL) != 0;
        let truncate_file_to_size_zero: bool = (open_flags & wasi::OFLAGS_TRUNC) != 0;

        // Find first unused file descriptor number.
        // TODO: Potentially can implement a more efficient search data structure.
        let mut fd: wasi::Fd = FIRST_NONRESERVED_FD;
        while self.fd_table.contains_key(&fd) {
            fd += 1;
        }

        // Split path into parent directory path and base path.
        let file_path: Path = Path::new(String::from(path));
        let mut file_path_tokens: Vec<&str> = file_path.components().collect();
        file_path_tokens.truncate(file_path_tokens.len().saturating_sub(1));
        let parent_dir_path: Path = Path::new(file_path_tokens.join("/"));
        let base_name: &str = file_path.basename();
        let base_path: Path = Path::new(String::from(base_name));

        // Get parent directory.
        let parent_dir: DirRef = match parent_dir_path.get(&starting_dir) {
            Some(file_or_dir) => match file_or_dir {
                FileOrDir::File { .. } => {
                    return Err(wasi::ERRNO_NOENT);
                }
                FileOrDir::Dir(dir_ref) => dir_ref,
            },
            None => {
                return Err(wasi::ERRNO_NOENT);
            }
        };

        // Open file or directory at path in accordance to open flags.
        let opened_file_or_dir: FileOrDir = match base_path.get(&parent_dir) {
            Some(file_or_dir) => match file_or_dir {
                FileOrDir::File { .. } => {
                    if fail_if_file_exists {
                        return Err(wasi::ERRNO_EXIST);
                    } else if fail_if_not_dir {
                        return Err(wasi::ERRNO_NOTDIR);
                    } else if truncate_file_to_size_zero {
                        // HACK: Truncate file by overwriting file.
                        let new_file: FileRef =
                            MemFile::create(String::from(base_name), &parent_dir).unwrap();
                        FileOrDir::File(new_file)
                    } else {
                        file_or_dir
                    }
                }
                FileOrDir::Dir { .. } => file_or_dir,
            },
            None => {
                if create_file_if_no_exist {
                    let new_file: FileRef =
                        MemFile::create(String::from(base_name), &parent_dir).unwrap();
                    FileOrDir::File(new_file)
                } else {
                    return Err(wasi::ERRNO_NOENT);
                }
            }
        };

        // Insert POSIX-style file in file descriptor table with given rights and flags.
        self.fd_table.insert(
            fd,
            PosixNodeOrStdio::Inode(PosixNode::new(
                opened_file_or_dir,
                fs_rights_base,
                fs_rights_inheriting,
                fs_flags,
            )),
        );

        Ok(fd)
    }

    /// Close a given file descriptor if not standard I/O.
    ///
    /// # Arguments
    /// * `fd`: file descriptor number to be closed.
    ///
    /// # Return
    /// If successful, returns ().
    /// Otherwise, returns a wasi::Errno.
    pub fn close_fd(&mut self, fd: wasi::Fd) -> Result<(), wasi::Errno> {
        if self.fd_table.contains_key(&fd)
            && fd != wasi::FD_STDIN
            && fd != wasi::FD_STDOUT
            && fd != wasi::FD_STDERR
        {
            self.fd_table.remove(&fd);
            return Ok(());
        }
        Err(wasi::ERRNO_BADF)
    }

    /// Get POSIX-style file from file descriptor number.
    ///
    /// # Arguments
    /// * `fd`: file descriptor number to access.
    ///
    /// # Return
    /// Returns corresponding POSIX-style file if exists.
    pub fn get_posix_node_or_stdio(&mut self, fd: wasi::Fd) -> Option<&mut PosixNodeOrStdio> {
        self.fd_table.get_mut(&fd)
    }

    /// Get file system node from file descriptor number.
    ///
    /// This method makes it easier to access an underlying file system node from a fd number.
    ///
    /// # Arguments
    /// * `fd`: file descriptor number to access.
    ///
    /// # Return
    /// Returns corresponding file system node if exists.
    pub fn get_posix_node(&mut self, fd: wasi::Fd) -> Option<&mut PosixNode> {
        if let Some(PosixNodeOrStdio::Inode(posix_node)) = self.get_posix_node_or_stdio(fd) {
            Some(posix_node)
        } else {
            None
        }        
    }
}
