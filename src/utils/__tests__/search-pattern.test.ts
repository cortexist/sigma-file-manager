// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { describe, expect, it } from 'vitest';
import {
  compileSearchPattern,
  looksLikeWildcard,
  wildcardToRegExpSource,
} from '../search-pattern';

// These cases are kept in step with the Rust ones in `src-tauri/src/search_pattern.rs`:
// the same query has to mean the same thing in both searches.
function matches(query: string, name: string): boolean {
  const { pattern } = compileSearchPattern(query, 'i');
  return pattern !== null && pattern.test(name);
}

describe('search pattern', () => {
  it('reads a wildcard query as a wildcard', () => {
    expect(looksLikeWildcard('*.png')).toBe(true);
    expect(looksLikeWildcard('report?.txt')).toBe(true);
    expect(looksLikeWildcard('report')).toBe(false);
  });

  it('leaves a query holding regex syntax as a regular expression', () => {
    expect(looksLikeWildcard(String.raw`^.*\.png$`)).toBe(false);
    expect(looksLikeWildcard('(png|jpg)*')).toBe(false);
    expect(looksLikeWildcard(String.raw`\d?`)).toBe(false);
  });

  it('matches a wildcard against the whole name', () => {
    expect(matches('*.png', 'photo.png')).toBe(true);
    expect(matches('*.png', 'PHOTO.PNG')).toBe(true);
    expect(matches('*.png', 'photo.png.txt')).toBe(false);
    expect(matches('*.png', 'photo.jpg')).toBe(false);
  });

  it('treats a wildcard question mark as one character', () => {
    expect(matches('report?.txt', 'report1.txt')).toBe(true);
    expect(matches('report?.txt', 'report.txt')).toBe(false);
    expect(matches('report?.txt', 'report12.txt')).toBe(false);
  });

  it('treats a dot in a wildcard as a literal dot', () => {
    expect(matches('a*.png', 'axpng')).toBe(false);
    expect(matches('a*.png', 'ax.png')).toBe(true);
    expect(matches('a?png', 'axpng')).toBe(true);
  });

  it('keeps the meaning of a wildcard character class', () => {
    expect(matches('[0-9]*.png', '1photo.png')).toBe(true);
    expect(matches('[0-9]*.png', 'photo.png')).toBe(false);
    expect(matches('[!0-9]*.png', 'photo.png')).toBe(true);
    expect(matches('[!0-9]*.png', '1photo.png')).toBe(false);
  });

  it('treats an unterminated class as a literal bracket', () => {
    expect(matches('[draft*', '[draft-1.txt')).toBe(true);
  });

  it('still runs a regular expression when the query holds regex syntax', () => {
    expect(matches(String.raw`\.png$`, 'photo.png')).toBe(true);
    expect(matches('^photo', 'photo.png')).toBe(true);
    // Unanchored, unlike a wildcard.
    expect(matches(String.raw`\.png`, 'photo.png.txt')).toBe(true);
  });

  it('anchors a wildcard translation at both ends', () => {
    expect(wildcardToRegExpSource('*.png')).toBe(String.raw`^.*\.png$`);
  });

  it('reports a pattern that cannot be compiled', () => {
    expect(compileSearchPattern('^(unclosed', 'i').error).not.toBeNull();
    expect(compileSearchPattern('*.png', 'i').error).toBeNull();
    expect(compileSearchPattern('', 'i').pattern).toBeNull();
  });
});
