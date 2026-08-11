// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

//! Reading and undoing what deleting a file did.
//!
//! Deleting already goes through the same crate (see `file_operations::delete_items`), which on
//! Linux means the FreeDesktop trash: the file is moved into a trash directory and a
//! `.trashinfo` file beside it records where it came from and when it went. That record is the
//! only thing that makes a deletion reversible, and it is not something a directory listing can
//! show — the file in `files/` may have been renamed to avoid a collision, and its original path
//! lives in the sidecar. So the trash is read through the crate rather than as a folder.
//!
//! Items are addressed by the `id` the crate assigns, which on Linux is the absolute path of the
//! `.trashinfo` file. The frontend holds those ids and hands them back, so nothing here keeps
//! state between calls; an id that has gone stale simply no longer appears in a fresh listing,
//! and operating on one is reported as a miss rather than silently doing nothing.
//!
//! Enumerating the trash is a Windows and FreeDesktop capability. macOS has a trash but no
//! supported way to read it, which the crate reflects by not compiling `os_limited` there — so
//! the commands exist on every platform and say plainly when the platform cannot answer.

use serde::Serialize;

/// One item in the trash, flattened for the frontend.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashEntry {
    /// Opaque to the frontend; hand it back to restore or purge this item.
    pub id: String,
    pub name: String,
    /// Where the item will go back to, which is also what identifies it to a person.
    pub original_path: String,
    pub original_parent: String,
    /// Milliseconds since the epoch, to match `DirEntry::modified_time`.
    pub deleted_time: i64,
    pub size: u64,
    /// Directories report an entry count instead of a size; see `item_count`.
    pub is_dir: bool,
    pub item_count: Option<u64>,
}

/// What one item actually occupies on disk.
///
/// Separate from the listing because it is the expensive half. The trash API reports a folder
/// as a count of what it directly contains, not a size, so the only way to answer "how much
/// space is this using" is to walk it — and a trashed build directory can be a hundred thousand
/// files. The listing stays instant and this arrives after it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashEntrySize {
    pub id: String,
    pub size: u64,
}

/// The outcome of restoring or purging, counted rather than abandoned on first failure.
///
/// A selection is a set of independent items, and one of them colliding with a file that has
/// since reappeared at its original path says nothing about the rest. The counts are what the
/// caller reports; `error` describes the last thing that went wrong, for when something did.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashOperationResult {
    pub success: bool,
    pub completed_count: u32,
    pub failed_count: u32,
    pub error: Option<String>,
}

impl TrashOperationResult {
    fn from_counts(completed_count: u32, failed_count: u32, last_error: Option<String>) -> Self {
        Self {
            success: failed_count == 0 && last_error.is_none(),
            completed_count,
            failed_count,
            error: last_error,
        }
    }
}

/// Whether this platform can enumerate its trash at all, so the interface can leave the door
/// closed rather than offer a room that is not there.
#[tauri::command]
pub fn trash_is_listable() -> bool {
    platform::IS_LISTABLE
}

#[tauri::command]
pub async fn trash_list() -> Result<Vec<TrashEntry>, String> {
    platform::list().await
}

/// What each item occupies on disk, folders measured by walking them.
#[tauri::command]
pub async fn trash_sizes() -> Result<Vec<TrashEntrySize>, String> {
    platform::sizes().await
}

#[tauri::command]
pub async fn trash_restore(ids: Vec<String>) -> Result<TrashOperationResult, String> {
    platform::restore(ids).await
}

#[tauri::command]
pub async fn trash_purge(ids: Vec<String>) -> Result<TrashOperationResult, String> {
    platform::purge(ids).await
}

#[tauri::command]
pub async fn trash_empty() -> Result<TrashOperationResult, String> {
    platform::empty().await
}

#[cfg(any(
    windows,
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
mod platform {
    use super::{TrashEntry, TrashEntrySize, TrashOperationResult};
    use crate::utils::format_trash_error;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use walkdir::WalkDir;

    pub const IS_LISTABLE: bool = true;

    /// Where the item's bytes actually sit, worked out from its record.
    ///
    /// The FreeDesktop layout puts the record at `<trash>/info/NAME.trashinfo` and the file
    /// itself at `<trash>/files/NAME`, and the crate hands out the record's path as the id — so
    /// one implies the other. This holds for trash directories on other mounts too, since both
    /// halves live under the same root.
    ///
    /// Windows identifies items by a shell display name rather than a path, so there is nothing
    /// to derive there and the caller falls back to what the API reports.
    #[cfg(unix)]
    fn trashed_file_path(item: &trash::TrashItem) -> Option<PathBuf> {
        let info_path = Path::new(&item.id);
        let info_directory = info_path.parent()?;

        if info_directory.file_name()? != "info" {
            return None;
        }

        // `file_stem` drops only the final extension, so `photo.png.trashinfo` gives `photo.png`.
        let file_name = info_path.file_stem()?;
        let trashed = info_directory.parent()?.join("files").join(file_name);

        trashed.symlink_metadata().is_ok().then_some(trashed)
    }

