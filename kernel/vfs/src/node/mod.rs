mod directory;
mod file;

pub use directory::Directory;
pub use file::{File, SeekFrom};

use crate::{file_system, Path};
use alloc::{string::String, sync::Arc};

/// A file system node.
///
/// Types implementing this trait should also implement either `File` or
/// `Directory`.
pub trait Node {
    /// Returns the node's name.
    fn name(&self) -> String;

    /// Sets the node's name.
    fn set_name(&self, name: String);

    /// Returns the node's parent.
    ///
    /// A return value of [`None`] either indicates `self` is a root directory,
    /// or its parent has been removed.
    fn parent(&self) -> Option<Arc<dyn Directory>>;

    /// Returns the specific node kind.
    fn as_specific(self: Arc<Self>) -> NodeKind;
}

/// A specific file system node kind.
pub enum NodeKind {
    Directory(Arc<dyn Directory>),
    File(Arc<dyn File>),
}

/// Gets the node at the `path`.
///
/// The path consists of a file system id, followed by a colon, followed by the
/// path. For example, `tmp:/a/b` or `nvme:/foo/bar`.
pub fn get_node(path: Path) -> Option<Arc<dyn Node>> {
    let (fs_id, mut path) = path.as_ref().split_once(':')?;
    let root = file_system(fs_id)?.root();
    // Path must be absolute
    path = match path.strip_prefix('/') {
        Some(path) => path,
        None => return None,
    };
    root.get(Path::new(path))
}
