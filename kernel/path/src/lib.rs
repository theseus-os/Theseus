//! File system paths.
//!
//! This crate is designed to mimic `std::path` and as such, much of the
//! documentation and implementation is the same.

#![no_std]

extern crate alloc;

mod component;

use alloc::{borrow::ToOwned, string::String, vec, vec::Vec};
use core::{
    borrow::Borrow,
    fmt::{self, Display},
    ops::{Deref, DerefMut},
};

pub use component::{Component, Components};

/// A slice of a path.
///
/// This type is just a wrapper around a [`str`].
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Path {
    inner: str,
}

impl AsRef<Path> for Path {
    #[inline]
    fn as_ref(&self) -> &Path {
        self
    }
}

impl AsMut<Path> for Path {
    #[inline]
    fn as_mut(&mut self) -> &mut Path {
        self
    }
}

impl AsRef<str> for Path {
    #[inline]
    fn as_ref(&self) -> &str {
        &self.inner
    }
}

impl AsMut<str> for Path {
    #[inline]
    fn as_mut(&mut self) -> &mut str {
        &mut self.inner
    }
}

impl AsRef<Path> for str {
    #[inline]
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}

impl AsMut<Path> for str {
    #[inline]
    fn as_mut(&mut self) -> &mut Path {
        // SAFETY: Path has the same type layout as str. This is the same
        // implementation as std: https://github.com/rust-lang/rust/blob/f654229c27267334023a22233795b88b75fc340e/library/std/src/path.rs#L2047
        unsafe { &mut *(self as *mut str as *mut Path) }
    }
}

impl AsRef<Path> for String {
    #[inline]
    fn as_ref(&self) -> &Path {
        self[..].as_ref()
    }
}

impl AsMut<Path> for String {
    #[inline]
    fn as_mut(&mut self) -> &mut Path {
        self[..].as_mut()
    }
}

impl Display for Path {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}

impl ToOwned for Path {
    type Owned = PathBuf;

    #[inline]
    fn to_owned(&self) -> Self::Owned {
        PathBuf {
            inner: self.inner.to_owned(),
        }
    }
}

impl Path {
    /// Wraps a string slice as a path slice.
    ///
    /// This is a cost-free conversion.
    #[inline]
    pub fn new<S>(s: &S) -> &Self
    where
        S: AsRef<str> + ?Sized,
    {
        // SAFETY: Path has the same type layout as str. This is the same
        // implementation as std: https://github.com/rust-lang/rust/blob/f654229c27267334023a22233795b88b75fc340e/library/std/src/path.rs#L2041
        unsafe { &*(s.as_ref() as *const str as *const Path) }
    }