    #[cfg(not(unix))]
    fn trashed_file_path(_item: &trash::TrashItem) -> Option<PathBuf> {
        None
    }

    /// Bytes under `path`, without following symbolic links.
    ///
    /// Not following them is the whole point: a trashed symlink pointing at a home directory
    /// would otherwise have that directory counted as its size, and one pointing at `/` would
    /// walk the entire filesystem. A link contributes only itself. Unreadable entries are
    /// skipped rather than failing the measurement — a size that is slightly short is more use
    /// than no size at all.
    fn size_on_disk(path: &Path) -> u64 {
        match path.symlink_metadata() {
            Ok(metadata) if metadata.is_dir() => WalkDir::new(path)
                .follow_links(false)
                .into_iter()
                .filter_map(Result::ok)
                .filter_map(|entry| entry.metadata().ok())
                .filter(|metadata| metadata.is_file())
                .map(|metadata| metadata.len())
                .sum(),
            Ok(metadata) => metadata.len(),
            Err(_) => 0,
        }
    }

    /// Falls back to what the trash API knows when the file cannot be located: a byte count for
    /// a file, and nothing usable for a folder, which is the gap this whole function exists for.
    fn reported_size(item: &trash::TrashItem) -> u64 {
        match trash::os_limited::metadata(item) {
            Ok(metadata) => metadata.size.size().unwrap_or(0),
            Err(_) => 0,
        }
    }

