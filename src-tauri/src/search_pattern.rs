// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! Pattern handling shared by the two searches.
//!
//! Both surfaces promise the same thing to the user: a pattern tested against an entry's
//! name, case-insensitive unless the pattern says otherwise. Two translations live here,
//! because two things disagree with that promise. The first is the user: asked for a
//! pattern, most people type a shell wildcard (`*.png`) rather than a regular expression,
//! and a wildcard is not valid regex syntax. The second is tantivy, whose term-dictionary
//! regex matches whole terms and rejects anchors.
//!
//! The TypeScript side mirrors the wildcard rules in `src/utils/search-pattern.ts`; the two
//! must agree, or the same query would mean different things on either side of the wall.

use regex::{Regex, RegexBuilder};

/// A pathological pattern compiles to a large automaton before it ever runs. The searches
/// are interactive and typed one character at a time, so the compiler is kept on a leash.
const REGEX_SIZE_LIMIT_BYTES: usize = 1 << 20;

/// Characters that only a regular expression would contain. A query holding any of them is
/// taken at its word as regex, so a deliberate pattern is never reinterpreted as a wildcard.
const REGEX_ONLY_METACHARACTERS: [char; 8] = ['\\', '^', '$', '(', ')', '{', '}', '|'];

/// Reads a query the way the person typing it meant it: `*.png` is a wildcard, `^.*\.png$`
/// is a regular expression. Returns regex source either way.
pub fn normalize_search_pattern(query: &str) -> String {
    if looks_like_wildcard(query) {
        return wildcard_to_regex(query);
    }

    query.to_string()
}

/// Wildcards are the shell's syntax, so a query is one when it uses `*` or `?` and holds
/// nothing that belongs to regular expressions alone. `+` is deliberately absent from that
/// list: a filename may contain one, and `c++` is a wildcard-free query either way.
pub fn looks_like_wildcard(query: &str) -> bool {
    query.contains(['*', '?'])
        && !query.contains(REGEX_ONLY_METACHARACTERS)
}

/// Translates shell wildcard syntax into regex source. The result is anchored, because
/// `*.png` asks about the whole name: a file called `notes.png.txt` is not a match.
pub fn wildcard_to_regex(wildcard: &str) -> String {
    let mut pattern = String::from("^");
    let mut characters = wildcard.chars().peekable();

    while let Some(character) = characters.next() {
        match character {
            '*' => pattern.push_str(".*"),
            '?' => pattern.push('.'),
            '[' => push_wildcard_class(&mut pattern, &mut characters),
            _ => pattern.push_str(&regex::escape(&character.to_string())),
        }
    }

    pattern.push('$');
    pattern
}

/// Copies a `[…]` character class across, which regex spells the same way apart from
/// negation. An unterminated `[` is a literal bracket, and the rest of the pattern after it
/// keeps its meaning — the lookahead is on a copy of the iterator so nothing is consumed
/// unless the class actually closes.
fn push_wildcard_class(
    pattern: &mut String,
    characters: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    let mut class = String::new();
    let mut is_terminated = false;

    for character in characters.clone() {
        if character == ']' && !class.is_empty() {
            is_terminated = true;
            break;
        }

        class.push(character);
    }

    if !is_terminated {
        pattern.push_str(&regex::escape("["));
        return;
    }

    // Consume the class and its closing bracket now that it is known to be one.
    for _ in 0..class.chars().count() + 1 {
        characters.next();
    }

    pattern.push('[');

    // The shell negates a class with `!`, regex with `^`.
    if let Some(rest) = class.strip_prefix('!') {
        pattern.push('^');
        pattern.push_str(&rest.replace('\\', r"\\"));
    } else {
        pattern.push_str(&class.replace('\\', r"\\"));
    }

    pattern.push(']');
}

/// Compiles a query the user typed, wildcard or regular expression.
pub fn compile_search_pattern(query: &str) -> Result<Regex, String> {
    compile_name_regex(&normalize_search_pattern(query))
}