    /// Produces an iterator over the [`Component`]s of the path.
    ///
    /// When parsing the path there is a small amount of normalization:
    /// - Repeated separators are ignored, so `a/b` and `a//b` both have `a` and
    ///   `b` as components.
    /// - Occurrences of `.` are normalized away, except if they are at the
    ///   beginning of the path. For example, `a/./b`, `a/b/`, `a/b/.` and `a/b`
    ///   all have `a` and `b` as components, but `./a/b` starts with an
    ///   additional [`CurDir`] component.
    /// - A trailing slash is normalized away, `/a/b` and `/a/b/` are
    ///   equivalent.
    ///
    /// # Examples
    ///
    /// ```
    /// # use path::{Component, Path};
    /// let mut components = Path::new("/tmp/foo.txt").components();
    ///
    /// assert_eq!(components.next(), Some(Component::RootDir));
    /// assert_eq!(components.next(), Some(Component::Normal("tmp")));
    /// assert_eq!(components.next(), Some(Component::Normal("foo.txt")));
    /// assert_eq!(components.next(), None)
    /// ```
    ///
    /// [`CurDir`]: Component::CurDir
    #[inline]
    pub fn components(&self) -> Components<'_> {
        Components::new(self)
    }

    /// Returns true if the path starts with the root.
    ///
    /// # Examples
    ///
    /// ```
    /// # use path::Path;
    /// assert!(Path::new("/foo.txt").is_absolute());
    /// assert!(!Path::new("foo.txt").is_absolute());
    /// ```
    #[inline]
    pub fn is_absolute(&self) -> bool {
        self.inner.starts_with('/')
    }

    /// Creates an owned [`PathBuf`] with `path` adjoined to `self`.
    ///
    /// If `path` is absolute, it replaces the current path.
    ///
    /// # Examples
    ///
    /// ```
    /// # use path::{Path, PathBuf};
    /// assert_eq!(
    ///     Path::new("/etc").join("passwd"),
    ///     PathBuf::from("/etc/passwd")
    /// );
    /// assert_eq!(Path::new("/etc").join("/bin/sh"), PathBuf::from("/bin/sh"));
    /// ```
    #[inline]
    pub fn join<P>(&self, path: P) -> PathBuf
    where
        P: AsRef<Self>,
    {
        let mut buf = self.to_owned();
        buf.push(path);
        buf
    }

    /// Returns the path without its final component, if there is one.
    ///
    /// # Examples
    ///
    /// ```
    /// # use path::Path;
    /// let path = Path::new("/foo/bar");
    /// let parent = path.parent().unwrap();
    /// assert_eq!(parent, Path::new("/foo"));
    ///
    /// let grand_parent = parent.parent().unwrap();
    /// assert_eq!(grand_parent, Path::new("/"));
    /// assert_eq!(grand_parent.parent(), None);
    ///
    /// let relative_path = Path::new("foo/bar");
    /// let parent = relative_path.parent();
    /// assert_eq!(parent, Some(Path::new("foo")));
    /// let grand_parent = parent.and_then(Path::parent);
    /// assert_eq!(grand_parent, Some(Path::new("")));
    /// assert_eq!(grand_parent, Some(Path::new("")));
    /// let great_grand_parent = grand_parent.and_then(Path::parent);
    /// assert_eq!(great_grand_parent, None);
    /// ```
    #[inline]
    pub fn parent(&self) -> Option<&Self> {
        let mut components = self.components();

        let component = components.next_back();
        component.and_then(|p| match p {
            Component::Normal(_) | Component::CurDir | Component::ParentDir => {
                Some(components.as_path())
            }
            _ => None,
        })
    }

    /// Returns the final component of the `Path`, if there is one.
    ///
    /// If the path is a normal file, this is the file name. If it's the path of
    /// a directory, this is the directory name.
    ///
    /// Returns [`None`] if the path terminates in `..`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use path::Path;
    /// assert_eq!(Some("bin"), Path::new("/usr/bin/").file_name());
    /// assert_eq!(Some("foo.txt"), Path::new("tmp/foo.txt").file_name());
    /// assert_eq!(Some("foo.txt"), Path::new("foo.txt/.").file_name());
    /// assert_eq!(Some("foo.txt"), Path::new("foo.txt/.//").file_name());
    /// assert_eq!(None, Path::new("foo.txt/..").file_name());
    /// assert_eq!(None, Path::new("/").file_name());
    /// ```
    #[inline]
    pub fn file_name(&self) -> Option<&str> {
        self.components().next_back().and_then(|p| match p {
            Component::Normal(p) => Some(p),
            _ => None,
        })
    }

    /// Extracts the stem (non-extension) portion of [`self.file_name`].
    ///
    /// [`self.file_name`]: Path::file_name
    ///
    /// The stem is:
    ///
    /// - [`None`], if there is no file name;
    /// - The entire file name if there is no embedded `.`;
    /// - The entire file name if the file name begins with `.` and has no other
    ///   `.`s within;
    /// - Otherwise, the portion of the file name before the final `.`
    ///
    /// # Examples
    ///
    /// ```
    /// # use path::Path;
    /// assert_eq!("foo", Path::new("foo.rs").file_stem().unwrap());
    /// assert_eq!(".foo", Path::new(".foo").file_stem().unwrap());
    /// assert_eq!("foo.tar", Path::new("foo.tar.gz").file_stem().unwrap());
    /// ```
    #[inline]
    pub fn file_stem(&self) -> Option<&str> {
        self.file_name().map(|name| match name.rsplit_once('.') {
            Some((before, _)) => {
                if before.is_empty() {
                    // The file starts with a `.` and has no other `.`s within.
                    name
                } else {
                    before
                }
            }
            None => name,
        })
    }

    // TODO: Move out of path crate.

    /// Returns the file or directory at the given path.
    ///
    /// The path can be relative or absolute.
    ///
    /// If the path does not point to a file system object, `None` is returned.
    #[inline]
    pub fn get(&self, cwd: &fs_node::DirRef) -> Option<fs_node::FileOrDir> {
        let mut iter = self.components().peekable();
        let mut current = match iter.peek() {
            Some(Component::RootDir) => {
                iter.next();
                root::get_root().clone()
            }
            _ => cwd.clone(),
        };

        while let Some(component) = iter.next() {
            match component {
                Component::RootDir => current = root::get_root().clone(),
                Component::CurDir => {}
                Component::ParentDir => {
                    let temp = current.lock().get_parent_dir()?;
                    current = temp;
                }
                Component::Normal(name) => {
                    if iter.peek().is_none() {
                        return current.lock().get(name);
                    } else {
                        let temp = match current.lock().get(name) {
                            Some(fs_node::FileOrDir::Dir(directory)) => directory,
                            // Path didn't exist or had a file in the middle e.g. /dir/file/dir
                            _ => return None,
                        };
                        current = temp;
                    }
                }
            }
        }

        Some(fs_node::FileOrDir::Dir(current))
    }

    // TODO: Move out of path crate.
    /// Returns the file at the given path.
    ///
    /// The path can be relative or absolute.
    ///
    /// If the path does not point to a file, `None` is returned.
    #[inline]
    pub fn get_file(&self, cwd: &fs_node::DirRef) -> Option<fs_node::FileRef> {
        match self.get(cwd) {
            Some(fs_node::FileOrDir::File(file)) => Some(file),
            _ => None,
        }
    }

    // TODO: Move out of path crate.
    /// Returns the directory at the given path.
    ///
    /// The path can be relative or absolute.
    ///
    /// If the path does not point to a directory, `None` is returned.
    #[inline]
    pub fn get_dir(&self, cwd: &fs_node::DirRef) -> Option<fs_node::DirRef> {
        match self.get(cwd) {
            Some(fs_node::FileOrDir::Dir(dir)) => Some(dir),
            _ => None,
        }
    }

    // TODO: Move out of path crate.
    /// Returns the file or directory at the given absolute path.
    ///
    /// If the path does not point to a file system object or the path is
    /// relative, `None` is returned.
    #[inline]
    pub fn get_absolute(path: &Path) -> Option<fs_node::FileOrDir> {
        if path.is_absolute() {
            path.get(root::get_root())
        } else {
            None
        }
    }

    /// Construct a relative path from a provided base directory path to the
    /// provided path.
    #[inline]
    pub fn relative<P>(&self, base: P) -> Option<PathBuf>
    where
        P: AsRef<Path>,
    {
        let base = base.as_ref();

        if self.is_absolute() != base.is_absolute() {
            if self.is_absolute() {
                Some(self.to_owned())
            } else {
                None
            }
        } else {
            let mut ita = self.components();
            let mut itb = base.components();
            let mut comps: Vec<Component> = vec![];
            loop {
                match (ita.next(), itb.next()) {
                    (None, None) => break,
                    (Some(a), None) => {
                        comps.push(a);
                        comps.extend(ita.by_ref());
                        break;
                    }
                    (None, _) => comps.push(Component::ParentDir),
                    (Some(a), Some(b)) if comps.is_empty() && a == b => (),
                    (Some(a), Some(b)) if b == Component::CurDir => comps.push(a),
                    (Some(_), Some(b)) if b == Component::ParentDir => return None,
                    (Some(a), Some(_)) => {
                        comps.push(Component::ParentDir);
                        for _ in itb {
                            comps.push(Component::ParentDir);
                        }
                        comps.push(a);
                        comps.extend(ita.by_ref());
                        break;
                    }
                }
            }
            Some(comps.iter().map(|c| -> &Path { c.as_ref() }).collect())
        }
    }

    /// Extracts the extension (without the leading dot) of [`self.file_name`],
    /// if possible.
    ///
    /// The extension is:
    ///
    /// - [`None`], if there is no file name;
    /// - [`None`], if there is no embedded `.`;
    /// - [`None`], if the file name begins with `.` and has no other `.`s
    ///   within;
    /// - Otherwise, the portion of the file name after the final `.`
    ///
    /// [`self.file_name`]: Path::file_name
    ///
    /// # Examples
    ///
    /// ```
    /// # use path::Path;
    /// assert_eq!(None, Path::new("foo").extension());
    /// assert_eq!(None, Path::new(".foo").extension());
    /// assert_eq!("rs", Path::new("foo.rs").extension().unwrap());
    /// assert_eq!("gz", Path::new("foo.tar.gz").extension().unwrap());
    /// ```
    #[inline]
    pub fn extension(&self) -> Option<&str> {
        self.file_name()
            .and_then(|file_name| file_name.rsplit_once('.'))
            .and_then(|(before, after)| if before.is_empty() { None } else { Some(after) })
    }
}

