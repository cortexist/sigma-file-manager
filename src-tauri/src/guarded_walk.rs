// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! A recursive directory walk that asks before it opens a directory.
//!
//! `walkdir` opens a directory the moment its entry comes up — before any `filter_entry`
//! predicate gets to see it. On the mount point of a remote filesystem whose server stopped
//! answering, that open parks the thread until the transport gives up, and a thread parked
//! there keeps the whole process from exiting. This walker consults its predicate first, so
//! a directory the caller will not enter is never touched.
//!
//! Pre-order, symlinks never followed, entries whose type cannot be read are skipped, and
//! the root itself is opened unconditionally and never yielded.

use std::fs::{FileType, ReadDir};
use std::path::{Path, PathBuf};

pub struct WalkEntry {
    pub path: PathBuf,
    /// 1 for the root's own entries.
    pub depth: usize,
    pub file_type: FileType,
}

pub struct GuardedWalk<F>
where
    F: FnMut(&Path, usize, bool) -> bool,
{
    pending_root: Option<PathBuf>,
    stack: Vec<(ReadDir, usize)>,
    max_depth: usize,
    admit: F,
}

impl<F> GuardedWalk<F>
where
    F: FnMut(&Path, usize, bool) -> bool,
{
    /// `admit(path, depth, is_dir)` is asked once per entry, before it is yielded and before
    /// a directory is opened; `false` drops the entry and everything under it. Nothing
    /// deeper than `max_depth` is produced (`usize::MAX` for no limit).
    pub fn new(root: impl Into<PathBuf>, max_depth: usize, admit: F) -> Self {
        Self {
            pending_root: Some(root.into()),
            stack: Vec::new(),
            max_depth,
            admit,
        }
    }
}

impl<F> Iterator for GuardedWalk<F>
where
    F: FnMut(&Path, usize, bool) -> bool,
{
    type Item = WalkEntry;

    fn next(&mut self) -> Option<WalkEntry> {
        if let Some(root) = self.pending_root.take() {
            if let Ok(read_dir) = std::fs::read_dir(&root) {
                self.stack.push((read_dir, 0));
            }
        }

        loop {
            let (read_dir, parent_depth) = self.stack.last_mut()?;
            let depth = *parent_depth + 1;

            let Some(entry) = read_dir.next() else {
                self.stack.pop();
                continue;
            };
            let Ok(entry) = entry else {
                continue;
            };
            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            let path = entry.path();
            let is_dir = file_type.is_dir();

            if !(self.admit)(&path, depth, is_dir) {
                continue;
            }

            if is_dir && depth < self.max_depth {
                if let Ok(child) = std::fs::read_dir(&path) {
                    self.stack.push((child, depth));
                }
            }

            return Some(WalkEntry {
                path,
                depth,
                file_type,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::fs;

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        fs::create_dir_all(root.join("a/deep/deeper")).unwrap();
        fs::create_dir_all(root.join("sealed/inside")).unwrap();
        fs::write(root.join("top.txt"), b"1").unwrap();
        fs::write(root.join("a/one.txt"), b"1").unwrap();
        fs::write(root.join("a/deep/two.txt"), b"1").unwrap();
        fs::write(root.join("sealed/secret.txt"), b"1").unwrap();
        dir
    }

    fn names(entries: Vec<WalkEntry>, root: &Path) -> HashSet<String> {
        entries
            .into_iter()
            .map(|entry| {
                entry
                    .path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    #[test]
    fn a_rejected_directory_is_neither_yielded_nor_entered() {
        let dir = tree();
        let root = dir.path().to_path_buf();
        let asked = std::cell::RefCell::new(Vec::new());

        let seen = GuardedWalk::new(&root, usize::MAX, |path, _depth, _is_dir| {
            asked.borrow_mut().push(path.to_path_buf());
            !path.ends_with("sealed")
        })
        .collect();
        let seen = names(seen, &root);

        assert!(seen.contains("a/deep/deeper"));
        assert!(seen.contains("top.txt"));
        assert!(!seen.contains("sealed"));
        assert!(!seen.iter().any(|name| name.starts_with("sealed/")));
        // Nothing under the rejected directory was ever asked about: it was never opened.
        let sealed = root.join("sealed");
        assert!(asked.borrow().iter().any(|path| *path == sealed));
        assert!(!asked
            .borrow()
            .iter()
            .any(|path| *path != sealed && path.starts_with(&sealed)));
    }

    #[test]
    fn max_depth_bounds_the_walk_and_entries_come_in_pre_order() {
        let dir = tree();
        let root = dir.path().to_path_buf();

        let entries: Vec<WalkEntry> =
            GuardedWalk::new(&root, 2, |_path, _depth, _is_dir| true).collect();
        assert!(entries.iter().all(|entry| entry.depth <= 2));
        let seen = names(entries, &root);
        assert!(seen.contains("a/deep"));
        assert!(!seen.contains("a/deep/deeper"));

        let order: Vec<PathBuf> = GuardedWalk::new(&root, usize::MAX, |p, _, _| {
            p.file_name().is_some_and(|name| name != "sealed")
        })
        .map(|entry| entry.path)
        .collect();
        let position = |name: &str| order.iter().position(|path| path.ends_with(name)).unwrap();
        assert!(position("a") < position("a/one.txt"));
        assert!(position("a/deep") < position("a/deep/two.txt"));
    }

    #[test]
    fn a_missing_root_yields_nothing() {
        let dir = tree();
        let missing = dir.path().join("nope");
        assert_eq!(
            GuardedWalk::new(missing, usize::MAX, |_, _, _| true).count(),
            0
        );
    }
}
