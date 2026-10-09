//! `os.path` as cpplint uses it, with Python's exact semantics.
//!
//! cpplint derives header-guard names and include classifications from
//! string manipulation of paths (`os.path.split`, `splitext`, `normpath`,
//! `commonprefix`); the guard a file is expected to carry depends on these
//! details, so they are reproduced here rather than approximated with
//! `std::path`.

use std::path::Path;

/// `os.path.split(path)`: everything up to the last slash (trailing slashes
/// stripped unless the head is nothing but slashes) and the rest.
pub fn split(path: &str) -> (&str, &str) {
    let i = path.rfind('/').map_or(0, |p| p + 1);
    let (head, tail) = path.split_at(i);
    let head = if !head.is_empty() && head.chars().any(|c| c != '/') {
        head.trim_end_matches('/')
    } else {
        head
    };
    (head, tail)
}

/// `os.path.dirname(path)`.
pub fn dirname(path: &str) -> &str {
    split(path).0
}

/// `os.path.splitext(path)`: the extension starts at the last dot of the
/// last component, unless that component is nothing but leading dots.
pub fn splitext(path: &str) -> (&str, &str) {
    let sep_index = path.rfind('/').map_or(0, |p| p + 1);
    let Some(dot_index) = path.rfind('.') else {
        return (path, "");
    };
    if dot_index < sep_index {
        return (path, "");
    }
    let name = &path[sep_index..dot_index];
    if name.chars().all(|c| c == '.') {
        return (path, "");
    }
    path.split_at(dot_index)
}

/// `os.path.normpath(path)` for POSIX paths: collapses `//`, `.` and `..`
/// lexically, without touching the file system.
pub fn normpath(path: &str) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    let initial_slashes = if path.starts_with("//") && !path.starts_with("///") {
        2
    } else {
        usize::from(path.starts_with('/'))
    };
    let mut comps: Vec<&str> = Vec::new();
    for comp in path.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp != ".." {
            comps.push(comp);
        } else if comps.last().is_some_and(|last| *last != "..") {
            comps.pop();
        } else if initial_slashes == 0 {
            comps.push(comp);
        }
    }
    let mut out = "/".repeat(initial_slashes);
    out.push_str(&comps.join("/"));
    if out.is_empty() { ".".to_string() } else { out }
}

/// `os.path.join(a, b)`.
pub fn join(a: &str, b: &str) -> String {
    if b.starts_with('/') {
        b.to_string()
    } else if a.is_empty() || a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// `os.path.join(*parts)`.
pub fn join_all(parts: &[String]) -> String {
    let mut out = String::new();
    for part in parts {
        out = join(&out, part);
    }
    out
}

/// `os.path.abspath(path)`: made absolute against the working directory,
/// then normalized lexically (symlinks are not resolved).
pub fn abspath(path: &str) -> String {
    if path.starts_with('/') {
        return normpath(path);
    }
    let cwd = std::env::current_dir().map_or_else(|_| "/".to_string(), |d| d.display().to_string());
    normpath(&join(&cwd, path))
}

/// `os.path.commonprefix([a, b])`: the longest common prefix, character by
/// character (not component by component).
pub fn commonprefix<'a>(a: &'a str, b: &str) -> &'a str {
    let mut end = 0;
    for ((ia, ca), cb) in a.char_indices().zip(b.chars()) {
        if ca != cb {
            break;
        }
        end = ia + ca.len_utf8();
    }
    &a[..end]
}

/// `os.path.relpath(path, start)` for absolute inputs.
pub fn relpath(path: &str, start: &str) -> String {
    let components = |p: &str| -> Vec<String> {
        abspath(p)
            .split('/')
            .filter(|c| !c.is_empty())
            .map(str::to_string)
            .collect()
    };
    let path_list = components(path);
    let start_list = components(start);
    let common = path_list
        .iter()
        .zip(start_list.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut rel: Vec<String> = vec!["..".to_string(); start_list.len() - common];
    rel.extend(path_list[common..].iter().cloned());
    if rel.is_empty() {
        ".".to_string()
    } else {
        rel.join("/")
    }
}

/// `os.path.exists(path)`.
pub fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// `os.path.isfile(path)`.
pub fn isfile(path: &str) -> bool {
    Path::new(path).is_file()
}

/// cpplint's `PathSplitToList`: a path as its components, `/` first for an
/// absolute path.
pub fn split_to_list(path: &str) -> Vec<String> {
    let mut list = Vec::new();
    let mut path = path.to_string();
    loop {
        let (head, tail) = split(&path);
        if head == path {
            list.push(head.to_string());
            break;
        }
        if tail == path {
            list.push(tail.to_string());
            break;
        }
        let head = head.to_string();
        list.push(tail.to_string());
        path = head;
    }
    list.reverse();
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_and_splitext_follow_python() {
        assert_eq!(split("a/b/c.h"), ("a/b", "c.h"));
        assert_eq!(split("/c.h"), ("/", "c.h"));
        assert_eq!(split("c.h"), ("", "c.h"));
        assert_eq!(split("a//b"), ("a", "b"));
        assert_eq!(splitext("foo.tar.gz"), ("foo.tar", ".gz"));
        assert_eq!(splitext(".cpplint"), (".cpplint", ""));
        assert_eq!(splitext("a.b/c"), ("a.b/c", ""));
        assert_eq!(splitext("..foo"), ("..foo", ""));
    }

    #[test]
    fn normpath_and_join_follow_python() {
        assert_eq!(normpath("a/./b/../c//d/"), "a/c/d");
        assert_eq!(normpath("/../a"), "/a");
        assert_eq!(normpath("../a"), "../a");
        assert_eq!(normpath(""), ".");
        assert_eq!(join("a", "b"), "a/b");
        assert_eq!(join("a/", "b"), "a/b");
        assert_eq!(join("a", "/b"), "/b");
        assert_eq!(join("", "b"), "b");
        assert_eq!(join_all(&["/".into(), "a".into(), "b".into()]), "/a/b");
    }

    #[test]
    fn split_to_list_follows_cpplint() {
        assert_eq!(split_to_list("/a/b/c"), vec!["/", "a", "b", "c"]);
        assert_eq!(split_to_list("a/b"), vec!["a", "b"]);
        assert_eq!(split_to_list("a"), vec!["a"]);
        assert_eq!(commonprefix("/x/y/z", "/x/yy"), "/x/y");
        assert_eq!(relpath("/a/b/c", "/a/d"), "../b/c");
        assert_eq!(relpath("/a/b", "/a/b"), ".");
    }
}