/// An owned, mutable path.
///
/// This type is just a wrapper around a [`String`].
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PathBuf {
    inner: String,
}

impl AsRef<str> for PathBuf {
    #[inline]
    fn as_ref(&self) -> &str {
        AsRef::<Path>::as_ref(self).as_ref()
    }
}

impl AsRef<Path> for PathBuf {
    #[inline]
    fn as_ref(&self) -> &Path {
        self.deref()
    }
}

impl Borrow<Path> for PathBuf {
    #[inline]
    fn borrow(&self) -> &Path {
        self.deref()
    }
}

impl Default for PathBuf {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for PathBuf {
    type Target = Path;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.inner.deref().as_ref()
    }
}

impl DerefMut for PathBuf {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.deref_mut().as_mut()
    }
}

impl Display for PathBuf {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}

impl From<String> for PathBuf {
    #[inline]
    fn from(value: String) -> Self {
        Self { inner: value }
    }
}

impl From<PathBuf> for String {
    #[inline]
    fn from(value: PathBuf) -> Self {
        value.inner
    }
}

impl<T> From<&T> for PathBuf
where
    T: ?Sized + AsRef<str>,
{
    fn from(value: &T) -> Self {
        Self {
            inner: value.as_ref().to_owned(),
        }
    }
}

impl<P> FromIterator<P> for PathBuf
where
    P: AsRef<Path>,
{
    #[inline]
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = P>,
    {
        let mut inner = String::new();
        let mut iter = iter.into_iter().peekable();
        while let Some(path) = iter.next() {
            inner.push_str(path.as_ref().as_ref());
            if iter.peek().is_some() {
                inner.push('/');
            }
        }
        Self { inner }
    }
}

