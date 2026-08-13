// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use once_cell::sync::Lazy;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tantivy::{Index, IndexReader, Term};

use super::index::{
    calculate_dir_size, create_maintenance_index_writer, index_dir, open_or_create_index,
};
use super::scan::{apply_committed_index_status, CommittedIndexUpdate};
use super::state::{index_generation, now_millis, GlobalSearchIndexFields, GLOBAL_SEARCH_STATE};

/// A commit costs far more than the deletions it carries, so dead paths are batched:
/// a purge runs once enough of them have piled up, or once the batch has waited long
/// enough that flushing a small one is still worth it.
const PURGE_BATCH_THRESHOLD: usize = 32;
const PURGE_MIN_INTERVAL_MS: u64 = 30_000;
/// Paths beyond this are dropped rather than queued. A later query re-reports them.
const PURGE_QUEUE_CAPACITY: usize = 4096;

static PENDING_PURGE_PATHS: Lazy<Mutex<BTreeSet<String>>> =
    Lazy::new(|| Mutex::new(BTreeSet::new()));
static IS_PURGE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static LAST_PURGE_TIME: AtomicU64 = AtomicU64::new(0);

/// Records index entries whose path no longer exists and, once the batch is worth a
/// commit, removes them from the index in the background so it stops carrying them.
pub(super) fn report_missing_paths(base_dir: &Path, missing_paths: Vec<String>) {
    if missing_paths.is_empty() {
        return;
    }

    let queue_length = enqueue_paths(missing_paths);

    if !should_drain_queue(
        queue_length,
        LAST_PURGE_TIME.load(Ordering::SeqCst),
        now_millis(),
    ) {
        return;
    }

    // A full scan builds a replacement index from scratch, which drops these entries
    // anyway, and its directory swap would invalidate whatever we committed here.
    if is_index_busy() {
        return;
    }

    if IS_PURGE_IN_FLIGHT.swap(true, Ordering::SeqCst) {
        return;
    }

    let base_dir = base_dir.to_path_buf();

    tauri::async_runtime::spawn_blocking(move || {
        // Released even if the purge panics, so one failure cannot wedge the flag and
        // silently disable purging for the rest of the session.
        let _in_flight_guard = PurgeInFlightGuard;
        let _ = purge_pending_paths(&base_dir);
    });
}

struct PurgeInFlightGuard;

impl Drop for PurgeInFlightGuard {
    fn drop(&mut self) {
        LAST_PURGE_TIME.store(now_millis(), Ordering::SeqCst);
        IS_PURGE_IN_FLIGHT.store(false, Ordering::SeqCst);
    }
}

fn is_index_busy() -> bool {
    GLOBAL_SEARCH_STATE
        .read()
        .map(|state| state.status.is_scan_in_progress || state.status.is_committing)
        .unwrap_or(true)
}

fn enqueue_paths(paths: Vec<String>) -> usize {
    let mut queue = match PENDING_PURGE_PATHS.lock() {
        Ok(queue) => queue,
        Err(_) => return 0,
    };

    for path in paths {
        if queue.len() >= PURGE_QUEUE_CAPACITY {
            break;
        }
        queue.insert(path);
    }

    queue.len()
}

fn should_drain_queue(queue_length: usize, last_purge_time: u64, now: u64) -> bool {
    if queue_length == 0 {
        return false;
    }

    queue_length >= PURGE_BATCH_THRESHOLD
        || now.saturating_sub(last_purge_time) >= PURGE_MIN_INTERVAL_MS
}

fn take_pending_paths() -> Vec<String> {
    PENDING_PURGE_PATHS
        .lock()
        .map(|mut queue| std::mem::take(&mut *queue).into_iter().collect())
        .unwrap_or_default()
}

fn purge_pending_paths(base_dir: &Path) -> Result<u64, String> {
    let paths = take_pending_paths();

    if paths.is_empty() {
        return Ok(0);
    }

    match purge_paths_from_index(base_dir, &paths) {
        Ok(removed_count) => Ok(removed_count),
        Err(error) => {
            // Keep them queued so a later query retries the removal.
            enqueue_paths(paths);
            Err(error)
        }
    }
}

fn purge_paths_from_index(base_dir: &Path, paths: &[String]) -> Result<u64, String> {
    let generation_before = index_generation();
    let index_path = index_dir(base_dir);

    let (index, reader, fields, doc_count) = delete_paths_from_index(&index_path, paths)?;
    let index_size = calculate_dir_size(&index_path);

    let mut state = GLOBAL_SEARCH_STATE
        .write()
        .map_err(|error| error.to_string())?;

    if index_generation() != generation_before {
        // A scan published a freshly built index while this purge ran. That index was
        // never given these paths, so there is nothing left to remove and publishing
        // our handles would replace it with ones pointing at the discarded directory.
        return Ok(0);
    }

    apply_committed_index_status(
        &mut state,
        base_dir,
        CommittedIndexUpdate {
            doc_count,
            index_size_bytes: index_size,
            index,
            reader,
            fields,
            indexed_drive_roots: None,
        },
        false,
    );

    Ok(paths.len() as u64)
}

