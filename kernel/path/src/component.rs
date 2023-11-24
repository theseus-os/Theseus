/// The implementation is heavily based on `std`.
use crate::Path;

pub const SEPARATOR: char = '/';
pub const SEPARATOR_STR: &str = "/";
pub const CURRENT_DIR_WITH_SEPARATOR: &str = "./";

/// An iterator over the components of a path.
///
/// This struct is created by the [`components`] method on Path. See its
/// documentation for more details.
///
/// [`components`]: Path::components
#[derive(Clone, PartialEq, PartialOrd, Debug)]
pub struct Components<'a> {
    path: &'a Path,
    front: State,
    back: State,
}

#[derive(Copy, Clone, PartialEq, PartialOrd, Debug)]
enum State {
    StartDir = 0,
    Body = 1,
    Done = 2,
}

impl<'a> Iterator for Components<'a> {
    type Item = Component<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        while !self.finished() {
            match self.front {
                State::StartDir => {
                    self.front = State::Body;
                    if self.path.inner.starts_with(SEPARATOR) {
                        // Trim the starting slash. Even if there are subsequent slashes, they will
                        // be ignored as we change our state to State::Body.
                        self.path = Path::new(&self.path.inner[1..]);
                        return Some(Component::RootDir);
                    } else if self.include_cur_dir() {
                        // Trim the dot.
                        self.path = Path::new(&self.path.inner[1..]);
                        return Some(Component::CurDir);
                    }
                }
                State::Body if !self.path.inner.is_empty() => {
                    let (rest, component) = self.peek();
                    self.path = rest;
                    if component.is_some() {
                        return component;
                    }
                }
                State::Body => {
                    self.front = State::Done;
                }
                State::Done => unreachable!(),
            }
        }
        None
    }
}

impl<'a> DoubleEndedIterator for Components<'a> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        while !self.finished() {
            match self.back {
                State::Body if self.path.inner.len() > self.len_before_body() => {
                    let (rest, component) = self.peek_back();
                    self.path = rest;
                    if component.is_some() {
                        return component;
                    }
                }
                State::Body => {
                    self.back = State::StartDir;
                }
                State::StartDir => {
                    self.back = State::Done;
                    if self.has_root() {
                        self.path = Path::new(&self.path.inner[..self.path.inner.len() - 1]);
                        return Some(Component::RootDir);
                    } else if self.include_cur_dir() {
                        self.path = Path::new(&self.path.inner[..self.path.inner.len() - 1]);
                        return Some(Component::CurDir);
                    }
                }
                State::Done => unreachable!(),
            }
        }
        None
    }
}

impl<'a> Components<'a> {
    pub(crate) fn new(path: &'a Path) -> Self {
        Self {
            path,
            front: State::StartDir,
            back: State::Body,
        }
    }

    /// Extracts a slice corresponding to the portion of the path remaining for
    /// iteration.
    #[inline]
    pub fn as_path(&self) -> &'a Path {
        let mut components = self.clone();
        if components.front == State::Body {
            components.trim_left();
        }
        if components.back == State::Body {
            components.trim_right();
        }
        components.path
    }

    fn include_cur_dir(&self) -> bool {
        self.path == ".".as_ref() || self.path.inner.starts_with(CURRENT_DIR_WITH_SEPARATOR)
    }

    fn has_root(&self) -> bool {
        self.path.inner.starts_with(SEPARATOR)
    }

    fn len_before_body(&self) -> usize {
        let root = if self.front == State::StartDir && self.has_root() {
            1
        } else {
            0
        };
        let cur_dir = if self.front == State::StartDir && self.include_cur_dir() {
            1
        } else {
            0
        };
        root + cur_dir
    }

    fn trim_left(&mut self) {
        while !self.path.inner.is_empty() {
            let (rest, comp) = self.peek();
            if comp.is_some() {
                return;
            } else {
                self.path = rest;
            }
        }
    }

    fn peek(&self) -> (&'a Path, Option<Component<'a>>) {
        match self.path.inner.split_once(SEPARATOR) {
            Some((next, rest)) => (Path::new(rest), component(next)),
            None => (Path::new(""), component(self.path.as_ref())),
        }
    }

    fn trim_right(&mut self) {
        while self.path.inner.len() > self.len_before_body() {
            let (rest, comp) = self.peek_back();
            if comp.is_some() {
                return;
            } else {
                self.path = rest;
            }
        }
    }

    fn peek_back(&self) -> (&'a Path, Option<Component<'a>>) {
        match self.path.inner[self.len_before_body()..].rsplit_once(SEPARATOR) {
            Some((rest, next)) => (
                Path::new(&self.path.inner[..(self.len_before_body() + rest.len())]),
                component(next),
            ),
            None => (
                Path::new(&self.path.inner[..self.len_before_body()]),
                component(&self.path.inner[self.len_before_body()..]),
            ),
        }
    }

    fn finished(&self) -> bool {
        self.front == State::Done || self.back == State::Done || self.front > self.back
    }
}