impl PathBuf {
    /// Allocates an empty `PathBuf`.
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: String::new(),
        }
    }

    /// Extends self with path.
    ///
    /// If path is absolute, it replaces the current path.
    ///
    /// # Examples
    ///
    /// Pushing a relative path extends the existing path:
    ///
    /// ```
    /// use std::path::PathBuf;
    ///
    /// let mut path = PathBuf::from("/tmp");
    /// path.push("file.bk");
    /// assert_eq!(path, PathBuf::from("/tmp/file.bk"));
    /// ```
    ///
    /// Pushing an absolute path replaces the existing path:
    ///
    /// ```
    /// use std::path::PathBuf;
    ///
    /// let mut path = PathBuf::from("/tmp");
    /// path.push("/etc");
    /// assert_eq!(path, PathBuf::from("/etc"));
    /// ```
    #[inline]
    pub fn push<P>(&mut self, path: P)
    where
        P: AsRef<Path>,
    {
        if path.as_ref().is_absolute() {
            *self = path.as_ref().to_owned();
        } else {
            self.inner.push('/');
            self.inner.push_str(path.as_ref().as_ref());
        }
    }

    /// Truncates `self` to [`self.parent`].
    ///
    /// Returns `false` and does nothing if [`self.parent`] is [`None`].
    /// Otherwise, returns `true`.
    ///
    /// [`self.parent`]: Path::parent
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::{Path, PathBuf};
    ///
    /// let mut p = PathBuf::from("/spirited/away.rs");
    ///
    /// p.pop();
    /// assert_eq!(Path::new("/spirited"), p);
    /// p.pop();
    /// assert_eq!(Path::new("/"), p);
    /// ```    
    #[inline]
    pub fn pop(&mut self) -> bool {
        match self.parent().map(|p| p.inner.len()) {
            Some(len) => {
                self.inner.truncate(len);
                true
            }
            None => false,
        }
    }
}
