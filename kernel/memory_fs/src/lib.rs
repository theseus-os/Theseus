//! In-memory file system.
//!
//! This is expected to eventually replace `memfs`.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::{
    borrow::ToOwned,
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};
use vfs::{Directory, File, FileSystem, Node, NodeKind, SeekFrom};

#[derive(Default)]
pub struct MemoryFileSystem {
    root: Arc<MemoryDirectory>,
}

impl MemoryFileSystem {
    pub fn new() -> Self {
        Self::default()
    }
}

impl FileSystem for MemoryFileSystem {
    fn root(&self) -> Arc<dyn Directory> {
        Arc::clone(&self.root) as Arc<dyn Directory>
    }
}

#[derive(Debug, Default)]
pub struct MemoryDirectory {
    name: spin::Mutex<String>,
    parent: Option<Weak<MemoryDirectory>>,
    nodes: spin::Mutex<Vec<DirectoryEntry>>,
}

impl Node for MemoryDirectory {
    fn name(&self) -> String {
        self.name.lock().clone()
    }

    fn set_name(&self, name: String) {
        *self.name.lock() = name;
    }

    fn parent(&self) -> Option<Arc<dyn Directory>> {
        match self.parent {
            Some(ref parent) => Some(Weak::clone(parent).upgrade()? as Arc<dyn Directory>),
            None => None,
        }
    }

    fn as_specific(self: Arc<Self>) -> NodeKind {
        NodeKind::Directory(self)
    }
}

impl Directory for MemoryDirectory {
    fn get_entry(&self, name: &str) -> Option<Arc<dyn Node>> {
        self.nodes
            .lock()
            .iter()
            .find(|node| node.name() == name)
            .cloned()
            .map(|node| node.into_node())
    }

    fn insert_file_entry(self: Arc<Self>, name: &str) -> Result<Arc<dyn File>, &'static str> {
        let file = Arc::new(MemoryFile {
            name: spin::Mutex::new(name.to_owned()),
            parent: Arc::downgrade(&self),
            ..Default::default()
        });

        let mut nodes = self.nodes.lock();
        if nodes.iter().any(|node| node.name() == name) {
            return Err("node with same name already exists");
        }
        nodes.push(DirectoryEntry::File(Arc::clone(&file)));

        Ok(file)
    }

    fn insert_directory_entry(
        self: Arc<Self>,
        name: &str,
    ) -> Result<Arc<dyn Directory>, &'static str> {
        let dir = Arc::new(MemoryDirectory {
            name: spin::Mutex::new(name.to_owned()),
            parent: Some(Arc::downgrade(&self)),
            ..Default::default()
        });

        let mut nodes = self.nodes.lock();
        if nodes.iter().any(|node| node.name() == name) {
            return Err("node with same name already exists");
        }
        nodes.push(DirectoryEntry::Directory(Arc::clone(&dir)));

        Ok(dir)
    }

    fn remove_entry(self: Arc<Self>, name: &str) -> Option<Arc<dyn Node>> {
        let mut nodes = self.nodes.lock();
        let index = nodes.iter().position(|node| node.name() == name)?;
        Some(nodes.remove(index).into_node())
    }

    fn list(&self) -> Vec<Arc<dyn Node>> {
        self.nodes.lock().clone().into_iter().map(|entry| entry.into_node()).collect()
    }
}

#[derive(Clone, Debug)]
enum DirectoryEntry {
    Directory(Arc<MemoryDirectory>),
    File(Arc<MemoryFile>),
}

impl DirectoryEntry {
    fn name(&self) -> String {
        match self {
            DirectoryEntry::Directory(dir) => dir.name(),
            DirectoryEntry::File(file) => file.name(),
        }
    }

    fn into_node(self) -> Arc<dyn Node> {
        match self {
            DirectoryEntry::Directory(dir) => dir,
            DirectoryEntry::File(file) => file,
        }
    }
}

#[derive(Debug, Default)]
pub struct MemoryFile {
    name: spin::Mutex<String>,
    parent: Weak<MemoryDirectory>,
    state: spin::Mutex<FileState>,
}

#[derive(Debug, Default)]
struct FileState {
    offset: usize,
    data: Vec<u8>,
}

impl Node for MemoryFile {
    fn name(&self) -> String {
        self.name.lock().clone()
    }

    fn set_name(&self, name: String) {
        *self.name.lock() = name;
    }

