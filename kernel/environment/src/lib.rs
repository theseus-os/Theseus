#![no_std]

extern crate alloc;

use alloc::{
    string::String,
    sync::Arc,
};
use fs_node::DirRef;
use hashbrown::HashMap;

/// A structure that contains environment state for a given `Task` or group of `Task`s.
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
