// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use super::blocking_timeout::with_blocking_timeout;
use super::mount_health;
use crate::utils::normalize_path;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;

const MAX_IN_FLIGHT_PATH_EXISTS_CHECKS: usize = 4;

fn path_exists_semaphore() -> Arc<Semaphore> {
    static SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SEMAPHORE
        .get_or_init(|| Arc::new(Semaphore::new(MAX_IN_FLIGHT_PATH_EXISTS_CHECKS)))
        .clone()
}

pub fn get_parent_dir(path: String) -> Option<String> {
    Path::new(&path)
        .parent()
        .and_then(|parent| parent.to_str())
        .map(normalize_path)
}

/// The mount point of a remote filesystem exists by definition and is answered from the
/// kernel's cache; anything below one that has stopped answering reads as absent, which is
/// what the caller would see once the transport gave up — only without the wait.
pub fn path_exists(path: String) -> bool {
    let path = Path::new(&path);

    if mount_health::mount_point_attributes(path).is_some() {
        return true;
    }
    if mount_health::is_unresponsive_path(path) {
        return false;
    }

    path.exists()
}

pub fn path_is_regular_file(path: String) -> bool {
    let path = Path::new(&path);

    if mount_health::mount_point_attributes(path).is_some()
        || mount_health::is_unresponsive_path(path)
    {
        return false;
    }

    path.symlink_metadata()
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

pub fn paths_are_directories(paths: Vec<String>) -> Vec<bool> {
    paths
        .into_iter()
        .map(|path| {
            let path = Path::new(&path);

            if let Some(attributes) = mount_health::mount_point_attributes(path) {
                return attributes.is_dir;
            }
            if mount_health::is_unresponsive_path(path) {
                return false;
            }

            path.symlink_metadata()
                .map(|metadata| metadata.is_dir())
                .unwrap_or(false)
        })
        .collect()
}

/// `None` is "could not tell": the check timed out, or the path is on a remote mount that
/// is not answering — a caller pruning stale entries must not mistake either for absence.
pub async fn path_exists_with_timeout(path: String, timeout_ms: u64) -> Option<bool> {
    let permit = path_exists_semaphore().acquire_owned().await.ok()?;

    with_blocking_timeout(timeout_ms, move || {
        let _permit = permit;
        let path = Path::new(&path);

        if mount_health::mount_point_attributes(path).is_some() {
            return Some(true);
        }
        if mount_health::is_unresponsive_path(path) {
            return None;
        }

        Some(path.exists())
    })
    .await
    .ok()
    .flatten()
}
