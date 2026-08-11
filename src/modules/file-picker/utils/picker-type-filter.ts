// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import type { DirEntry } from '@/types/dir-entry';

/**
 * The caller's "show me only these files" narrowed to what the picker can answer.
 *
 * These arrive over the portal as shell patterns, MIME types, or both, and the two forms are
 * not interchangeable — an application sends whichever it has. What matters here is that the
 * patterns are *fnmatch*, not a simplified wildcard syntax: Chromium in particular writes an
 * extension as a bracket expression per letter, so a JPEG filter reaches us as
 * `*.[jJ][pP][eE][gG]` while the name shown in the dropdown is the friendly `*.jpeg`. A
 * translation that does not understand brackets turns that into a pattern matching a file
 * literally called `something.[jJ][pP][eE][gG]`, which is to say nothing at all, and the
 * dialog presents an empty folder to someone who can see the file is right there.
 */
export interface PickerTypeFilter {
  name: string;
  globs: string[];
  mimes: string[];
}

export interface PickerTypeFilterMatchers {
  globs: RegExp[];
  mimes: string[];
}

/**
 * Patterns that admit every named file. Chromium's "All Files" entry is `*.*`, which under
 * fnmatch would still hide a file with no dot in its name — not what anyone choosing All Files
 * is asking for.
 */
const UNIVERSAL_GLOBS = new Set(['*', '*.*']);

/**
 * Reads one fnmatch bracket expression, starting at its `[`.
 *
 * Returns `null` when what follows is not one — an unclosed `[` is an ordinary character, as
 * it is in a shell. The contents carry over to a regular expression class almost unchanged:
 * ranges (`a-z`) mean the same thing in both, so they are left alone, and only the characters
 * that would end or re-interpret the class are escaped.
 */
function readBracketExpression(
  glob: string,
  startIndex: number,
): {
  pattern: string;
  nextIndex: number;
} | null {
  let index = startIndex + 1;
  let isNegated = false;

  if (glob[index] === '!' || glob[index] === '^') {
    isNegated = true;
    index += 1;
  }

  let body = '';

  // A `]` in first position is one of the listed characters rather than the terminator.
  if (glob[index] === ']') {
    body += '\\]';
    index += 1;
  }

  while (index < glob.length && glob[index] !== ']') {
    body += glob[index] === '\\' ? '\\\\' : glob[index];
    index += 1;
  }

  if (index >= glob.length || !body) {
    return null;
  }

  return {
    pattern: `[${isNegated ? '^' : ''}${body}]`,
    nextIndex: index + 1,
  };
}

function escapeRegExpCharacter(character: string): string {
  return /[.+^${}()|[\]\\]/.test(character) ? `\\${character}` : character;
}

/**
 * Case-insensitive on purpose, unlike a strict fnmatch: an application asking for `*.jpg`
 * means the photos, not the photos whose extension happens to be lowercase. This also makes
 * Chromium's per-letter case folding redundant rather than harmful.
 */
export function globToRegExp(glob: string): RegExp {
  let source = '';
  let index = 0;

  while (index < glob.length) {
    const character = glob[index];

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
      const bracket = readBracketExpression(glob, index);

      if (bracket) {
        source += bracket.pattern;
        index = bracket.nextIndex;
        continue;
      }
    }

    source += escapeRegExpCharacter(character);
    index += 1;
  }

  return new RegExp(`^${source}$`, 'i');
}

/**
 * `null` means "no narrowing" — for a filter that was not found, one that carries no patterns
 * at all, or one that admits everything anyway. Each of those is a filter that should leave
 * the listing alone, and answering with an empty matcher set instead would blank it.
 */
export function createTypeFilterMatchers(
  filter: PickerTypeFilter | undefined,
): PickerTypeFilterMatchers | null {
  if (!filter) {
    return null;
  }

  // The patterns are a union, so one that admits everything settles it for the whole filter,
  // whatever else it lists alongside.
  const admitsEverything = filter.globs.some(glob => UNIVERSAL_GLOBS.has(glob));

  if (admitsEverything || (filter.globs.length === 0 && filter.mimes.length === 0)) {
    return null;
  }

  return {
    globs: filter.globs.map(globToRegExp),
    mimes: filter.mimes.map(pattern => pattern.toLowerCase()),
  };
}

export function entryPassesTypeFilter(
  entry: DirEntry,
  matchers: PickerTypeFilterMatchers | null,
): boolean {
  if (!matchers) {
    return true;
  }

  if (matchers.globs.some(glob => glob.test(entry.name))) {
    return true;
  }

  const mime = entry.mime?.toLowerCase();

  if (!mime) {
    return false;
  }

  return matchers.mimes.some((pattern) => {
    if (pattern === '*' || pattern === '*/*') {
      return true;
    }

    if (pattern.endsWith('/*')) {
      return mime.startsWith(pattern.slice(0, -1));
    }

    return mime === pattern;
  });
}
