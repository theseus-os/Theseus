#![no_std]

extern crate alloc;

use alloc::{string::String, sync::Arc};
use core::fmt;
use fs_node::{DirRef, FileOrDir};
use hashbrown::HashMap;
use path::Path;

/// A structure that contains environment state for a given `Task` or group of
/// `Task`s.
///
/// A default environment can be created with the following state:
/// * The working directory is the `root` directory.
pub struct Environment {
    /// The "current working directory", i.e.,
    /// where a task's relative path begins upon first execution.
    pub working_dir: DirRef,
    pub variables: HashMap<String, String>,
}

impl Environment {
    /// Returns the absolute file path of the current working directory.
    #[doc(alias("working", "dir", "current", "getcwd"))]
    pub fn cwd(&self) -> String {
        let wd = self.working_dir.lock();
        wd.get_absolute_path()
    }

    /// Changes the current working directory.
    #[doc(alias("change"))]
    pub fn chdir(&mut self, path: &Path) -> Result<()> {
        match path.get(&self.working_dir) {
            Some(FileOrDir::Dir(dir_ref)) => {
                self.working_dir = dir_ref;
                Ok(())
            }
            Some(FileOrDir::File(_)) => Err(Error::NotADirectory),
            None => Err(Error::NotFound),
        }
    }

    /// Returns the value of the environment variable with the given `key`.
    #[doc(alias("var"))]
    pub fn get(&self, key: &str) -> Option<&String> {
        self.variables.get(key)
    }

    /// Sets an environment variable with the given `key` and `value`.
    #[doc(alias("set_var"))]
    pub fn set(&mut self, key: String, value: String) {
        self.variables.insert(key, value);
    }

    /// Unsets the environment variable with the given `key`.
    #[doc(alias("remove_var"))]
    pub fn unset(&mut self, key: &str) {
        self.variables.remove(key);
    }
}

impl Default for Environment {
    fn default() -> Environment {
        Environment {
            working_dir: Arc::clone(root::get_root()),
            variables: HashMap::new(),
        }
    }
}

/// A specialized [`Result`] type for environment operations.
///
/// [`Result`]: core::result::Result
pub type Result<T> = core::result::Result<T, Error>;

/// The error type for environment operations.
pub enum Error {
    /// A filesystem node was, unexpectedly, not a directory.
    NotADirectory,
    /// A filesystem node wasn't found.
    NotFound,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::NotADirectory => "not a directory",
            Error::NotFound => "entity not found",
        })
    }
}
