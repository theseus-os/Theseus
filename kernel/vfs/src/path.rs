/// A file system path.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Path<'a> {
    inner: &'a str,
}

/// An iterator over the components of a [`Path`].
pub struct Components<'a> {
    inner: core::str::Split<'a, char>,
}

impl<'a> Iterator for Components<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

impl<'a> Path<'a> {
    pub fn new(inner: &'a str) -> Self {
        Self { inner }
    }

    pub fn components(&self) -> Components<'a> {
        let inner = match self.inner.strip_prefix('/') {
            Some(inner) => inner,
            None => self.inner,
        };
        Components { inner: inner.split('/') }
    }

    pub fn split_final_component(&self) -> (Path, &str) {
        // TODO: What do we do about trailing slashes?
        let (path, final_component) = self.inner.rsplit_once('/').unwrap_or(("", self.inner));
        (Path::new(path), final_component)
    }
}

impl<'a> AsRef<str> for Path<'a> {
    fn as_ref(&self) -> &str {
        self.inner
    }
}

impl<'a> From<&'a str> for Path<'a> {
    fn from(inner: &'a str) -> Self {
        Self::new(inner)
    }
}

impl<'a> From<Path<'a>> for &'a str {
    fn from(path: Path<'a>) -> Self {
        path.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_components() {
        let path = Path::new("/if/pirus/and/crips");
        assert_eq!(path.components().collect::<Vec<_>>(), vec!["if", "pirus", "and", "crips"]);

        let path = Path::new("all/got/along/theyd/probably");
        assert_eq!(
            path.components().collect::<Vec<_>>(),
            vec!["all", "got", "along", "theyd", "probably"]
        );
    }

    #[test]
    fn test_split_final_component() {
        let path = Path::new("got");
        assert_eq!(path.split_final_component(), (Path::new(""), "got"));

        let path = Path::new("/me");
        assert_eq!(path.split_final_component(), (Path::new(""), "me"));

        let path = Path::new("/down/by/the");
        assert_eq!(path.split_final_component(), (Path::new("/down/by"), "the"));

        let path = Path::new("/end/of/the/song");
        assert_eq!(path.split_final_component(), (Path::new("/end/of/the"), "song"));

        let path = Path::new("seems/like/the/whole");
        assert_eq!(path.split_final_component(), (Path::new("seems/like/the"), "whole"));

        let path = Path::new("city/go/against/me");
        assert_eq!(path.split_final_component(), (Path::new("city/go/against"), "me"));
    }

    #[test]
    fn test_path_str_conversions() {
        let path = Path::new("/every/time/im/in/the/street/i/hear");
        let string: &str = path.into();
        assert_eq!(string, "/every/time/im/in/the/street/i/hear");
        assert_eq!(string, path.as_ref());
        assert_eq!(Path::from(string), path);

        let path = Path::new("yawk/yawk/yawk/yawk");
        let string: &str = path.into();
        assert_eq!(string, "yawk/yawk/yawk/yawk");
        assert_eq!(string, path.as_ref());
        assert_eq!(Path::from(string), path);
    }
}
