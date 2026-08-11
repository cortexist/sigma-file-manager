// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { describe, expect, it } from 'vitest';
import {
  createTypeFilterMatchers,
  entryPassesTypeFilter,
  globToRegExp,
  type PickerTypeFilter,
} from '@/modules/file-picker/utils/picker-type-filter';
import type { DirEntry } from '@/types/dir-entry';

function file(name: string, mime: string | null = null): DirEntry {
  return {
    name,
    ext: name.split('.').pop() ?? null,
    path: `/home/zero/Downloads/${name}`,
    size: 1,
    item_count: null,
    modified_time: 1,
    accessed_time: 1,
    created_time: 1,
    mime,
    is_file: true,
    is_dir: false,
    is_symlink: false,
    is_hidden: false,
  };
}

function filter(overrides: Partial<PickerTypeFilter> = {}): PickerTypeFilter {
  return {
    name: 'Filter',
    globs: [],
    mimes: [],
    ...overrides,
  };
}

describe('globToRegExp', () => {
  it('matches a plain extension pattern', () => {
    expect(globToRegExp('*.jpeg').test('asimov.jpeg')).toBe(true);
    expect(globToRegExp('*.jpeg').test('asimov.png')).toBe(false);
  });

  /**
   * The regression this guards. Chromium writes each letter of an extension as a bracket
   * expression so that fnmatch matches either case, and sends *that* as the pattern while the
   * dropdown label stays the friendly `*.jpeg`. Escaping the brackets produced a pattern that
   * could only match a file literally named `something.[jJ][pP][eE][gG]`, so a Save dialog
   * from a browser showed an empty folder.
   */
  it('understands the per-letter bracket form a browser actually sends', () => {
    const jpeg = globToRegExp('*.[jJ][pP][eE][gG]');

    expect(jpeg.test('asimov.jpeg')).toBe(true);
    expect(jpeg.test('HPanDPAbMAA7LfB.jpeg')).toBe(true);
    expect(jpeg.test('SHOUTING.JPEG')).toBe(true);
    expect(jpeg.test('clipboard-image.png')).toBe(false);
  });

  it('handles ranges, negation and the literal-bracket cases', () => {
    expect(globToRegExp('file[0-9].txt').test('file7.txt')).toBe(true);
    expect(globToRegExp('file[0-9].txt').test('filex.txt')).toBe(false);

    expect(globToRegExp('*.[!oO]*').test('notes.txt')).toBe(true);
    expect(globToRegExp('*.[!oO]*').test('archive.odt')).toBe(false);

    // An unclosed bracket is an ordinary character, as it is in a shell.
    expect(globToRegExp('weird[name.txt').test('weird[name.txt')).toBe(true);
  });

  it('matches a single character for ?', () => {
    expect(globToRegExp('IMG_?.png').test('IMG_4.png')).toBe(true);
    expect(globToRegExp('IMG_?.png').test('IMG_42.png')).toBe(false);
  });

  it('treats regex punctuation in a name as literal text', () => {
    expect(globToRegExp('report (final).pdf').test('report (final).pdf')).toBe(true);
    expect(globToRegExp('a+b.txt').test('aab.txt')).toBe(false);
  });
});

describe('createTypeFilterMatchers', () => {
  it('narrows nothing when the filter admits everything', () => {
    // Chromium's "All Files" entry. Under a strict fnmatch `*.*` would still hide a file with
    // no dot in its name, which is not what choosing All Files asks for.
    expect(createTypeFilterMatchers(filter({ globs: ['*.*'] }))).toBeNull();
    expect(createTypeFilterMatchers(filter({ globs: ['*'] }))).toBeNull();
    expect(createTypeFilterMatchers(filter())).toBeNull();
    expect(createTypeFilterMatchers(undefined)).toBeNull();
  });

  it('keeps a real pattern', () => {
    const matchers = createTypeFilterMatchers(filter({ globs: ['*.png'] }));

    expect(matchers?.globs).toHaveLength(1);
  });

  /** The patterns are a union: one that admits everything decides the whole filter. */
  it('narrows nothing when an everything pattern sits beside a narrower one', () => {
    expect(createTypeFilterMatchers(filter({ globs: ['*.*', '*.png'] }))).toBeNull();
    expect(createTypeFilterMatchers(filter({
      globs: ['*'],
      mimes: ['image/*'],
    }))).toBeNull();
  });
});

describe('entryPassesTypeFilter', () => {
  it('admits everything when there is no filter in force', () => {
    expect(entryPassesTypeFilter(file('anything'), null)).toBe(true);
  });

  it('admits a file a browser save dialog asked for', () => {
    const matchers = createTypeFilterMatchers(filter({ globs: ['*.[jJ][pP][eE][gG]'] }));

    expect(entryPassesTypeFilter(file('asimov.jpeg', 'image/jpeg'), matchers)).toBe(true);
    expect(entryPassesTypeFilter(file('clipboard-image.png', 'image/png'), matchers)).toBe(false);
  });

  /** Applications that send MIME types instead of patterns are just as valid a caller. */
  it('admits a file by MIME type, including a wildcard subtype', () => {
    const anyImage = createTypeFilterMatchers(filter({ mimes: ['image/*'] }));

    expect(entryPassesTypeFilter(file('asimov.jpeg', 'image/jpeg'), anyImage)).toBe(true);
    expect(entryPassesTypeFilter(file('song.mp3', 'audio/mpeg'), anyImage)).toBe(false);

    // Nothing to compare against: a file with no known type cannot satisfy a MIME filter.
    expect(entryPassesTypeFilter(file('README', null), anyImage)).toBe(false);
  });
});