/// Regex source for a query typed with no pattern mode switched on, which is how a shell
/// reads one: `*` and `?` are wildcards, and anything else is a plain name.
///
/// A shell matches a whole name, but a search box is not a shell — the name being looked for
/// is usually a fragment of a longer one. So a query with no wildcards of its own is padded
/// into a substring search, which is what every file search that takes wildcards does.
/// `whole_name` is the caller's "exact match" setting and drops the padding.
pub fn wildcard_search_pattern(query: &str, whole_name: bool) -> String {
    if whole_name || looks_like_wildcard(query) {
        return wildcard_to_regex(query);
    }

    wildcard_to_regex(&format!("*{query}*"))
}

/// Compiles the query of a search with no pattern mode switched on.
pub fn compile_wildcard_search_pattern(query: &str, whole_name: bool) -> Result<Regex, String> {
    compile_name_regex(&wildcard_search_pattern(query, whole_name))
}

/// Compiles regex source for matching entry names one at a time.
pub fn compile_name_regex(pattern: &str) -> Result<Regex, String> {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .size_limit(REGEX_SIZE_LIMIT_BYTES)
        .build()
        .map_err(|error| format!("Invalid regular expression: {error}"))
}

/// Rewrites a pattern for tantivy's term-dictionary regex, which matches a whole term and
/// rejects the `^` and `$` anchors outright. Consuming the anchors and padding the
/// unanchored sides with `.*` turns full-term matching back into the substring matching
/// the user typed the pattern for.
pub fn to_term_dictionary_pattern(pattern: &str) -> String {
    let mut body = pattern;
    let anchored_start = body.starts_with('^');

    if anchored_start {
        body = &body[1..];
    }

    let anchored_end = ends_with_unescaped_dollar(body);

    if anchored_end {
        body = &body[..body.len() - 1];
    }

    if body.is_empty() {
        return "(?i).*".to_string();
    }

    let prefix = if anchored_start { "" } else { ".*" };
    let suffix = if anchored_end { "" } else { ".*" };

    // The body is grouped because a top-level alternation would otherwise bind the padding
    // to the first and last branches only: `foo|bar` must not become `.*foo|bar.*`.
    format!("(?i){prefix}(?:{body}){suffix}")
}