fn component(component: &str) -> Option<Component<'_>> {
    match component {
        "." | "" => None,
        ".." => Some(Component::ParentDir),
        _ => Some(Component::Normal(component)),
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Component<'a> {
    RootDir,
    CurDir,
    ParentDir,
    Normal(&'a str),
}

impl<'a> AsRef<Path> for Component<'a> {
    #[inline]
    fn as_ref(&self) -> &'a Path {
        // TODO: Why is this a lifetime error?
        // Path::new(AsRef::<str>::as_ref(self))
        match self {
            Component::RootDir => Path::new(SEPARATOR_STR),
            Component::CurDir => Path::new("."),
            Component::ParentDir => Path::new(".."),
            Component::Normal(path) => Path::new(*path),
        }
    }
}

impl<'a> AsRef<str> for Component<'a> {
    #[inline]
    fn as_ref(&self) -> &'a str {
        match self {
            Component::RootDir => SEPARATOR_STR,
            Component::CurDir => ".",
            Component::ParentDir => "..",
            Component::Normal(path) => path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_components_iter_front() {
        let mut components = Path::new("/tmp/foo/bar.txt").components();
        assert_eq!(components.as_path(), "/tmp/foo/bar.txt".as_ref());
        assert_eq!(components.next(), Some(Component::RootDir));
        assert_eq!(components.as_path(), "tmp/foo/bar.txt".as_ref());
        assert_eq!(components.next(), Some(Component::Normal("tmp")));
        assert_eq!(components.as_path(), "foo/bar.txt".as_ref());
        assert_eq!(components.next(), Some(Component::Normal("foo")));
        assert_eq!(components.as_path(), "bar.txt".as_ref());
        assert_eq!(components.next(), Some(Component::Normal("bar.txt")));
        assert_eq!(components.as_path(), "".as_ref());
        assert_eq!(components.next(), None);

        let mut components = Path::new("//tmp//../foo/./").components();
        assert_eq!(components.as_path(), "//tmp//../foo".as_ref());
        assert_eq!(components.next(), Some(Component::RootDir));
        assert_eq!(components.as_path(), "tmp//../foo".as_ref());
        assert_eq!(components.next(), Some(Component::Normal("tmp")));
        assert_eq!(components.as_path(), "../foo".as_ref());
        assert_eq!(components.next(), Some(Component::ParentDir));
        assert_eq!(components.as_path(), "foo".as_ref());
        assert_eq!(components.next(), Some(Component::Normal("foo")));
        assert_eq!(components.as_path(), "".as_ref());
        assert_eq!(components.next(), None);

        let mut components = Path::new("..//./foo").components();
        assert_eq!(components.as_path(), "..//./foo".as_ref());
        assert_eq!(components.next(), Some(Component::ParentDir));
        assert_eq!(components.as_path(), "foo".as_ref());
        assert_eq!(components.next(), Some(Component::Normal("foo")));
        assert_eq!(components.as_path(), "".as_ref());
        assert_eq!(components.next(), None);
    }

    #[test]
    fn test_components_iter_back() {
        let mut components = Path::new("/tmp/foo/bar.txt").components();
        assert_eq!(components.as_path(), "/tmp/foo/bar.txt".as_ref());
        assert_eq!(components.next_back(), Some(Component::Normal("bar.txt")));
        assert_eq!(components.as_path(), "/tmp/foo".as_ref());
        assert_eq!(components.next_back(), Some(Component::Normal("foo")));
        assert_eq!(components.as_path(), "/tmp".as_ref());
        assert_eq!(components.next_back(), Some(Component::Normal("tmp")));
        assert_eq!(components.as_path(), "/".as_ref());
        assert_eq!(components.next_back(), Some(Component::RootDir));
        assert_eq!(components.as_path(), "".as_ref());
        assert_eq!(components.next_back(), None);

        let mut components = Path::new("//tmp//../foo/./").components();
        assert_eq!(components.as_path(), "//tmp//../foo".as_ref());
        assert_eq!(components.next_back(), Some(Component::Normal("foo")));
        assert_eq!(components.as_path(), "//tmp//..".as_ref());
        assert_eq!(components.next_back(), Some(Component::ParentDir));
        assert_eq!(components.as_path(), "//tmp".as_ref());
        assert_eq!(components.next_back(), Some(Component::Normal("tmp")));
        assert_eq!(components.as_path(), "/".as_ref());
        assert_eq!(components.next_back(), Some(Component::RootDir));
        assert_eq!(components.as_path(), "".as_ref());
        assert_eq!(components.next_back(), None);

        let mut components = Path::new("..//./foo").components();
        assert_eq!(components.as_path(), "..//./foo".as_ref());
        assert_eq!(components.next_back(), Some(Component::Normal("foo")));
        assert_eq!(components.as_path(), "..".as_ref());
        assert_eq!(components.next_back(), Some(Component::ParentDir));
        assert_eq!(components.as_path(), "".as_ref());
        assert_eq!(components.next_back(), None);
    }

    #[test]
    fn test_components_iter_front_back() {
        let mut components = Path::new("/tmp/foo/bar.txt").components();
        assert_eq!(components.as_path(), "/tmp/foo/bar.txt".as_ref());
        assert_eq!(components.next(), Some(Component::RootDir));
        assert_eq!(components.as_path(), "tmp/foo/bar.txt".as_ref());
        assert_eq!(components.next_back(), Some(Component::Normal("bar.txt")));
        assert_eq!(components.as_path(), "tmp/foo".as_ref());
        assert_eq!(components.next(), Some(Component::Normal("tmp")));
        assert_eq!(components.as_path(), "foo".as_ref());
        assert_eq!(components.next_back(), Some(Component::Normal("foo")));
        assert_eq!(components.as_path(), "".as_ref());
        assert_eq!(components.next_back(), None);

        let mut components = Path::new("//tmp//../foo/./").components();
        assert_eq!(components.as_path(), "//tmp//../foo".as_ref());
        assert_eq!(components.next(), Some(Component::RootDir));
        assert_eq!(components.as_path(), "tmp//../foo".as_ref());
        assert_eq!(components.next_back(), Some(Component::Normal("foo")));
        assert_eq!(components.as_path(), "tmp//..".as_ref());
        assert_eq!(components.next_back(), Some(Component::ParentDir));
        assert_eq!(components.as_path(), "tmp".as_ref());
        assert_eq!(components.next(), Some(Component::Normal("tmp")));
        assert_eq!(components.as_path(), "".as_ref());
        assert_eq!(components.next_back(), None);

        let mut components = Path::new("..//./foo").components();
        assert_eq!(components.as_path(), "..//./foo".as_ref());
        assert_eq!(components.next_back(), Some(Component::Normal("foo")));
        assert_eq!(components.as_path(), "..".as_ref());
        assert_eq!(components.next(), Some(Component::ParentDir));
        assert_eq!(components.as_path(), "".as_ref());
        assert_eq!(components.next_back(), None);
    }
}
