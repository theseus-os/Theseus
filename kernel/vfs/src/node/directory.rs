// TODO: We would rather these methods take &Arc<Self>, but that isn't currently
// possible.
// https://internals.rust-lang.org/t/rc-arc-borrowed-an-object-safe-version-of-rc-t-arc-t/8896/9

use crate::{File, Node, NodeKind, Path};
use alloc::{sync::Arc, vec::Vec};

/// A directory.
///
/// Consumers of this trait should use the methods implemented for the `dyn
/// Directory` type.
pub trait Directory: Node {
    /// Retrieves the entry with the specified name.
    ///
    /// `name` must not contain slashes.
    fn get_entry(&self, name: &str) -> Option<Arc<dyn Node>>;

    /// Retrieves the file entry with the specified name.
    ///
    /// `name` must not contain slashes.
    fn get_file_entry(&self, name: &str) -> Option<Arc<dyn File>> {
        match self.get_entry(name)?.as_specific() {
            NodeKind::File(file) => Some(file),
            _ => None,
        }
    }

    /// Retrieves the directory entry with the specified name.
    ///
    /// `name` must not contain slashes.
    fn get_directory_entry(&self, name: &str) -> Option<Arc<dyn Directory>> {
        match self.get_entry(name)?.as_specific() {
            NodeKind::Directory(dir) => Some(dir),
            _ => None,
        }
    }

    /// Inserts a file with the specified name, returning it.
    ///
    /// `name` must not contain slashes.
    fn insert_file_entry(self: Arc<Self>, name: &str) -> Result<Arc<dyn File>, &'static str>;

    /// Inserts a directory with the specified name, returning it.
    ///
    /// `name` must not contain slashes.
    fn insert_directory_entry(
        self: Arc<Self>,
        name: &str,
    ) -> Result<Arc<dyn Directory>, &'static str>;

    /// Removes the node with the specified name, returning it.
    ///
    /// `name` must not contain slashes.
    fn remove_entry(self: Arc<Self>, name: &str) -> Option<Arc<dyn Node>>;

    /// Returns a list of directory entries.
    fn list(&self) -> Vec<Arc<dyn Node>>;
}

// Why we use an impl block rather than including these methods in the trait:
// https://users.rust-lang.org/t/casting-arc-t-to-arc-dyn-trait/81407

// FIXME: Invalid file/dir names e.g. ., ..

impl dyn Directory {
    /// Retrieves the file system node at the specified path.
    ///
    /// The path is relative to `self`.
    pub fn get(self: Arc<Self>, path: Path) -> Option<Arc<dyn Node>> {
        let (dir_path, entry_name) = path.split_final_component();
        let dir = traverse_relative_path(self, dir_path)?;
        handle_component(dir, entry_name)
    }

    /// Retrieves the file at the specified path.
    ///
    /// The path is relative to `self`.
    pub fn get_file(self: Arc<Self>, path: Path) -> Option<Arc<dyn File>> {
        match self.get(path)?.as_specific() {
            NodeKind::File(file) => Some(file),
            _ => None,
        }
    }

    /// Retrieves the directory at the specified path.
    ///
    /// The path is relative to `self`.
    pub fn get_directory(self: Arc<Self>, path: Path) -> Option<Arc<dyn Directory>> {
        match self.get(path)?.as_specific() {
            NodeKind::Directory(dir) => Some(dir),
            _ => None,
        }
    }

    /// Inserts a file at the specified path, returning it.
    ///
    /// The path is relative to `self`.
    pub fn insert_file(self: Arc<Self>, path: Path) -> Result<Arc<dyn File>, &'static str> {
        let (dir_path, entry_name) = path.split_final_component();
        let dir = traverse_relative_path(self, dir_path).ok_or("directory doesn't exist")?;
        dir.insert_file_entry(entry_name)
    }

    /// Inserts a directory at the specified path, returning it.
    ///
    /// The path is relative to `self`.
    pub fn insert_directory(
        self: Arc<Self>,
        path: Path,
    ) -> Result<Arc<dyn Directory>, &'static str> {
        let (dir_path, entry_name) = path.split_final_component();
        // TODO: Add option to recursively insert directories?
        let dir = traverse_relative_path(self, dir_path).ok_or("directory doesn't exist")?;
        dir.insert_directory_entry(entry_name)
    }

    /// Removes the node at the specified path, returning it.
    ///
    /// The path is relative to `self`.
    pub fn remove(self: Arc<Self>, path: Path) -> Option<Arc<dyn Node>> {
        let (dir_path, entry_name) = path.split_final_component();
        let dir = traverse_relative_path(self, dir_path)?;
        dir.remove_entry(entry_name)
    }
}

fn handle_component(current_dir: Arc<dyn Directory>, component: &str) -> Option<Arc<dyn Node>> {
    match component {
        "" | "." => Some(current_dir),
        ".." => Some(current_dir.parent().unwrap_or(current_dir)),
        _ => current_dir.get_entry(component),
    }
}

/// Returns the directory at the relative path from the specified directory.
fn traverse_relative_path(
    mut current_dir: Arc<dyn Directory>,
    path: Path,
) -> Option<Arc<dyn Directory>> {
    for component in path.components() {
        current_dir = match handle_component(current_dir, component)?.as_specific() {
            NodeKind::Directory(dir) => dir,
            _ => return None,
        };
    }
    Some(current_dir)
}