fn delete_paths_from_index(
    index_path: &Path,
    paths: &[String],
) -> Result<(Index, IndexReader, GlobalSearchIndexFields, u64), String> {
    let (index, reader, fields) = open_or_create_index(index_path)?;
    let mut writer = create_maintenance_index_writer(&index)?;

    for path in paths {
        // `path` is an untokenized STRING field, so a document holds its whole path as
        // a single term and this match is exact rather than a prefix or token match.
        writer.delete_term(Term::from_field_text(fields.path, path));
    }

    writer.commit().map_err(|error| error.to_string())?;
    drop(writer);

    reader.reload().map_err(|error| error.to_string())?;
    let doc_count = reader.searcher().num_docs();

    Ok((index, reader, fields, doc_count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::global_search::index::create_fresh_index;
    use tantivy::doc;
    use tempfile::TempDir;

    fn write_docs(index_path: &Path, paths: &[&str]) {
        let (index, fields) = create_fresh_index(index_path).unwrap();
        let mut writer = index.writer(15_000_000).unwrap();

        for path in paths {
            writer
                .add_document(doc!(
                    fields.path => path.to_string(),
                    fields.name => path.to_string(),
                    fields.name_lower => path.to_lowercase(),
                    fields.is_file => 1u64,
                    fields.is_dir => 0u64,
                    fields.modified_time => 0u64,
                    fields.size => 0u64,
                ))
                .unwrap();
        }

        writer.commit().unwrap();
    }

    fn count_docs_with_path(
        reader: &IndexReader,
        fields: GlobalSearchIndexFields,
        path: &str,
    ) -> usize {
        use tantivy::collector::Count;
        use tantivy::query::TermQuery;
        use tantivy::schema::IndexRecordOption;

        let query = TermQuery::new(
            Term::from_field_text(fields.path, path),
            IndexRecordOption::Basic,
        );

        reader.searcher().search(&query, &Count).unwrap()
    }

    #[test]
    fn deleting_paths_removes_only_the_listed_documents() {
        let temp = TempDir::new().unwrap();
        let index_path = temp.path().join("index");

        write_docs(
            &index_path,
            &["/home/zero/gone", "/home/zero/kept", "/home/zero/gone-too"],
        );

        let removed = vec![
            "/home/zero/gone".to_string(),
            "/home/zero/gone-too".to_string(),
        ];
        let (_index, reader, fields, doc_count) =
            delete_paths_from_index(&index_path, &removed).unwrap();

        assert_eq!(doc_count, 1);
        assert_eq!(count_docs_with_path(&reader, fields, "/home/zero/kept"), 1);
        assert_eq!(count_docs_with_path(&reader, fields, "/home/zero/gone"), 0);
        assert_eq!(
            count_docs_with_path(&reader, fields, "/home/zero/gone-too"),
            0
        );
    }

    #[test]
    fn deleting_a_path_is_exact_and_spares_its_children() {
        let temp = TempDir::new().unwrap();
        let index_path = temp.path().join("index");

        write_docs(&index_path, &["/home/zero/dir", "/home/zero/dir/child"]);

        let removed = vec!["/home/zero/dir".to_string()];
        let (_index, _reader, _fields, doc_count) =
            delete_paths_from_index(&index_path, &removed).unwrap();

        assert_eq!(doc_count, 1);
    }

    #[test]
    fn empty_queue_never_drains() {
        assert!(!should_drain_queue(0, 0, PURGE_MIN_INTERVAL_MS * 10));
    }

    #[test]
    fn full_batch_drains_before_the_interval_elapses() {
        assert!(should_drain_queue(PURGE_BATCH_THRESHOLD, 1_000, 1_001));
    }

    #[test]
    fn small_batch_waits_for_the_interval() {
        assert!(!should_drain_queue(
            1,
            1_000,
            1_000 + PURGE_MIN_INTERVAL_MS - 1
        ));
        assert!(should_drain_queue(1, 1_000, 1_000 + PURGE_MIN_INTERVAL_MS));
    }

    #[test]
    fn queue_deduplicates_and_stops_at_capacity() {
        let _ = take_pending_paths();

        assert_eq!(enqueue_paths(vec!["/a".to_string(), "/a".to_string()]), 1);

        let overflow: Vec<String> = (0..PURGE_QUEUE_CAPACITY + 100)
            .map(|index| format!("/overflow/{index}"))
            .collect();

        assert_eq!(enqueue_paths(overflow), PURGE_QUEUE_CAPACITY);

        let drained = take_pending_paths();
        assert_eq!(drained.len(), PURGE_QUEUE_CAPACITY);
    }
}