    pub async fn sizes() -> Result<Vec<TrashEntrySize>, String> {
        tauri::async_runtime::spawn_blocking(|| {
            let items = read_trash_items()?;

            Ok(items
                .iter()
                .map(|item| TrashEntrySize {
                    id: item.id.to_string_lossy().into_owned(),
                    size: match trashed_file_path(item) {
                        Some(path) => size_on_disk(&path),
                        None => reported_size(item),
                    },
                })
                .collect())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    fn read_trash_items() -> Result<Vec<trash::TrashItem>, String> {
        trash::os_limited::list().map_err(format_trash_error)
    }

    fn entry_from_item(item: &trash::TrashItem) -> TrashEntry {
        // Metadata is a second look at the filesystem per item, and a trash left in an odd state
        // should still list. An item whose size cannot be read reports zero rather than vanishing.
        let (size, item_count, is_dir) = match trash::os_limited::metadata(item) {
            Ok(metadata) => match metadata.size {
                trash::TrashItemSize::Bytes(bytes) => (bytes, None, false),
                trash::TrashItemSize::Entries(entries) => (0, Some(entries as u64), true),
            },
            Err(_) => (0, None, false),
        };

        TrashEntry {
            id: item.id.to_string_lossy().into_owned(),
            name: item.name.to_string_lossy().into_owned(),
            original_path: item.original_path().to_string_lossy().into_owned(),
            original_parent: item.original_parent.to_string_lossy().into_owned(),
            // The crate reports whole seconds; the app measures timestamps in milliseconds.
            deleted_time: item.time_deleted.saturating_mul(1000),
            size,
            is_dir,
            item_count,
        }
    }

    /// Splits a fresh listing into the items named by `ids`, and how many were not found.
    fn take_items_by_id(
        ids: &[String],
        items: Vec<trash::TrashItem>,
    ) -> (Vec<trash::TrashItem>, u32) {
        let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
        let selected: Vec<trash::TrashItem> = items
            .into_iter()
            .filter(|item| wanted.contains(item.id.to_string_lossy().as_ref()))
            .collect();

        let missing = ids.len().saturating_sub(selected.len()) as u32;
        (selected, missing)
    }

    fn missing_error(missing: u32) -> Option<String> {
        (missing > 0).then(|| "Item is no longer in the trash.".to_string())
    }

    /// Everything currently in the trash, newest deletion first.
    pub async fn list() -> Result<Vec<TrashEntry>, String> {
        tauri::async_runtime::spawn_blocking(|| {
            let mut items = read_trash_items()?;

            // The crate returns them in no order at all. Most recent first is the order that
            // matters here: what someone wants back is usually what they just deleted.
            items.sort_by_key(|item| std::cmp::Reverse(item.time_deleted));

            Ok(items.iter().map(entry_from_item).collect())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    /// Puts items back where they were deleted from.
    ///
    /// Restored one at a time on purpose. The crate's `restore_all` abandons the whole batch if
    /// any item collides with a file that now occupies its original path, which for a multiple
    /// selection would mean restoring nothing because of one name — so each item is given its
    /// own chance and the failures are counted.
    pub async fn restore(ids: Vec<String>) -> Result<TrashOperationResult, String> {
        tauri::async_runtime::spawn_blocking(move || {
            let (selected, missing) = take_items_by_id(&ids, read_trash_items()?);

            let mut restored_count: u32 = 0;
            let mut failed_count = missing;
            let mut last_error = missing_error(missing);

            for item in selected {
                match trash::os_limited::restore_all([item]) {
                    Ok(()) => restored_count += 1,
                    Err(error) => {
                        failed_count += 1;
                        last_error = Some(format_trash_error(error));
                    }
                }
            }

            Ok(TrashOperationResult::from_counts(
                restored_count,
                failed_count,
                last_error,
            ))
        })
        .await
        .map_err(|error| error.to_string())?
    }

    /// Deletes items from the trash for good.
    pub async fn purge(ids: Vec<String>) -> Result<TrashOperationResult, String> {
        tauri::async_runtime::spawn_blocking(move || {
            let (selected, missing) = take_items_by_id(&ids, read_trash_items()?);
            let selected_count = selected.len() as u32;

            match trash::os_limited::purge_all(selected) {
                Ok(()) => Ok(TrashOperationResult::from_counts(
                    selected_count,
                    missing,
                    missing_error(missing),
                )),
                Err(error) => Ok(TrashOperationResult::from_counts(
                    0,
                    selected_count + missing,
                    Some(format_trash_error(error)),
                )),
            }
        })
        .await
        .map_err(|error| error.to_string())?
    }

    #[cfg(test)]
    mod tests {
        use super::{size_on_disk, take_items_by_id};
        use std::path::PathBuf;

        #[test]
        fn measures_a_folder_by_everything_under_it() {
            let root = tempfile::tempdir().expect("temp dir");
            let nested = root.path().join("nested");
            std::fs::create_dir(&nested).expect("create nested");
            std::fs::write(root.path().join("a.bin"), vec![0u8; 100]).expect("write a");
            std::fs::write(nested.join("b.bin"), vec![0u8; 250]).expect("write b");

            assert_eq!(size_on_disk(root.path()), 350);
            assert_eq!(size_on_disk(&root.path().join("a.bin")), 100);
        }

        /// A trashed symlink must count as itself. Following it would charge a deleted shortcut
        /// for the size of whatever it points at, and one pointing at `/` would walk the disk.
        #[cfg(unix)]
        #[test]
        fn does_not_follow_symbolic_links_out_of_the_trash() {
            let root = tempfile::tempdir().expect("temp dir");
            let outside = tempfile::tempdir().expect("temp dir");
            std::fs::write(outside.path().join("big.bin"), vec![0u8; 10_000]).expect("write big");

            let link = root.path().join("link");
            std::os::unix::fs::symlink(outside.path(), &link).expect("symlink");

            assert!(size_on_disk(root.path()) < 10_000);
            assert!(size_on_disk(&link) < 10_000);
        }

        fn item(id: &str) -> trash::TrashItem {
            trash::TrashItem {
                id: id.into(),
                name: "file.txt".into(),
                original_parent: PathBuf::from("/home/user"),
                time_deleted: 0,
            }
        }

        /// The ids come back from the frontend as strings and are matched against a listing read
        /// fresh, so a mismatch here would restore nothing while reporting nothing wrong.
        #[test]
        fn selects_the_named_items() {
            let (selected, missing) = take_items_by_id(
                &["b".to_string(), "c".to_string()],
                vec![item("a"), item("b"), item("c")],
            );

            assert_eq!(
                selected
                    .iter()
                    .map(|item| item.id.to_string_lossy().into_owned())
                    .collect::<Vec<_>>(),
                vec!["b", "c"],
            );
            assert_eq!(missing, 0);
        }

        /// An item emptied from the trash by something else between listing and acting.
        #[test]
        fn counts_ids_that_are_no_longer_there() {
            let (selected, missing) =
                take_items_by_id(&["a".to_string(), "gone".to_string()], vec![item("a")]);

            assert_eq!(selected.len(), 1);
            assert_eq!(missing, 1);
        }
    }

    /// Empties the trash.
    ///
    /// Reads the listing itself rather than taking ids, so that emptying means what it says even
    /// if something was deleted after the window last refreshed.
    pub async fn empty() -> Result<TrashOperationResult, String> {
        tauri::async_runtime::spawn_blocking(|| {
            let items = read_trash_items()?;
            let count = items.len() as u32;

            match trash::os_limited::purge_all(items) {
                Ok(()) => Ok(TrashOperationResult::from_counts(count, 0, None)),
                Err(error) => Ok(TrashOperationResult::from_counts(
                    0,
                    count,
                    Some(format_trash_error(error)),
                )),
            }
        })
        .await
        .map_err(|error| error.to_string())?
    }
}

#[cfg(not(any(
    windows,
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
)))]
mod platform {
    use super::{TrashEntry, TrashEntrySize, TrashOperationResult};

    pub const IS_LISTABLE: bool = false;

    const UNSUPPORTED: &str = "This platform does not support reading the trash.";

    pub async fn list() -> Result<Vec<TrashEntry>, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub async fn sizes() -> Result<Vec<TrashEntrySize>, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub async fn restore(_ids: Vec<String>) -> Result<TrashOperationResult, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub async fn purge(_ids: Vec<String>) -> Result<TrashOperationResult, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub async fn empty() -> Result<TrashOperationResult, String> {
        Err(UNSUPPORTED.to_string())
    }
}
