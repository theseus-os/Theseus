//! A minimal interface to access Theseus's environment variables,
//! primarily the current working directory path.
use core2::io;
use crate::path::PathBuf;
use theseus_fs_node::{FileOrDir, DirRef};

/// Returns a Theseus-specific reference to the current working directory.
pub fn current_dir() -> io::Result<DirRef> {
    theseus_task::get_my_current_task()
        .ok_or(io::Error::new(
            io::ErrorKind::Other, "failed to get Theseus current task")
        )
        .and_then(|task| 
            match theseus_path::Path::get_absolute(
                &task.get_env().lock().cwd().into()
            ) {
                Some(FileOrDir::File(_)) => Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Theseus current working directory path pointed to a file..."
                )),
                Some(FileOrDir::Dir(d)) => Ok(d),
                None => Err(io::ErrorKind::NotFound.into()),
            }
        )
}

/// Returns the path of the current working directory.
pub fn current_dir_path() -> io::Result<PathBuf> {
    theseus_task::get_my_current_task()
        .ok_or(io::Error::new(io::ErrorKind::Other, "failed to get Theseus current task"))
        .map(|task| task.get_env()
            .lock()
            .cwd()
            .into()
        )
}
