// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! Recursive name search under a single directory.
//!
//! The quick search filters the directory the user is looking at, which is a list the
//! frontend already holds. Searching subdirectories cannot work that way: the tree below a
//! directory is unbounded, so the walk lives here, matches names as it goes, and returns
//! only the hits.

use crate::search_pattern::{compile_search_pattern, compile_wildcard_search_pattern, looks_like_wildcard};
use crate::utils::is_hidden_path;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use walkdir::{DirEntry as WalkDirEntry, WalkDir};

use super::read::{read_entry, ReadEntryOptions};
use super::types::DirEntry;

/// Enough hits to fill any amount of scrolling, and few enough that the list stays
/// responsive once they reach the frontend. Reaching it reports the results as truncated
/// rather than pretending the search was exhaustive.
const DEFAULT_MAX_RESULTS: usize = 20_000;

/// A pattern that matches almost nothing still walks every directory below the root, so
/// the walk stops after this many entries even when the result cap is nowhere near.
const MAX_SCANNED_ENTRIES: u64 = 2_000_000;

/// Cancellation is checked on a stride: the lock is cheap, but not per-entry cheap.
const CANCELLATION_CHECK_STRIDE: u64 = 512;

/// Each pane runs at most one search, so a pane's newest request supersedes its own
/// previous one and leaves other panes alone.
static ACTIVE_SEARCHES: Lazy<Mutex<HashMap<String, u64>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static NEXT_SEARCH_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecursiveSearchOptions {
    /// Identifies the caller, not the request: a second search from the same pane cancels
    /// the first.
    pub search_key: String,
    /// Matched against entry names. An empty query returns everything under the root.
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub include_hidden: bool,
    #[serde(default)]
    pub max_results: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecursiveSearchResults {
    pub entries: Vec<DirEntry>,
    /// Set when the walk stopped early, so the frontend can say the list is partial
    /// instead of implying the tree holds nothing else.
    pub truncated: bool,
    /// Set when a newer search for the same key took over. The results are incomplete and
    /// the caller is expected to discard them.
    pub superseded: bool,
    pub scanned_count: u64,
}

enum NameMatcher {
    Everything,
    Substring(String),
    Pattern(Box<Regex>),
}

impl NameMatcher {
    fn build(query: &str, use_regex: bool) -> Result<Self, String> {
        if query.is_empty() {
            return Ok(Self::Everything);
        }

        if use_regex {
            return Ok(Self::Pattern(Box::new(compile_search_pattern(query)?)));
        }

        // `*` and `?` mean what they mean in a shell whether or not a pattern mode is on.
        if looks_like_wildcard(query) {
            return Ok(Self::Pattern(Box::new(compile_wildcard_search_pattern(
                query, false,
            )?)));
        }

        Ok(Self::Substring(query.to_lowercase()))
    }

    fn matches(&self, name: &str) -> bool {
        match self {
            Self::Everything => true,
            Self::Substring(needle) => name.to_lowercase().contains(needle),
            Self::Pattern(regex) => regex.is_match(name),
        }
    }
}

fn claim_search_id(search_key: &str) -> u64 {
    let search_id = NEXT_SEARCH_ID.fetch_add(1, Ordering::SeqCst);

    if let Ok(mut active) = ACTIVE_SEARCHES.lock() {
        active.insert(search_key.to_string(), search_id);
    }

    search_id
}

fn is_superseded(search_key: &str, search_id: u64) -> bool {
    match ACTIVE_SEARCHES.lock() {
        Ok(active) => active.get(search_key).copied() != Some(search_id),
        Err(_) => false,
    }
}

fn release_search_id(search_key: &str, search_id: u64) {
    if let Ok(mut active) = ACTIVE_SEARCHES.lock() {
        if active.get(search_key).copied() == Some(search_id) {
            active.remove(search_key);
        }
    }
}

/// Stops whatever that pane is currently searching. Used when the search is switched off
/// or the pane navigates away, so an abandoned walk does not keep spinning a disk.
pub fn cancel_recursive_search(search_key: &str) {
    if let Ok(mut active) = ACTIVE_SEARCHES.lock() {
        active.remove(search_key);
    }
}

fn walk_entry_is_hidden(entry: &WalkDirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }

    if entry
        .file_name()
        .to_str()
        .is_some_and(|name| name.starts_with('.'))
    {
        return true;
    }

    is_hidden_path(entry.path())
}

