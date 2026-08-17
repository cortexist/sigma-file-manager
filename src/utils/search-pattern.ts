// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

/**
 * Reading the pattern a person typed.
 *
 * Asked for a pattern, most people type a shell wildcard — `*.png` — rather than a regular
 * expression, and a wildcard is not valid regex syntax: `*.png` does not compile at all.
 * So a query is read as a wildcard when it looks like one, and as a regular expression
 * otherwise.
 *
 * The Rust side mirrors these rules in `src-tauri/src/search_pattern.rs`; the two must
 * agree, or the same query would mean different things in the two searches.
 */

/**
 * Characters that only a regular expression would contain. A query holding any of them is
 * taken at its word as regex, so a deliberate pattern is never reinterpreted as a wildcard.
 */
const REGEX_ONLY_METACHARACTERS = /[\\^$(){}|]/;

const WILDCARD_CHARACTERS = /[*?]/;

const REGEX_ESCAPE_PATTERN = /[.*+?^${}()|[\]\\]/g;

/**
 * Wildcards are the shell's syntax, so a query is one when it uses `*` or `?` and holds
 * nothing that belongs to regular expressions alone. `+` is deliberately absent from that
 * list: a filename may contain one, and `c++` is a wildcard-free query either way.
 */
export function looksLikeWildcard(query: string): boolean {
  return WILDCARD_CHARACTERS.test(query) && !REGEX_ONLY_METACHARACTERS.test(query);
}

function escapeRegExp(value: string): string {
  return value.replace(REGEX_ESCAPE_PATTERN, '\\$&');
}

/**
 * Translates shell wildcard syntax into a regular expression. The result is anchored,
 * because `*.png` asks about the whole name: a file called `notes.png.txt` is not a match.
 */
export function wildcardToRegExpSource(wildcard: string): string {
  let source = '^';
  let index = 0;

  while (index < wildcard.length) {
    const character = wildcard[index];

    if (character === '*') {
      source += '.*';
      index += 1;
      continue;
    }

    if (character === '?') {
      source += '.';
      index += 1;
      continue;
    }

    if (character === '[') {
      const closingIndex = wildcard.indexOf(']', index + 2);

      // An unterminated `[` is a literal bracket, and what follows keeps its meaning.
      if (closingIndex === -1) {
        source += escapeRegExp('[');
        index += 1;
        continue;
      }

      const body = wildcard.slice(index + 1, closingIndex);
      // The shell negates a class with `!`, regex with `^`.
      const negated = body.startsWith('!');
      source += `[${negated ? '^' : ''}${(negated ? body.slice(1) : body).replace(/\\/g, '\\\\')}]`;
      index = closingIndex + 1;
      continue;
    }

    source += escapeRegExp(character);
    index += 1;
  }

  return `${source}$`;
}

/** Reads a query the way the person typing it meant it, and returns regex source. */
export function normalizeSearchPattern(query: string): string {
  return looksLikeWildcard(query) ? wildcardToRegExpSource(query) : query;
}

/**
 * Compiles a query the user typed, wildcard or regular expression, or reports why it
 * cannot be compiled.
 */
export function compileSearchPattern(
  query: string,
  flags: string,
): {
  pattern: RegExp | null;
  error: string | null;
} {
  if (!query) {
    return {
      pattern: null,
      error: null,
    };
  }

  try {
    return {
      pattern: new RegExp(normalizeSearchPattern(query), flags),
      error: null,
    };
  }
  catch (error) {
    return {
      pattern: null,
      error: error instanceof Error ? error.message : String(error),
    };
  }
}
