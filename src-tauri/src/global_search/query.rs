// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use crate::search_pattern::{
    compile_name_regex, compile_search_pattern, compile_wildcard_search_pattern,
    looks_like_wildcard, normalize_search_pattern, to_term_dictionary_pattern,
    wildcard_search_pattern,
};
use crate::utils::{
    is_hidden_path, metadata_times_unix_ms, normalize_path, path_extension_lowercase,
};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use tantivy::collector::TopDocs;
use tantivy::query::{AllQuery, BooleanQuery, FuzzyTermQuery, Occur, Query, RegexQuery, TermQuery};
use tantivy::schema::{IndexRecordOption, Value};
use tantivy::Term;
use tauri::Manager;

use super::ignore::{builtin_ignored_paths, get_drive_root, normalize_case, IgnoredPathMatcher};
use super::index::{index_dir, open_or_create_index};
use super::purge::report_missing_paths;
use super::scoring::{calculate_similarity_score, get_min_score_for_query_length};
use super::state::{GlobalSearchIndexFields, GLOBAL_SEARCH_STATE};
use super::types::{GlobalSearchQueryOptions, GlobalSearchResultEntry};

/// The index lags the filesystem between scans, so indexed hits are verified against
/// disk before they are shown. Dead hits are scored and sorted like any other, so more
/// candidates than were asked for are kept: dropping them must not leave the caller
/// short of results, nor let a dead entry take a live one's place.
const EXISTENCE_CHECK_OVERFETCH_FACTOR: usize = 2;
const EXISTENCE_CHECK_MIN_CANDIDATES: usize = 32;

/// Pattern hits are ranked equally, above any score a fuzzy match can reach, so a mixed
/// merge never demotes an exact pattern hit below a guess.
const REGEX_MATCH_SCORE: f32 = 1.0;

fn existence_check_candidate_limit(limit: usize) -> usize {
    limit
        .saturating_mul(EXISTENCE_CHECK_OVERFETCH_FACTOR)
        .max(EXISTENCE_CHECK_MIN_CANDIDATES)
}

/// Splits verified entries from the paths that have since disappeared. Link metadata is
/// read without following it, matching how the indexer decided what to store.
fn partition_existing_entries(
    entries: Vec<GlobalSearchResultEntry>,
) -> (Vec<GlobalSearchResultEntry>, Vec<String>) {
    let (existing, missing): (Vec<GlobalSearchResultEntry>, Vec<GlobalSearchResultEntry>) = entries
        .into_par_iter()
        .partition(|entry| std::fs::symlink_metadata(&entry.path).is_ok());

    (
        existing,
        missing.into_iter().map(|entry| entry.path).collect(),
    )
}