/// A trailing `$` is an anchor only when it is not itself escaped, and a `\` in front of it
/// is only an escape when it is not itself escaped, so the run length decides.
fn ends_with_unescaped_dollar(pattern: &str) -> bool {
    if !pattern.ends_with('$') {
        return false;
    }

    let preceding_backslashes = pattern[..pattern.len() - 1]
        .chars()
        .rev()
        .take_while(|character| *character == '\\')
        .count();

    preceding_backslashes % 2 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(query: &str, name: &str) -> bool {
        compile_search_pattern(query).unwrap().is_match(name)
    }

    #[test]
    fn a_wildcard_query_is_read_as_a_wildcard() {
        assert!(looks_like_wildcard("*.png"));
        assert!(looks_like_wildcard("report?.txt"));
        assert!(!looks_like_wildcard("report"));
    }

    #[test]
    fn a_query_holding_regex_syntax_is_left_as_regex() {
        assert!(!looks_like_wildcard(r"^.*\.png$"));
        assert!(!looks_like_wildcard("(png|jpg)*"));
        assert!(!looks_like_wildcard(r"\d?"));
    }

    #[test]
    fn a_wildcard_matches_the_whole_name() {
        assert!(matches("*.png", "photo.png"));
        assert!(matches("*.png", "PHOTO.PNG"));
        // Anchored: the name ends there, it does not merely contain it.
        assert!(!matches("*.png", "photo.png.txt"));
        assert!(!matches("*.png", "photo.jpg"));
    }

    #[test]
    fn a_wildcard_question_mark_stands_for_one_character() {
        assert!(matches("report?.txt", "report1.txt"));
        assert!(!matches("report?.txt", "report.txt"));
        assert!(!matches("report?.txt", "report12.txt"));
    }

    #[test]
    fn a_wildcard_dot_is_a_literal_dot() {
        // The regex reading of `a*.png` would match `axpng`, because `.` is any character
        // there. Read as a wildcard, the dot is a dot and `?` is the way to say "any".
        assert!(!matches("a*.png", "axpng"));
        assert!(matches("a*.png", "ax.png"));
        assert!(matches("a?png", "axpng"));
    }

    #[test]
    fn a_wildcard_class_keeps_its_meaning() {
        assert!(matches("[0-9]*.png", "1photo.png"));
        assert!(!matches("[0-9]*.png", "photo.png"));
        assert!(matches("[!0-9]*.png", "photo.png"));
        assert!(!matches("[!0-9]*.png", "1photo.png"));
    }

    #[test]
    fn an_unterminated_class_is_a_literal_bracket() {
        assert!(matches("[draft*", "[draft-1.txt"));
    }

    #[test]
    fn a_regular_expression_still_wins_when_the_query_holds_regex_syntax() {
        assert!(matches(r"\.png$", "photo.png"));
        assert!(matches(r"^photo", "photo.png"));
        // Unanchored, unlike a wildcard.
        assert!(matches(r"\.png", "photo.png.txt"));
    }

    fn matches_without_pattern_mode(query: &str, name: &str, whole_name: bool) -> bool {
        compile_wildcard_search_pattern(query, whole_name)
            .unwrap()
            .is_match(name)
    }

    #[test]
    fn a_plain_query_searches_for_a_fragment_of_the_name() {
        assert!(matches_without_pattern_mode("report", "annual-report-2024.pdf", false));
        assert!(matches_without_pattern_mode("ann", "annual-report-2024.pdf", false));
        assert!(!matches_without_pattern_mode("invoice", "annual-report-2024.pdf", false));
    }

    #[test]
    fn a_wildcard_query_keeps_its_own_anchoring() {
        // The padding a plain query gets would turn this into "contains .png", which is not
        // what `*.png` says.
        assert!(matches_without_pattern_mode("*.png", "photo.png", false));
        assert!(!matches_without_pattern_mode("*.png", "photo.png.txt", false));
    }

    #[test]
    fn exact_match_drops_the_padding() {
        assert!(matches_without_pattern_mode("report", "report", true));
        assert!(!matches_without_pattern_mode("report", "annual-report.pdf", true));
    }

    #[test]
    fn a_plain_query_is_a_literal_not_a_pattern() {
        // Regex metacharacters in a query without a pattern mode are just characters.
        assert!(matches_without_pattern_mode("a.b", "xx-a.b-yy", false));
        assert!(!matches_without_pattern_mode("a.b", "xx-axb-yy", false));
    }

    #[test]
    fn name_regex_ignores_case() {
        let regex = compile_name_regex("readme").unwrap();

        assert!(regex.is_match("README.md"));
    }

    #[test]
    fn name_regex_matches_without_anchors() {
        let regex = compile_name_regex(r"\.rs$").unwrap();

        assert!(regex.is_match("main.rs"));
        assert!(!regex.is_match("main.rs.bak"));
    }

    #[test]
    fn name_regex_reports_a_broken_pattern() {
        assert!(compile_name_regex("[unclosed").is_err());
    }

    #[test]
    fn term_pattern_pads_an_unanchored_pattern() {
        assert_eq!(to_term_dictionary_pattern("report"), "(?i).*(?:report).*");
    }

    #[test]
    fn term_pattern_consumes_both_anchors() {
        assert_eq!(to_term_dictionary_pattern("^report$"), "(?i)(?:report)");
    }

    #[test]
    fn term_pattern_pads_only_the_unanchored_side() {
        assert_eq!(to_term_dictionary_pattern("^report"), "(?i)(?:report).*");
        assert_eq!(to_term_dictionary_pattern(r"\.rs$"), r"(?i).*(?:\.rs)");
    }

    #[test]
    fn term_pattern_keeps_an_escaped_dollar_as_a_literal() {
        assert_eq!(to_term_dictionary_pattern(r"cost\$"), r"(?i).*(?:cost\$).*");
    }

    #[test]
    fn term_pattern_keeps_an_escaped_backslash_before_the_anchor() {
        assert_eq!(
            to_term_dictionary_pattern(r"path\\$"),
            r"(?i).*(?:path\\)"
        );
    }

    #[test]
    fn term_pattern_groups_a_top_level_alternation() {
        assert_eq!(to_term_dictionary_pattern("png|jpg"), "(?i).*(?:png|jpg).*");
    }

    #[test]
    fn term_pattern_falls_back_to_matching_everything() {
        assert_eq!(to_term_dictionary_pattern("^$"), "(?i).*");
    }

}