pub fn search_dir_recursive(
    path: String,
    options: RecursiveSearchOptions,
) -> Result<RecursiveSearchResults, String> {
    let root = Path::new(&path);

    if !root.is_dir() {
        return Err(format!("Path is not a directory: {path}"));
    }

    let matcher = NameMatcher::build(options.query.trim(), options.regex)?;
    let max_results = options.max_results.unwrap_or(DEFAULT_MAX_RESULTS);
    let search_id = claim_search_id(&options.search_key);
    // Item counts and link targets cost a syscall each; the list fills them in lazily for
    // the rows that end up on screen, exactly as it does for a plain directory listing.
    let read_options = ReadEntryOptions::default();

    let mut entries: Vec<DirEntry> = Vec::new();
    let mut scanned_count: u64 = 0;
    let mut truncated = false;
    let mut superseded = false;

    // Symlinked directories are not followed: a link back up the tree would otherwise turn
    // the walk into a loop, and the same file would be reported under several paths.
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| options.include_hidden || !walk_entry_is_hidden(entry));

    for walk_result in walker {
        scanned_count += 1;

        if scanned_count % CANCELLATION_CHECK_STRIDE == 0
            && is_superseded(&options.search_key, search_id)
        {
            superseded = true;
            break;
        }

        if scanned_count > MAX_SCANNED_ENTRIES {
            truncated = true;
            break;
        }

        let Ok(walk_entry) = walk_result else {
            continue;
        };

        if walk_entry.depth() == 0 {
            continue;
        }

        let Some(name) = walk_entry.file_name().to_str() else {
            continue;
        };

        if !matcher.matches(name) {
            continue;
        }

        let Some(entry) = read_entry(walk_entry.path(), read_options) else {
            continue;
        };

        if !options.include_hidden && entry.is_hidden {
            continue;
        }

        entries.push(entry);

        if entries.len() >= max_results {
            truncated = true;
            break;
        }
    }

    if !superseded && is_superseded(&options.search_key, search_id) {
        superseded = true;
    }

    release_search_id(&options.search_key, search_id);

    Ok(RecursiveSearchResults {
        entries,
        truncated,
        superseded,
        scanned_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_tree() -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("create temp dir");
        let nested = temp.path().join("nested").join("deeper");
        fs::create_dir_all(&nested).expect("create nested dirs");
        fs::create_dir_all(temp.path().join(".hidden-dir")).expect("create hidden dir");

        fs::write(temp.path().join("root-report.txt"), b"").expect("write file");
        fs::write(nested.join("nested-report.md"), b"").expect("write file");
        fs::write(nested.join("unrelated.bin"), b"").expect("write file");
        fs::write(temp.path().join(".hidden-report.txt"), b"").expect("write file");
        fs::write(temp.path().join(".hidden-dir").join("report.txt"), b"").expect("write file");

        temp
    }

    fn search(
        temp: &tempfile::TempDir,
        query: &str,
        regex: bool,
        include_hidden: bool,
    ) -> RecursiveSearchResults {
        search_dir_recursive(
            temp.path().to_string_lossy().to_string(),
            RecursiveSearchOptions {
                search_key: format!("test:{query}:{regex}:{include_hidden}"),
                query: query.to_string(),
                regex,
                include_hidden,
                max_results: None,
            },
        )
        .expect("run search")
    }

    fn names(results: &RecursiveSearchResults) -> Vec<String> {
        let mut names: Vec<String> = results
            .entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn substring_search_reaches_into_subdirectories() {
        let temp = create_tree();

        assert_eq!(
            names(&search(&temp, "report", false, false)),
            vec!["nested-report.md", "root-report.txt"]
        );
    }

    #[test]
    fn regex_search_matches_the_pattern_not_the_literal() {
        let temp = create_tree();

        assert_eq!(
            names(&search(&temp, r"^nested.*\.md$", true, false)),
            vec!["nested-report.md"]
        );
    }

    #[test]
    fn wildcard_search_works_without_the_pattern_mode() {
        let temp = create_tree();

        // `regex: false`, the way the quick search sends a query with the toggle off.
        assert_eq!(names(&search(&temp, "*.md", false, false)), vec!["nested-report.md"]);
        assert!(names(&search(&temp, "*report*", false, false)).len() == 2);
        // Anchored, so a name merely containing ".md" is not a match.
        assert!(names(&search(&temp, "*.md", false, false))
            .iter()
            .all(|name| name.ends_with(".md")));
    }

    #[test]
    fn regex_search_reports_a_broken_pattern() {
        let temp = create_tree();

        let error = search_dir_recursive(
            temp.path().to_string_lossy().to_string(),
            RecursiveSearchOptions {
                search_key: "test:broken".to_string(),
                query: "[unclosed".to_string(),
                regex: true,
                include_hidden: false,
                max_results: None,
            },
        )
        .expect_err("broken pattern is rejected");

        assert!(error.contains("Invalid regular expression"));
    }

    #[test]
    fn hidden_entries_are_skipped_unless_asked_for() {
        let temp = create_tree();

        assert!(!names(&search(&temp, "report", false, false))
            .contains(&".hidden-report.txt".to_string()));

        let with_hidden = names(&search(&temp, "report", false, true));
        assert!(with_hidden.contains(&".hidden-report.txt".to_string()));
        // The hit inside the hidden directory proves the walk descends into it too.
        assert!(with_hidden.contains(&"report.txt".to_string()));
    }

    #[test]
    fn an_empty_query_returns_the_whole_subtree() {
        let temp = create_tree();
        let results = search(&temp, "", false, false);

        assert_eq!(
            names(&results),
            vec![
                "deeper",
                "nested",
                "nested-report.md",
                "root-report.txt",
                "unrelated.bin"
            ]
        );
    }

    #[test]
    fn hitting_the_result_cap_marks_the_results_truncated() {
        let temp = create_tree();

        let results = search_dir_recursive(
            temp.path().to_string_lossy().to_string(),
            RecursiveSearchOptions {
                search_key: "test:cap".to_string(),
                query: "report".to_string(),
                regex: false,
                include_hidden: false,
                max_results: Some(1),
            },
        )
        .expect("run search");

        assert_eq!(results.entries.len(), 1);
        assert!(results.truncated);
    }

    #[test]
    fn a_newer_search_supersedes_the_one_it_replaces() {
        let temp = create_tree();
        let search_key = "test:supersede";
        let first_id = claim_search_id(search_key);
        let second_id = claim_search_id(search_key);

        assert!(is_superseded(search_key, first_id));
        assert!(!is_superseded(search_key, second_id));

        cancel_recursive_search(search_key);
        assert!(is_superseded(search_key, second_id));

        drop(temp);
    }
}