/// Takes one drive's scored hits down to the requested limit, dropping any whose path
/// is gone and reporting those so they can be purged from the index.
fn finalize_drive_group(
    mut entries: Vec<GlobalSearchResultEntry>,
    limit: usize,
) -> (Vec<GlobalSearchResultEntry>, Vec<String>) {
    entries.par_sort_by(|entry_a, entry_b| {
        entry_b
            .score
            .partial_cmp(&entry_a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    entries.truncate(existence_check_candidate_limit(limit));

    let (mut existing_entries, missing_paths) = partition_existing_entries(entries);
    existing_entries.truncate(limit);

    (existing_entries, missing_paths)
}

pub(super) fn build_query(
    fields: &GlobalSearchIndexFields,
    query: &str,
    options: &GlobalSearchQueryOptions,
) -> Result<Box<dyn Query>, String> {
    let normalized = normalize_case(query);

    if normalized.is_empty() {
        return Ok(Box::new(AllQuery));
    }

    match select_match_mode(query, options) {
        MatchMode::Pattern => return build_pattern_query(fields, query),
        MatchMode::Wildcard => return build_wildcard_query(fields, query, options.exact_match),
        MatchMode::Fuzzy => {}
    }

    if options.exact_match {
        return Ok(build_exact_match_query(fields, &normalized));
    }

    let words = split_query_tokens(&normalized);

    let max_distance = if options.typo_tolerance { 2 } else { 1 };

    let mut subqueries: Vec<(tantivy::query::Occur, Box<dyn Query>)> = Vec::new();

    for word in &words {
        let term = Term::from_field_text(fields.name, word);
        let fuzzy = FuzzyTermQuery::new(term, max_distance, true);
        subqueries.push((Occur::Should, Box::new(fuzzy)));
    }

    let whitespace_words: Vec<&str> = normalized.split_whitespace().collect();
    for word in whitespace_words {
        if word.contains('.') || word.contains('_') || word.contains('-') {
            let term = Term::from_field_text(fields.name, word);
            let fuzzy = FuzzyTermQuery::new(term, max_distance, true);
            subqueries.push((Occur::Should, Box::new(fuzzy)));
        }
    }

    Ok(Box::new(BooleanQuery::from(subqueries)))
}

/// How a query is read. The three are decided per query rather than only by settings: a
/// query carrying `*` or `?` is a wildcard whatever else is switched on, because that is
/// what those characters mean to everyone who types them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MatchMode {
    /// A regular expression, as typed.
    Pattern,
    /// Shell wildcards, and a plain query as a fragment of the name.
    Wildcard,
    /// Terms matched with a tolerance for typos, which is what this search did originally.
    Fuzzy,
}

pub(super) fn select_match_mode(query: &str, options: &GlobalSearchQueryOptions) -> MatchMode {
    if options.regex {
        return MatchMode::Pattern;
    }

    if looks_like_wildcard(query) {
        return MatchMode::Wildcard;
    }

    if options.typo_tolerance {
        return MatchMode::Fuzzy;
    }

    MatchMode::Wildcard
}

/// Runs a wildcard query over the term dictionary. A plain query searches for a fragment of
/// the name, which is the one thing the fuzzy engine could not do: it matches whole tokens,
/// so `ann` never finds `annual-report`.
fn build_wildcard_query(
    fields: &GlobalSearchIndexFields,
    query: &str,
    whole_name: bool,
) -> Result<Box<dyn Query>, String> {
    let pattern = wildcard_search_pattern(query, whole_name);
    let term_pattern = to_term_dictionary_pattern(&pattern);

    RegexQuery::from_pattern(&term_pattern, fields.name_lower)
        .map(|query| Box::new(query) as Box<dyn Query>)
        .map_err(|error| format!("Invalid search pattern: {error}"))
}

/// Runs the pattern over the term dictionary of the untokenized lowercased name, so a
/// filename is matched whole rather than word by word.
fn build_pattern_query(
    fields: &GlobalSearchIndexFields,
    pattern: &str,
) -> Result<Box<dyn Query>, String> {
    // A wildcard is translated to regex source first, so both engines are handed the same
    // meaning. Compiled up front so a half-typed pattern is reported as the user's mistake
    // rather than as an index failure.
    let normalized = normalize_search_pattern(pattern);
    compile_name_regex(&normalized)?;

    let term_pattern = to_term_dictionary_pattern(&normalized);

    RegexQuery::from_pattern(&term_pattern, fields.name_lower)
        .map(|query| Box::new(query) as Box<dyn Query>)
        .map_err(|error| format!("Invalid regular expression: {error}"))
}

fn build_exact_match_query(
    fields: &GlobalSearchIndexFields,
    normalized_query: &str,
) -> Box<dyn Query> {
    let words = split_query_tokens(normalized_query);

    if words.is_empty() {
        return Box::new(AllQuery);
    }

    if words.len() == 1 {
        let term = Term::from_field_text(fields.name, &words[0]);
        return Box::new(TermQuery::new(term, IndexRecordOption::Basic));
    }

    let subqueries: Vec<(Occur, Box<dyn Query>)> = words
        .iter()
        .map(|word| {
            let term = Term::from_field_text(fields.name, word);
            (
                Occur::Must,
                Box::new(TermQuery::new(term, IndexRecordOption::Basic)) as Box<dyn Query>,
            )
        })
        .collect();

    Box::new(BooleanQuery::from(subqueries))
}

fn split_query_tokens(value: &str) -> Vec<String> {
    value
        .split(|character: char| {
            character.is_whitespace() || character == '.' || character == '_' || character == '-'
        })
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_string())
        .collect()
}

fn matches_exact_name(query: &str, name: &str) -> bool {
    let query_tokens = split_query_tokens(&normalize_case(query));
    if query_tokens.is_empty() {
        return true;
    }

    let name_tokens = split_query_tokens(&normalize_case(name));
    query_tokens.iter().all(|query_token| {
        name_tokens
            .iter()
            .any(|name_token| name_token == query_token)
    })
}

pub(super) fn matches_type(
    doc_is_file: u64,
    doc_is_dir: u64,
    options: &GlobalSearchQueryOptions,
) -> bool {
    let is_file = doc_is_file == 1;
    let is_dir = doc_is_dir == 1;

    (options.include_files && is_file) || (options.include_directories && is_dir)
}

pub async fn global_search_query(
    app: tauri::AppHandle,
    query: String,
    options: GlobalSearchQueryOptions,
) -> Result<Vec<GlobalSearchResultEntry>, String> {
    let base_dir = app
        .path()
        .app_data_dir()
        .map_err(|error: tauri::Error| error.to_string())?;

    tauri::async_runtime::spawn_blocking(move || {
        global_search_query_blocking(base_dir, query, options)
    })
    .await
    .map_err(|join_error| format!("Global search query task failed: {join_error}"))?
}

fn global_search_query_blocking(
    base_dir: PathBuf,
    query: String,
    options: GlobalSearchQueryOptions,
) -> Result<Vec<GlobalSearchResultEntry>, String> {
    let index_path = index_dir(&base_dir);

    let needs_open = {
        let state = GLOBAL_SEARCH_STATE
            .read()
            .map_err(|error| error.to_string())?;
        if state.status.is_committing {
            return Ok(Vec::new());
        }
        state.index.is_none() || state.reader.is_none() || state.fields.is_none()
    };

    if needs_open {
        let (index, reader, fields) = open_or_create_index(&index_path)?;
        let mut state = GLOBAL_SEARCH_STATE
            .write()
            .map_err(|error| error.to_string())?;

        if state.status.is_committing {
            return Ok(Vec::new());
        }

        if state.index.is_none() || state.reader.is_none() || state.fields.is_none() {
            state.index = Some(index);
            state.reader = Some(reader);
            state.fields = Some(fields);
        }
    }

    // Cloning the handles keeps the state lock out of the search itself, which also lets
    // the purge below take the write lock without contending with a query in flight.
    let (reader, fields) = {
        let state = GLOBAL_SEARCH_STATE
            .read()
            .map_err(|error| error.to_string())?;

        if state.status.is_committing {
            return Ok(Vec::new());
        }

        let reader = state
            .reader
            .as_ref()
            .ok_or_else(|| "Search index reader is not initialized".to_string())?
            .clone();
        let fields = *state
            .fields
            .as_ref()
            .ok_or_else(|| "Search index fields are not initialized".to_string())?;

        (reader, fields)
    };

    let searcher = reader.searcher();
    let normalized_query = normalize_case(&query);

    let match_mode = select_match_mode(&query, &options);
    let query_boxed = build_query(&fields, &query, &options)?;
    let top_docs = searcher
        .search(&query_boxed, &TopDocs::with_limit(100_000).order_by_score())
        .map_err(|error| error.to_string())?;

    let internal_ignored: Vec<String> = builtin_ignored_paths()
        .iter()
        .map(|path| path.to_string())
        .collect();
    let internal_ignored_matcher = IgnoredPathMatcher::new(&internal_ignored);

    let min_score = options
        .min_score_threshold
        .unwrap_or_else(|| get_min_score_for_query_length(normalized_query.len()));

    let results: Vec<GlobalSearchResultEntry> = top_docs
        .par_iter()
        .filter_map(|(_tantivy_score, doc_address)| {
            let retrieved: tantivy::TantivyDocument = searcher.doc(*doc_address).ok()?;

            let path_value = retrieved
                .get_first(fields.path)
                .and_then(|value| value.as_str())?
                .to_string();

            if internal_ignored_matcher.is_ignored(&path_value) {
                return None;
            }

            let name_value = retrieved
                .get_first(fields.name)
                .and_then(|value| value.as_str())?
                .to_string();

            // Which engine ran decides how a hit is scored. Only the fuzzy one guesses, so
            // only it needs a threshold to throw its own worst guesses away; the index has
            // already decided the other two, and scoring them again could only drop a hit
            // the user asked for by name.
            let name_score = match match_mode {
                MatchMode::Pattern => REGEX_MATCH_SCORE,
                // Ranked, not filtered: the closer a name is to what was typed, the higher
                // it sits, which is what makes a truncated result list keep the best hits.
                MatchMode::Wildcard => calculate_similarity_score(&normalized_query, &name_value),
                MatchMode::Fuzzy => {
                    if options.exact_match && !matches_exact_name(&normalized_query, &name_value) {
                        return None;
                    }

                    let similarity_score =
                        calculate_similarity_score(&normalized_query, &name_value);

                    if similarity_score < min_score {
                        return None;
                    }

                    similarity_score
                }
            };

            let doc_is_file = retrieved
                .get_first(fields.is_file)
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let doc_is_dir = retrieved
                .get_first(fields.is_dir)
                .and_then(|value| value.as_u64())
                .unwrap_or(0);

            if !matches_type(doc_is_file, doc_is_dir, &options) {
                return None;
            }

            let modified_time = retrieved
                .get_first(fields.modified_time)
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let size = retrieved
                .get_first(fields.size)
                .and_then(|value| value.as_u64())
                .unwrap_or(0);

            let ext = path_extension_lowercase(Path::new(&path_value));

            Some(GlobalSearchResultEntry {
                name: name_value,
                ext,
                path: path_value,
                size,
                item_count: None,
                modified_time,
                accessed_time: 0,
                created_time: 0,
                mime: None,
                is_file: doc_is_file == 1,
                is_dir: doc_is_dir == 1,
                is_symlink: false,
                is_hidden: false,
                score: name_score,
            })
        })
        .collect();

    let mut drive_groups: std::collections::HashMap<String, Vec<GlobalSearchResultEntry>> =
        std::collections::HashMap::new();

    for entry in results {
        let drive_root = get_drive_root(&entry.path);
        drive_groups.entry(drive_root).or_default().push(entry);
    }

    let mut final_results: Vec<GlobalSearchResultEntry> = Vec::new();
    let mut missing_paths: Vec<String> = Vec::new();
    for (_drive, entries) in drive_groups {
        let (existing_entries, missing_entry_paths) = finalize_drive_group(entries, options.limit);
        missing_paths.extend(missing_entry_paths);
        final_results.extend(existing_entries);
    }

    final_results.par_sort_by(|entry_a, entry_b| {
        entry_b
            .score
            .partial_cmp(&entry_a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    report_missing_paths(&base_dir, missing_paths);

    Ok(final_results)
}

pub async fn global_search_query_paths(
    paths: Vec<String>,
    query: String,
    options: GlobalSearchQueryOptions,
) -> Result<Vec<GlobalSearchResultEntry>, String> {
    if paths.is_empty() || query.trim().is_empty() {
        return Ok(Vec::new());
    }

    tauri::async_runtime::spawn_blocking(move || {
        global_search_query_paths_blocking(paths, query, options)
    })
    .await
    .map_err(|join_error| format!("Global search path query task failed: {join_error}"))?
}

fn global_search_query_paths_blocking(
    paths: Vec<String>,
    query: String,
    options: GlobalSearchQueryOptions,
) -> Result<Vec<GlobalSearchResultEntry>, String> {
    let normalized_query = normalize_case(&query);
    let min_score = get_min_score_for_query_length(normalized_query.len());
    // The same three engines the indexed search picks between, so a query means one thing
    // no matter which half of the results it is being matched against.
    let match_mode = select_match_mode(&query, &options);
    let name_pattern = match match_mode {
        MatchMode::Pattern => Some(compile_search_pattern(&query)?),
        MatchMode::Wildcard => Some(compile_wildcard_search_pattern(&query, options.exact_match)?),
        MatchMode::Fuzzy => None,
    };

    let all_searchable_paths: Vec<PathBuf> = paths
        .par_iter()
        .flat_map(|path_string| {
            let path = Path::new(path_string);
            let mut collected_paths = Vec::new();

            if let Ok(metadata) = std::fs::metadata(path) {
                if metadata.is_dir() {
                    if let Ok(entries) = std::fs::read_dir(path) {
                        for entry_result in entries.flatten() {
                            collected_paths.push(entry_result.path());
                        }
                    }
                } else {
                    collected_paths.push(path.to_path_buf());
                }
            }

            collected_paths
        })
        .collect();

    let results: Vec<GlobalSearchResultEntry> = all_searchable_paths
        .par_iter()
        .filter_map(|path| {
            let name = path
                .file_name()
                .and_then(|segment| segment.to_str())?
                .to_string();

            let name_score = if let Some(pattern) = name_pattern.as_ref() {
                if !pattern.is_match(&name) {
                    return None;
                }

                if match_mode == MatchMode::Wildcard {
                    calculate_similarity_score(&normalized_query, &name)
                } else {
                    REGEX_MATCH_SCORE
                }
            } else {
                if options.exact_match && !matches_exact_name(&normalized_query, &name) {
                    return None;
                }

                let similarity_score = calculate_similarity_score(&normalized_query, &name);

                if similarity_score < min_score {
                    return None;
                }

                similarity_score
            };

            let metadata = std::fs::metadata(path).ok()?;

            let is_file = metadata.is_file();
            let is_dir = metadata.is_dir();

            if !matches_type(
                if is_file { 1 } else { 0 },
                if is_dir { 1 } else { 0 },
                &options,
            ) {
                return None;
            }

            let (modified_time, accessed_time, created_time) = metadata_times_unix_ms(&metadata);

            let size = if is_file { metadata.len() } else { 0 };

            let ext = path_extension_lowercase(path);

            let path_string = path.to_string_lossy().to_string();
            let normalized_path = normalize_path(&path_string);

            Some(GlobalSearchResultEntry {
                name,
                ext,
                path: normalized_path,
                size,
                item_count: None,
                modified_time,
                accessed_time,
                created_time,
                mime: None,
                is_file,
                is_dir,
                is_symlink: metadata.is_symlink(),
                is_hidden: is_hidden_path(path),
                score: name_score,
            })
        })
        .collect();

    let mut sorted_results = results;
    sorted_results.par_sort_by(|entry_a, entry_b| {
        entry_b
            .score
            .partial_cmp(&entry_a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if sorted_results.len() > options.limit {
        sorted_results.truncate(options.limit);
    }

    Ok(sorted_results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::global_search::index::build_schema;
    use tantivy::doc;

    fn create_options(exact_match: bool) -> GlobalSearchQueryOptions {
        GlobalSearchQueryOptions {
            limit: 10,
            include_files: true,
            include_directories: true,
            exact_match,
            typo_tolerance: true,
            min_score_threshold: Some(0.0),
            regex: false,
        }
    }

    fn create_regex_options() -> GlobalSearchQueryOptions {
        GlobalSearchQueryOptions {
            regex: true,
            ..create_options(false)
        }
    }

    /// How a query arrives with no pattern mode and no typo tolerance switched on.
    fn create_plain_options() -> GlobalSearchQueryOptions {
        GlobalSearchQueryOptions {
            typo_tolerance: false,
            ..create_options(false)
        }
    }

    fn search_names(query_text: &str, exact_match: bool) -> Vec<String> {
        search_names_with_options(query_text, &create_options(exact_match))
    }

    fn search_names_with_options(
        query_text: &str,
        options: &GlobalSearchQueryOptions,
    ) -> Vec<String> {
        let (schema, fields) = build_schema();
        let index = tantivy::Index::create_in_ram(schema);
        let mut writer = index.writer(50_000_000).unwrap();

        for name in [
            "Exile by Aleksey Hoffman.jpg",
            "Exiled by Aleksey Hoffman.jpg",
            "Example.jpg",
        ] {
            writer
                .add_document(doc!(
                    fields.path => format!("C:/test/{name}"),
                    fields.name => name.to_string(),
                    fields.name_lower => name.to_lowercase(),
                    fields.is_file => 1u64,
                    fields.is_dir => 0u64,
                    fields.modified_time => 0u64,
                    fields.size => 0u64,
                ))
                .unwrap();
        }

        writer.commit().unwrap();

        let reader = index.reader().unwrap();
        let searcher = reader.searcher();
        let query = build_query(&fields, query_text, options).unwrap();
        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(10).order_by_score())
            .unwrap();

        top_docs
            .into_iter()
            .filter_map(|(_score, address)| {
                let doc: tantivy::TantivyDocument = searcher.doc(address).ok()?;
                doc.get_first(fields.name)
                    .and_then(|value| value.as_str())
                    .map(|value| value.to_string())
            })
            .collect()
    }

    #[test]
    fn exact_match_query_matches_exact_filename_token() {
        let names = search_names("exile", true);

        assert!(names.contains(&"Exile by Aleksey Hoffman.jpg".to_string()));
    }

    #[test]
    fn exact_match_query_does_not_match_prefixed_token() {
        let names = search_names("exile", true);

        assert!(!names.contains(&"Exiled by Aleksey Hoffman.jpg".to_string()));
    }

    #[test]
    fn regex_query_matches_a_pattern_across_the_whole_filename() {
        // The tokenized name field would have split this into words; the pattern spans the
        // spaces and the extension, so only the untokenized field can answer it.
        let names = search_names_with_options(r"^exile by .*\.jpg$", &create_regex_options());

        assert_eq!(names, vec!["Exile by Aleksey Hoffman.jpg".to_string()]);
    }

    #[test]
    fn regex_query_matches_an_unanchored_pattern_anywhere_in_the_name() {
        let mut names = search_names_with_options("ex[ai]", &create_regex_options());
        names.sort();

        assert_eq!(
            names,
            vec![
                "Example.jpg".to_string(),
                "Exile by Aleksey Hoffman.jpg".to_string(),
                "Exiled by Aleksey Hoffman.jpg".to_string(),
            ]
        );
    }

    #[test]
    fn regex_query_supports_alternation_without_swallowing_the_padding() {
        let mut names = search_names_with_options("example|exiled", &create_regex_options());
        names.sort();

        assert_eq!(
            names,
            vec![
                "Example.jpg".to_string(),
                "Exiled by Aleksey Hoffman.jpg".to_string(),
            ]
        );
    }

    #[test]
    fn a_wildcard_query_matches_the_way_a_shell_would() {
        // `*.jpg` is not valid regex syntax at all, so this only works if the wildcard is
        // recognised and translated before the index ever sees it.
        let mut names = search_names_with_options("*.jpg", &create_regex_options());
        names.sort();

        assert_eq!(
            names,
            vec![
                "Example.jpg".to_string(),
                "Exile by Aleksey Hoffman.jpg".to_string(),
                "Exiled by Aleksey Hoffman.jpg".to_string(),
            ]
        );
    }

    #[test]
    fn a_wildcard_query_is_anchored_to_the_whole_name() {
        // `exile*` reaches from the start of the name, so it takes both Exile and Exiled.
        assert_eq!(search_names_with_options("exile*", &create_regex_options()).len(), 2);
        assert_eq!(search_names_with_options("*hoffman*", &create_regex_options()).len(), 2);
        // Anchored: no name begins with "hoffman", however many contain it.
        assert!(search_names_with_options("hoffman*", &create_regex_options()).is_empty());
    }

    #[test]
    fn a_plain_query_finds_a_fragment_of_the_name() {
        // The fuzzy engine matches whole tokens, so this prefix found nothing before.
        let names = search_names_with_options("exil", &create_plain_options());

        assert_eq!(names.len(), 2);
    }

    #[test]
    fn a_wildcard_query_works_without_the_pattern_mode() {
        let mut names = search_names_with_options("*.jpg", &create_plain_options());
        names.sort();

        assert_eq!(names.len(), 3);
        assert!(names.iter().all(|name| name.ends_with(".jpg")));
    }

    #[test]
    fn a_wildcard_query_beats_typo_tolerance_to_the_query() {
        // Typo tolerance left on must not turn `*.jpg` back into a fuzzy guess.
        let options = GlobalSearchQueryOptions {
            typo_tolerance: true,
            ..create_options(false)
        };

        assert_eq!(select_match_mode("*.jpg", &options), MatchMode::Wildcard);
        assert_eq!(search_names_with_options("*.jpg", &options).len(), 3);
    }

    #[test]
    fn typo_tolerance_still_reaches_the_fuzzy_engine() {
        let options = GlobalSearchQueryOptions {
            typo_tolerance: true,
            ..create_options(false)
        };

        assert_eq!(select_match_mode("exile", &options), MatchMode::Fuzzy);
        assert_eq!(select_match_mode("exile", &create_plain_options()), MatchMode::Wildcard);
        assert_eq!(select_match_mode("exile", &create_regex_options()), MatchMode::Pattern);
    }

    #[test]
    fn exact_match_asks_for_the_whole_name() {
        let options = GlobalSearchQueryOptions {
            exact_match: true,
            ..create_plain_options()
        };

        assert!(search_names_with_options("exile", &options).is_empty());
        assert_eq!(
            search_names_with_options("Exile by Aleksey Hoffman.jpg", &options),
            vec!["Exile by Aleksey Hoffman.jpg".to_string()]
        );
    }

    #[test]
    fn regex_query_reports_a_broken_pattern() {
        let (schema, fields) = build_schema();
        let _index = tantivy::Index::create_in_ram(schema);

        let error = build_query(&fields, "[unclosed", &create_regex_options())
            .expect_err("broken pattern is rejected");

        assert!(error.contains("Invalid regular expression"));
    }

    fn create_entry(path: &str, score: f32) -> GlobalSearchResultEntry {
        GlobalSearchResultEntry {
            name: Path::new(path)
                .file_name()
                .and_then(|segment| segment.to_str())
                .unwrap_or(path)
                .to_string(),
            ext: None,
            path: path.to_string(),
            size: 0,
            item_count: None,
            modified_time: 0,
            accessed_time: 0,
            created_time: 0,
            mime: None,
            is_file: true,
            is_dir: false,
            is_symlink: false,
            is_hidden: false,
            score,
        }
    }

    #[test]
    fn candidate_limit_over_fetches_so_pruning_can_still_fill_the_limit() {
        assert_eq!(existence_check_candidate_limit(100), 200);
    }

    #[test]
    fn candidate_limit_keeps_a_floor_for_small_limits() {
        assert_eq!(
            existence_check_candidate_limit(1),
            EXISTENCE_CHECK_MIN_CANDIDATES
        );
    }

    #[test]
    fn candidate_limit_saturates_instead_of_overflowing() {
        assert_eq!(existence_check_candidate_limit(usize::MAX), usize::MAX);
    }

    #[test]
    fn partitioning_keeps_entries_on_disk_and_reports_the_rest() {
        let temp = tempfile::TempDir::new().unwrap();
        let existing_path = temp.path().join("still-here");
        std::fs::write(&existing_path, b"").unwrap();

        let existing_path_string = existing_path.to_string_lossy().to_string();
        let missing_path_string = temp.path().join("deleted").to_string_lossy().to_string();

        let (existing, missing) = partition_existing_entries(vec![
            create_entry(&existing_path_string, 1.0),
            create_entry(&missing_path_string, 0.9),
        ]);

        assert_eq!(
            existing
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<String>>(),
            vec![existing_path_string]
        );
        assert_eq!(missing, vec![missing_path_string]);
    }

    #[test]
    fn deleted_entries_are_dropped_and_live_ones_still_fill_the_limit() {
        let temp = tempfile::TempDir::new().unwrap();
        let limit = 3;

        // Deleted entries outscore the live ones, so without over-fetching they would
        // take every slot and leave the caller with nothing.
        let mut entries = Vec::new();
        let mut deleted_paths = Vec::new();

        for index in 0..limit {
            let path = temp
                .path()
                .join(format!("deleted-{index}"))
                .to_string_lossy()
                .to_string();
            entries.push(create_entry(&path, 1.0));
            deleted_paths.push(path);
        }

        let mut live_paths = Vec::new();

        for index in 0..limit {
            let path = temp.path().join(format!("live-{index}"));
            std::fs::write(&path, b"").unwrap();
            let path = path.to_string_lossy().to_string();
            entries.push(create_entry(&path, 0.5));
            live_paths.push(path);
        }

        let (results, missing) = finalize_drive_group(entries, limit);

        assert_eq!(
            results
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<String>>(),
            live_paths
        );

        let mut missing_sorted = missing;
        missing_sorted.sort();
        deleted_paths.sort();
        assert_eq!(missing_sorted, deleted_paths);
    }

    #[test]
    fn partitioning_keeps_a_dangling_symlink_it_can_still_stat() {
        #[cfg(unix)]
        {
            let temp = tempfile::TempDir::new().unwrap();
            let link_path = temp.path().join("dangling");
            std::os::unix::fs::symlink(temp.path().join("no-such-target"), &link_path).unwrap();

            let link_path_string = link_path.to_string_lossy().to_string();
            let (existing, missing) =
                partition_existing_entries(vec![create_entry(&link_path_string, 1.0)]);

            assert_eq!(existing.len(), 1);
            assert!(missing.is_empty());
        }
    }

    #[test]
    fn exact_name_filter_matches_all_query_tokens() {
        assert!(matches_exact_name(
            "exile hoffman",
            "Exile by Aleksey Hoffman.jpg"
        ));
        assert!(!matches_exact_name(
            "exile missing",
            "Exile by Aleksey Hoffman.jpg"
        ));
    }
}

