// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use crate::guarded_walk::{GuardedWalk, WalkEntry};
use crate::utils::normalize_path;
use lru::LruCache;
use once_cell::sync::Lazy;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CACHE_SIZE: usize = 2000;
const CACHE_TTL_SECONDS: u64 = 300;
/// How many walked entries are held at once while their metadata is read. Collecting the
/// whole walk first meant a single size calculation kept every `DirEntry` — each owning a
/// full path — resident until the walk finished, which costs hundreds of megabytes on a
/// large tree and, because several calculations run in parallel, does so several times
/// over. Reading metadata in batches keeps the parallelism that makes the walk fast while
/// bounding what is resident to one batch per calculation.
const SIZE_WALK_BATCH: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SizeStatus {
    Complete,
    Partial,
    Timeout,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirSizeResult {
    pub path: String,
    pub size: u64,
    pub status: SizeStatus,
    pub file_count: u64,
    pub dir_count: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    size: u64,
    file_count: u64,
    dir_count: u64,
    status: SizeStatus,
    calculated_at: u64,
    dir_mtime: u64,
}

static SIZE_CACHE: Lazy<Mutex<LruCache<String, CacheEntry>>> =
    Lazy::new(|| Mutex::new(LruCache::new(NonZeroUsize::new(CACHE_SIZE).unwrap())));

// Map of path -> cancellation token for active calculations
static ACTIVE_CALCULATIONS: Lazy<Mutex<HashMap<String, Arc<AtomicBool>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// Store for current progress of active calculations
#[derive(Debug, Clone)]
struct CalculationProgress {
    size: Arc<AtomicU64>,
    file_count: Arc<AtomicU64>,
    dir_count: Arc<AtomicU64>,
}

static CALCULATION_PROGRESS: Lazy<Mutex<HashMap<String, CalculationProgress>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn register_calculation(path: &str) -> (Arc<AtomicBool>, CalculationProgress) {
    let normalized = normalize_path(path);
    let cancel_token = Arc::new(AtomicBool::new(false));
    let progress = CalculationProgress {
        size: Arc::new(AtomicU64::new(0)),
        file_count: Arc::new(AtomicU64::new(0)),
        dir_count: Arc::new(AtomicU64::new(0)),
    };

    if let Ok(mut active) = ACTIVE_CALCULATIONS.lock() {
        active.insert(normalized.clone(), cancel_token.clone());
    }
    if let Ok(mut prog) = CALCULATION_PROGRESS.lock() {
        prog.insert(normalized, progress.clone());
    }

    (cancel_token, progress)
}

fn unregister_calculation(path: &str) {
    let normalized = normalize_path(path);
    if let Ok(mut active) = ACTIVE_CALCULATIONS.lock() {
        active.remove(&normalized);
    }
    if let Ok(mut prog) = CALCULATION_PROGRESS.lock() {
        prog.remove(&normalized);
    }
}

fn get_dir_mtime(path: &Path) -> u64 {
    path.metadata()
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn get_current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn get_cached_size(path: &str) -> Option<CacheEntry> {
    let normalized = normalize_path(path);
    let mut cache = SIZE_CACHE.lock().ok()?;
    let entry = cache.get(&normalized)?;

    let now = get_current_timestamp();
    if now - entry.calculated_at > CACHE_TTL_SECONDS {
        return None;
    }

    let current_mtime = get_dir_mtime(Path::new(path));
    if current_mtime > entry.dir_mtime {
        return None;
    }

    Some(entry.clone())
}

fn set_cached_size(path: &str, entry: CacheEntry) {
    let normalized = normalize_path(path);
    if let Ok(mut cache) = SIZE_CACHE.lock() {
        cache.put(normalized, entry);
    }
}

/// A walk must not enter the mount point of a remote filesystem that is not answering —
/// even opening it blocks until the transport gives up — nor an automount trigger, where
/// the open itself would mount something nobody asked to size.
fn admits_answering_storage(path: &Path, _depth: usize, is_dir: bool) -> bool {
    !is_dir || !crate::dir_reader::mount_health::must_not_enter_mount_point(path)
}

/// What one walked entry adds to the running totals. Directories count by their type alone;
/// only a regular file needs its metadata, for the size.
fn tally(entry: &WalkEntry, total_size: &AtomicU64, file_count: &AtomicU64, dir_count: &AtomicU64) {
    if entry.file_type.is_dir() {
        dir_count.fetch_add(1, Ordering::Relaxed);
    } else if entry.file_type.is_file() {
        if let Ok(metadata) = std::fs::symlink_metadata(&entry.path) {
            total_size.fetch_add(metadata.len(), Ordering::Relaxed);
            file_count.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn unresponsive_storage_result(path_str: String, error: String) -> DirSizeResult {
    DirSizeResult {
        path: path_str,
        size: 0,
        status: SizeStatus::Error,
        file_count: 0,
        dir_count: 0,
        error: Some(error),
    }
}

fn calculate_dir_size_with_timeout(path: &Path, timeout: Duration) -> DirSizeResult {
    let path_str = normalize_path(&path.to_string_lossy());

    if let Err(error) = crate::dir_reader::mount_health::ensure_responsive(path) {
        return unresponsive_storage_result(path_str, error);
    }

    if !path.exists() {
        return DirSizeResult {
            path: path_str,
            size: 0,
            status: SizeStatus::Error,
            file_count: 0,
            dir_count: 0,
            error: Some("Path does not exist".to_string()),
        };
    }

    if !path.is_dir() {
        return DirSizeResult {
            path: path_str,
            size: 0,
            status: SizeStatus::Error,
            file_count: 0,
            dir_count: 0,
            error: Some("Path is not a directory".to_string()),
        };
    }

    let start_time = Instant::now();
    let mut was_cancelled = false;
    let total_size = AtomicU64::new(0);
    let file_count = AtomicU64::new(0);
    let dir_count = AtomicU64::new(0);

    let mut walker = GuardedWalk::new(path, usize::MAX, admits_answering_storage);

    let mut batch: Vec<WalkEntry> = Vec::with_capacity(SIZE_WALK_BATCH);

    loop {
        batch.clear();
        let mut walk_finished = false;

        while batch.len() < SIZE_WALK_BATCH {
            // The clock is read before pulling, but only counts as running out of time if
            // there was in fact another entry to take — the same rule the previous
            // `take_while` followed. A walk that ends just as the budget does has still
            // covered everything, and calling that partial would discard a complete answer
            // and recalculate it on the next look.
            let timed_out = start_time.elapsed() > timeout;

            match walker.next() {
                Some(entry) => {
                    if timed_out {
                        was_cancelled = true;
                        break;
                    }
                    batch.push(entry);
                }
                None => {
                    walk_finished = true;
                    break;
                }
            }
        }

        if !batch.is_empty() {
            batch
                .par_iter()
                .for_each(|entry| tally(entry, &total_size, &file_count, &dir_count));
        }

        if was_cancelled || walk_finished {
            break;
        }
    }

    let final_size = total_size.load(Ordering::SeqCst);
    let final_file_count = file_count.load(Ordering::SeqCst);
    let final_dir_count = dir_count.load(Ordering::SeqCst);

    let status = if was_cancelled {
        SizeStatus::Partial
    } else {
        SizeStatus::Complete
    };

    // Only cache complete results - partial sizes are not stored
    if !was_cancelled {
        let dir_mtime = get_dir_mtime(path);
        set_cached_size(
            &path_str,
            CacheEntry {
                size: final_size,
                file_count: final_file_count,
                dir_count: final_dir_count,
                status: status.clone(),
                calculated_at: get_current_timestamp(),
                dir_mtime,
            },
        );
    }

    DirSizeResult {
        path: path_str,
        size: final_size,
        status,
        file_count: final_file_count,
        dir_count: final_dir_count,
        error: None,
    }
}

fn calculate_dir_size_no_timeout(
    path: &Path,
    cancel_token: Arc<AtomicBool>,
    progress: CalculationProgress,
) -> DirSizeResult {
    let path_str = normalize_path(&path.to_string_lossy());

    if let Err(error) = crate::dir_reader::mount_health::ensure_responsive(path) {
        return unresponsive_storage_result(path_str, error);
    }

    if !path.exists() {
        return DirSizeResult {
            path: path_str,
            size: 0,
            status: SizeStatus::Error,
            file_count: 0,
            dir_count: 0,
            error: Some("Path does not exist".to_string()),
        };
    }

    if !path.is_dir() {
        return DirSizeResult {
            path: path_str,
            size: 0,
            status: SizeStatus::Error,
            file_count: 0,
            dir_count: 0,
            error: Some("Path is not a directory".to_string()),
        };
    }

    let was_cancelled = Arc::new(AtomicBool::new(false));
    let was_cancelled_clone = was_cancelled.clone();
    let cancel_token_clone = cancel_token.clone();

    // Use the shared progress counters
    let total_size = progress.size.clone();
    let file_count = progress.file_count.clone();
    let dir_count = progress.dir_count.clone();

    let total_size_clone = total_size.clone();
    let file_count_clone = file_count.clone();
    let dir_count_clone = dir_count.clone();

    // Process entries one by one, updating progress as we go
    for entry in GuardedWalk::new(path, usize::MAX, admits_answering_storage) {
        // Check cancellation
        if cancel_token_clone.load(Ordering::SeqCst) {
            was_cancelled_clone.store(true, Ordering::SeqCst);
            break;
        }

        tally(
            &entry,
            &total_size_clone,
            &file_count_clone,
            &dir_count_clone,
        );
    }

    // Check if cancelled
    if cancel_token.load(Ordering::SeqCst) || was_cancelled.load(Ordering::SeqCst) {
        return DirSizeResult {
            path: path_str,
            size: total_size.load(Ordering::SeqCst),
            status: SizeStatus::Cancelled,
            file_count: file_count.load(Ordering::SeqCst),
            dir_count: dir_count.load(Ordering::SeqCst),
            error: None,
        };
    }

    let final_size = total_size.load(Ordering::SeqCst);
    let final_file_count = file_count.load(Ordering::SeqCst);
    let final_dir_count = dir_count.load(Ordering::SeqCst);

    let dir_mtime = get_dir_mtime(path);
    set_cached_size(
        &path_str,
        CacheEntry {
            size: final_size,
            file_count: final_file_count,
            dir_count: final_dir_count,
            status: SizeStatus::Complete,
            calculated_at: get_current_timestamp(),
            dir_mtime,
        },
    );

    DirSizeResult {
        path: path_str,
        size: final_size,
        status: SizeStatus::Complete,
        file_count: final_file_count,
        dir_count: final_dir_count,
        error: None,
    }
}

#[tauri::command]
pub async fn get_dir_size(path: String, timeout_ms: Option<u64>) -> DirSizeResult {
    let path_clone = path.clone();
    let (cancel_token, progress) = register_calculation(&path);

    let result = tokio::task::spawn_blocking(move || {
        let dir_path = Path::new(&path_clone);

        match timeout_ms {
            Some(ms) => calculate_dir_size_with_timeout(dir_path, Duration::from_millis(ms)),
            None => calculate_dir_size_no_timeout(dir_path, cancel_token, progress),
        }
    })
    .await
    .unwrap_or_else(|_| DirSizeResult {
        path: normalize_path(&path),
        size: 0,
        status: SizeStatus::Error,
        file_count: 0,
        dir_count: 0,
        error: Some("Task failed".to_string()),
    });

    unregister_calculation(&path);
    result
}

/// Get the current progress of an active calculation
#[tauri::command]
pub fn get_dir_size_progress(path: String) -> Option<DirSizeResult> {
    let normalized = normalize_path(&path);

    if let Ok(prog) = CALCULATION_PROGRESS.lock() {
        if let Some(progress) = prog.get(&normalized) {
            return Some(DirSizeResult {
                path: normalized,
                size: progress.size.load(Ordering::SeqCst),
                status: SizeStatus::Partial, // In progress, so partial
                file_count: progress.file_count.load(Ordering::SeqCst),
                dir_count: progress.dir_count.load(Ordering::SeqCst),
                error: None,
            });
        }
    }

    None
}

/// Get all active calculations (for frontend recovery after reload)
#[tauri::command]
pub fn get_active_calculations() -> Vec<DirSizeResult> {
    let mut results = Vec::new();

    if let Ok(prog) = CALCULATION_PROGRESS.lock() {
        for (path, progress) in prog.iter() {
            results.push(DirSizeResult {
                path: path.clone(),
                size: progress.size.load(Ordering::SeqCst),
                status: SizeStatus::Partial,
                file_count: progress.file_count.load(Ordering::SeqCst),
                dir_count: progress.dir_count.load(Ordering::SeqCst),
                error: None,
            });
        }
    }

    results
}

#[tauri::command]
pub fn cancel_dir_size(path: String) -> bool {
    let normalized = normalize_path(&path);
    if let Ok(active) = ACTIVE_CALCULATIONS.lock() {
        if let Some(cancel_token) = active.get(&normalized) {
            cancel_token.store(true, Ordering::SeqCst);
            return true;
        }
    }
    false
}

#[tauri::command]
pub async fn get_dir_sizes_batch(
    paths: Vec<String>,
    timeout_ms: Option<u64>,
    use_cache: Option<bool>,
) -> Vec<DirSizeResult> {
    tokio::task::spawn_blocking(move || {
        let timeout = match timeout_ms {
            None => Duration::from_secs(60 * 60 * 24 * 365),
            Some(ms) => Duration::from_millis(ms),
        };
        let should_use_cache = use_cache.unwrap_or(true);

        paths
            .par_iter()
            .map(|path| {
                if should_use_cache {
                    if let Some(cached) = get_cached_size(path) {
                        return DirSizeResult {
                            path: normalize_path(path),
                            size: cached.size,
                            status: cached.status,
                            file_count: cached.file_count,
                            dir_count: cached.dir_count,
                            error: None,
                        };
                    }
                }

                calculate_dir_size_with_timeout(Path::new(path), timeout)
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub fn invalidate_dir_size_cache(paths: Vec<String>) {
    if let Ok(mut cache) = SIZE_CACHE.lock() {
        for path in paths {
            let normalized = normalize_path(&path);
            cache.pop(&normalized);

            let path_with_slash = if normalized.ends_with('/') {
                normalized.clone()
            } else {
                format!("{}/", normalized)
            };

            let keys_to_remove: Vec<String> = cache
                .iter()
                .filter(|(key, _)| key.starts_with(&path_with_slash))
                .map(|(key, _)| key.clone())
                .collect();

            for key in keys_to_remove {
                cache.pop(&key);
            }
        }
    }
}

#[tauri::command]
pub fn clear_dir_size_cache() {
    if let Ok(mut cache) = SIZE_CACHE.lock() {
        cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Writes `count` files of `size_each` bytes and returns what their total should be.
    fn fill(dir: &Path, count: usize, size_each: usize) -> u64 {
        for index in 0..count {
            fs::write(dir.join(format!("file-{index}")), vec![b'x'; size_each]).unwrap();
        }
        (count as u64) * (size_each as u64)
    }

    fn generous_timeout() -> Duration {
        Duration::from_secs(60)
    }

    #[test]
    fn totals_a_tree_that_spans_several_batches() {
        let temp = TempDir::new().unwrap();
        let expected = fill(temp.path(), SIZE_WALK_BATCH * 2 + 7, 3);

        let result = calculate_dir_size_with_timeout(temp.path(), generous_timeout());

        assert_eq!(result.status, SizeStatus::Complete);
        assert_eq!(result.size, expected);
        assert_eq!(result.file_count as usize, SIZE_WALK_BATCH * 2 + 7);
        assert_eq!(result.dir_count, 0);
    }

    /// A tree whose entry count is an exact multiple of the batch size leaves the walker
    /// empty at a batch boundary. The loop has to notice that and stop rather than spin.
    #[test]
    fn terminates_when_the_walk_ends_on_a_batch_boundary() {
        let temp = TempDir::new().unwrap();
        let expected = fill(temp.path(), SIZE_WALK_BATCH, 1);

        let result = calculate_dir_size_with_timeout(temp.path(), generous_timeout());

        assert_eq!(result.status, SizeStatus::Complete);
        assert_eq!(result.size, expected);
        assert_eq!(result.file_count as usize, SIZE_WALK_BATCH);
    }

    #[test]
    fn counts_nested_directories_and_their_files() {
        let temp = TempDir::new().unwrap();
        let nested = temp.path().join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        let expected = fill(&nested, 4, 10) + fill(temp.path(), 2, 5);

        let result = calculate_dir_size_with_timeout(temp.path(), generous_timeout());

        assert_eq!(result.status, SizeStatus::Complete);
        assert_eq!(result.size, expected);
        assert_eq!(result.file_count, 6);
        assert_eq!(result.dir_count, 2);
    }

    /// An expired budget reports what was reached so far as partial, and a partial answer
    /// must never reach the cache — a wrong size that sticks is worse than none.
    #[test]
    fn an_expired_timeout_reports_partial_and_is_not_cached() {
        let temp = TempDir::new().unwrap();
        fill(temp.path(), SIZE_WALK_BATCH + 1, 1);

        let result = calculate_dir_size_with_timeout(temp.path(), Duration::from_secs(0));

        assert_eq!(result.status, SizeStatus::Partial);
        assert!(get_cached_size(&result.path).is_none());
    }

    #[test]
    fn an_empty_directory_totals_zero() {
        let temp = TempDir::new().unwrap();

        let result = calculate_dir_size_with_timeout(temp.path(), generous_timeout());

        assert_eq!(result.status, SizeStatus::Complete);
        assert_eq!(result.size, 0);
        assert_eq!(result.file_count, 0);
        assert_eq!(result.dir_count, 0);
    }
}
