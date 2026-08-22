// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

/** A hit in the searched text, as a half-open offset range. */
export interface TextMatch {
  start: number;
  end: number;
}

export interface FindOptions {
  matchCase: boolean;
}

/** One run of the text, drawn either plain or as the match it belongs to. */
export interface TextSegment {
  text: string;
  /** Index into the match list, or `null` for text between matches. */
  matchIndex: number | null;
}

/**
 * Ceiling on matches collected per search. A one-letter query over a multi-megabyte file would
 * otherwise produce hundreds of thousands of hits that nothing can usefully show; the count
 * shown to the user stops being exact past this point, which is a fair trade for staying quick.
 */
export const MAX_TEXT_MATCHES = 100_000;

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/**
 * Every non-overlapping occurrence of `query` in `text`, in order.
 *
 * Matching goes through a regular expression rather than lower-casing both strings, because
 * case folding can change a string's length (`İ` folds to two code units) and the offsets
 * would then point into the wrong places of the original text. The `i` flag folds without
 * moving anything.
 */
export function findTextMatches(
  text: string,
  query: string,
  options: FindOptions,
  limit = MAX_TEXT_MATCHES,
): TextMatch[] {
  if (!query || !text) {
    return [];
  }

  const pattern = new RegExp(escapeRegExp(query), options.matchCase ? 'g' : 'gi');
  const matches: TextMatch[] = [];

  for (let hit = pattern.exec(text); hit !== null; hit = pattern.exec(text)) {
    matches.push({
      start: hit.index,
      end: hit.index + hit[0].length,
    });

    if (matches.length >= limit) {
      break;
    }
  }

  return matches;
}

/**
 * Where a fresh search should land: the first match at or after `position` — the caret, so
 * typing a query finds the next occurrence from where the reader is rather than jumping to
 * the top of the file — wrapping to the first match when nothing lies ahead.
 */
export function firstMatchIndexFrom(matches: readonly TextMatch[], position: number): number {
  if (matches.length === 0) {
    return -1;
  }

  const ahead = matches.findIndex(match => match.start >= position);

  return ahead === -1 ? 0 : ahead;
}

/** The next or previous match index, wrapping around at either end. */
export function stepMatchIndex(current: number, count: number, direction: 1 | -1): number {
  if (count <= 0) {
    return -1;
  }

  if (current < 0 || current >= count) {
    return direction === 1 ? 0 : count - 1;
  }

  return (current + direction + count) % count;
}

export function replaceMatch(text: string, match: TextMatch, replacement: string): string {
  return text.slice(0, match.start) + replacement + text.slice(match.end);
}

export function replaceAllMatches(
  text: string,
  matches: readonly TextMatch[],
  replacement: string,
): string {
  let result = '';
  let cursor = 0;

  for (const match of matches) {
    result += text.slice(cursor, match.start) + replacement;
    cursor = match.end;
  }

  return result + text.slice(cursor);
}

/**
 * Splits the text into alternating plain and matched runs, for drawing the matches behind an
 * editor. Matches past `limit` are left plain: drawing tens of thousands of marks costs more
 * than it shows, while the count and navigation keep working over the full list.
 */
export function segmentTextByMatches(
  text: string,
  matches: readonly TextMatch[],
  limit = matches.length,
): TextSegment[] {
  const segments: TextSegment[] = [];
  let cursor = 0;
  const drawn = Math.min(limit, matches.length);

  for (let index = 0; index < drawn; index += 1) {
    const match = matches[index];

    if (match.start > cursor) {
      segments.push({
        text: text.slice(cursor, match.start),
        matchIndex: null,
      });
    }

    segments.push({
      text: text.slice(match.start, match.end),
      matchIndex: index,
    });
    cursor = match.end;
  }

  if (cursor < text.length) {
    segments.push({
      text: text.slice(cursor),
      matchIndex: null,
    });
  }

  return segments;
}