    fn parent(&self) -> Option<Arc<dyn Directory>> {
        Some(Weak::clone(&self.parent).upgrade()? as Arc<dyn Directory>)
    }

    fn as_specific(self: Arc<Self>) -> NodeKind {
        NodeKind::File(self)
    }
}

impl File for MemoryFile {
    fn read(&self, buffer: &mut [u8]) -> Result<usize, &'static str> {
        let mut state = self.state.lock();
        let read_len = core::cmp::min(state.data.len() - state.offset, buffer.len());

        buffer[..read_len].copy_from_slice(&state.data[state.offset..(state.offset + read_len)]);

        state.offset += read_len;
        Ok(read_len)
    }

    fn write(&self, buffer: &[u8]) -> Result<usize, &'static str> {
        let mut state = self.state.lock();
        let write_len = core::cmp::min(state.data.len() - state.offset, buffer.len());

        let offset = state.offset;
        state.data[offset..(offset + write_len)].clone_from_slice(&buffer[..write_len]);

        if write_len < buffer.len() {
            state.data.extend(&buffer[write_len..]);
        }

        state.offset += buffer.len();
        Ok(buffer.len())
    }

    fn seek(&self, offset: SeekFrom) -> Result<usize, &'static str> {
        let mut state = self.state.lock();
        state.offset = match offset {
            SeekFrom::Start(o) => o,
            SeekFrom::Current(o) => (o + state.offset as isize)
                .try_into()
                .map_err(|_| "tried to seek to negative offset")?,
            SeekFrom::End(o) => (state.data.len() as isize + o)
                .try_into()
                .map_err(|_| "tried to seek to negative offset")?,
        };
        Ok(state.offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fs_path_resolution() {
        let fs = MemoryFileSystem::new();

        let temp_dir = fs.root().insert_directory("temp".into()).unwrap();
        let foo_file = Arc::clone(&temp_dir).insert_file("foo".into()).unwrap();
        let bar_file = fs.root().insert_file("bar".into()).unwrap();

        // We only compare the data pointers of the dynamic object.
        // https://rust-lang.github.io/rust-clippy/master/index.html#vtable_address_comparisons
        macro_rules! dyn_cmp {
            ($left:expr, $right:expr) => {
                assert_eq!(Arc::as_ptr(&$left) as *const (), Arc::as_ptr(&$right) as *const ());
            };
        }

        dyn_cmp!(fs.root(), fs.root().get_directory(".".into()).unwrap());
        dyn_cmp!(fs.root(), fs.root().get_directory(".////.//./".into()).unwrap());

        dyn_cmp!(temp_dir, fs.root().get_directory("temp".into()).unwrap());
        dyn_cmp!(temp_dir, temp_dir.clone().get_directory(".".into()).unwrap());

        dyn_cmp!(foo_file, fs.root().get_file("temp/foo".into()).unwrap());
        dyn_cmp!(foo_file, temp_dir.clone().get_file("foo".into()).unwrap());

        dyn_cmp!(bar_file, fs.root().get_file("bar".into()).unwrap());
        dyn_cmp!(bar_file, temp_dir.get_file("../bar".into()).unwrap());
    }

    #[test]
    fn test_files() {
        let fs = MemoryFileSystem::new();
        let foo_file = fs.root().insert_file("foo".into()).unwrap();

        assert_eq!(foo_file.write(&[0, 1, 2, 3, 4, 5]), Ok(6));

        let mut buf = [0; 1];

        assert_eq!(foo_file.seek(SeekFrom::Current(-1)), Ok(5));
        assert_eq!(foo_file.read(&mut buf), Ok(1));
        assert_eq!(buf, [5]);

        assert_eq!(foo_file.seek(SeekFrom::Start(1)), Ok(1));
        assert_eq!(foo_file.read(&mut buf), Ok(1));
        assert_eq!(buf, [1]);
        
        // Offset is at two now.

        assert_eq!(foo_file.seek(SeekFrom::Current(2)), Ok(4));
        assert_eq!(foo_file.read(&mut buf), Ok(1));
        assert_eq!(buf, [4]);

        assert_eq!(foo_file.seek(SeekFrom::End(-3)), Ok(3));
        assert_eq!(foo_file.read(&mut buf), Ok(1));
        assert_eq!(buf, [3]);
    }
}
