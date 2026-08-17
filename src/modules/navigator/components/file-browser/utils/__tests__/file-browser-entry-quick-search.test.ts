// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import {
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import type { DirEntry } from '@/types/dir-entry';
import type { DirSizesStore } from '../file-browser-sort';
import {
  compileFileBrowserQuickSearchPattern,
  createFileBrowserQuickSearchCache,
  createFileBrowserQuickSearchMatcher,
  fileBrowserEntryMatchesQuickSearch,
} from '../file-browser-entry-quick-search';

vi.mock('@/stores/storage/user-settings', () => ({
  useUserSettingsStore: () => ({
    userSettings: {
      dateTime: {
        month: 'short',
        regionalFormat: {
          code: 'en-US',
          name: 'United States',
        },
        autoDetectRegionalFormat: false,
        hour12: true,
        showRelativeDates: true,
        properties: {
          showSeconds: false,
          showMilliseconds: false,
        },
      },
    },
  }),
}));

function createMockDirSizesStore(overrides?: { getSize?: DirSizesStore['getSize'] }): DirSizesStore {
  return {
    sizes: new Map(),
    getSize: overrides?.getSize ?? (() => undefined),
  } as DirSizesStore;
}

const MB = 1024 ** 2;

function createFileEntry(overrides: Partial<DirEntry> = {}): DirEntry {
  return {
    name: 'readme',
    ext: null,
    path: 'C:/docs/readme',
    size: 0,
    item_count: null,
    modified_time: 0,
    accessed_time: 0,
    created_time: 0,
    mime: null,
    is_file: true,
    is_dir: false,
    is_symlink: false,
    is_hidden: false,
    ...overrides,
  };
}

describe('fileBrowserEntryMatchesQuickSearch', () => {
  it('matches by filename substring', () => {
    const entry = createFileEntry({ name: 'annual-report.pdf' });
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'report', createMockDirSizesStore())).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'missing', createMockDirSizesStore())).toBe(false);
  });

  it('matches by formatted size only, not raw bytes', () => {
    const entry = createFileEntry({ size: 1024 });
    const store = createMockDirSizesStore();
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'kb', store)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, '1.0', store)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, '1024', store)).toBe(false);
  });

  it('matches by extension and mime', () => {
    const entry = createFileEntry({
      name: 'x',
      ext: 'pdf',
      path: 'C:/x.pdf',
      mime: 'application/pdf',
    });
    const store = createMockDirSizesStore();
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'pdf', store)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'application', store)).toBe(true);
  });

  it('matches by item count and localized label', () => {
    const entry = createFileEntry({
      name: 'folder',
      path: 'C:/folder',
      is_file: false,
      is_dir: true,
      item_count: 12,
    });
    const store = createMockDirSizesStore();
    expect(fileBrowserEntryMatchesQuickSearch(entry, '12', store)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'items', store)).toBe(true);
  });

  it('does not match by path segment without a property prefix', () => {
    const entry = createFileEntry({ path: 'C:/Users/projects/demo.txt' });
    const store = createMockDirSizesStore();
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'projects', store)).toBe(false);
  });

  it('matches by formatted date only, not raw timestamp', () => {
    const modifiedTime = Date.UTC(2024, 0, 15, 12, 0, 0);
    const entry = createFileEntry({ modified_time: modifiedTime });
    const store = createMockDirSizesStore();
    expect(fileBrowserEntryMatchesQuickSearch(entry, String(modifiedTime), store)).toBe(false);
    expect(fileBrowserEntryMatchesQuickSearch(entry, '2024', store)).toBe(true);
  });

  it('matches size property with comparison and range predicates', () => {
    const store = createMockDirSizesStore();
    const largeFile = createFileEntry({ size: 3 * MB });
    expect(fileBrowserEntryMatchesQuickSearch(largeFile, 'size: >=2', store)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(largeFile, 'size: <1mb', store)).toBe(false);
    expect(fileBrowserEntryMatchesQuickSearch(largeFile, 'size: 2mb..4mb', store)).toBe(true);
    const smallFile = createFileEntry({ size: 400 * 1024 });
    expect(fileBrowserEntryMatchesQuickSearch(smallFile, 'size: <=500kb', store)).toBe(true);
  });

  it('matches items property with comparison and range predicates', () => {
    const store = createMockDirSizesStore();
    const folder = createFileEntry({
      name: 'd',
      path: 'C:/d',
      is_file: false,
      is_dir: true,
      item_count: 8,
    });
    expect(fileBrowserEntryMatchesQuickSearch(folder, 'items: >=5', store)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(folder, 'items: 3..10', store)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(folder, 'items: ==12', store)).toBe(false);
    const fileEntry = createFileEntry({
      name: 'f',
      path: 'C:/f',
      item_count: null,
    });
    expect(fileBrowserEntryMatchesQuickSearch(fileEntry, 'items: >=0', store)).toBe(false);
  });

  it('property prefix limits search to that field', () => {
    const entry = createFileEntry({
      name: 'photo.jpg',
      path: 'C:/pics/photo.jpg',
      size: 1024,
    });
    const store = createMockDirSizesStore();
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'path: pics', store)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'name: pics', store)).toBe(false);
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'size: kb', store)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'size: photo', store)).toBe(false);
  });

  it('uses directory size cache when present', () => {
    const entry = createFileEntry({
      name: 'big',
      path: 'D:/big',
      is_file: false,
      is_dir: true,
    });
    const store = createMockDirSizesStore({
      getSize: (path: string) => {
        if (path === 'D:/big') {
          return {
            size: 5_000_000,
            status: 'Complete',
            fileCount: 3,
            dirCount: 1,
            calculatedAt: Date.now(),
          };
        }

        return undefined;
      },
    });
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'mb', store)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, '5000000', store)).toBe(false);
    expect(fileBrowserEntryMatchesQuickSearch(entry, '3', store)).toBe(true);
  });

  it('invalidates cached haystacks when directory size info changes', () => {
    const entry = createFileEntry({
      name: 'cached-dir',
      path: 'D:/cached-dir',
      is_file: false,
      is_dir: true,
    });
    let fileCount = 3;
    const store = createMockDirSizesStore({
      getSize: () => ({
        size: 5_000_000,
        status: 'Complete',
        fileCount,
        dirCount: 1,
        calculatedAt: fileCount,
      }),
    });
    const cache = createFileBrowserQuickSearchCache();

    expect(createFileBrowserQuickSearchMatcher('7', store, cache)(entry)).toBe(false);
    fileCount = 7;
    expect(createFileBrowserQuickSearchMatcher('7', store, cache)(entry)).toBe(true);
  });

  it('invalidates cached relative modified labels when the label changes', () => {
    vi.useFakeTimers();

    try {
      const referenceNowMs = Date.UTC(2024, 0, 1, 12, 0, 10);
      vi.setSystemTime(referenceNowMs);

      const entry = createFileEntry({
        modified_time: referenceNowMs - 5000,
      });
      const store = createMockDirSizesStore();
      const cache = createFileBrowserQuickSearchCache();

      expect(createFileBrowserQuickSearchMatcher('just now', store, cache)(entry)).toBe(true);

      // Still inside the first minute: the label has not moved, so neither has the cache.
      vi.setSystemTime(referenceNowMs + 1000);

      expect(createFileBrowserQuickSearchMatcher('just now', store, cache)(entry)).toBe(true);

      // A minute has passed and the label now counts minutes, which must reach the cache.
      vi.setSystemTime(referenceNowMs + 60_000);

      expect(createFileBrowserQuickSearchMatcher('1 min', store, cache)(entry)).toBe(true);
      expect(createFileBrowserQuickSearchMatcher('just now', store, cache)(entry)).toBe(false);
    }
    finally {
      vi.useRealTimers();
    }
  });
});

describe('quick search regular expressions', () => {
  const regexOptions = { regex: true };

  it('matches a pattern against the name instead of a literal substring', () => {
    const entry = createFileEntry({
      name: 'report-2024.pdf',
      ext: 'pdf',
    });
    const store = createMockDirSizesStore();

    expect(fileBrowserEntryMatchesQuickSearch(entry, String.raw`report-\d{4}`, store, regexOptions)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, String.raw`report-\d{2}\.pdf`, store, regexOptions)).toBe(false);
  });

  it('honours anchors against the whole name', () => {
    const entry = createFileEntry({
      name: 'annual-report.pdf',
      ext: 'pdf',
    });
    const store = createMockDirSizesStore();

    expect(fileBrowserEntryMatchesQuickSearch(entry, String.raw`\.pdf$`, store, regexOptions)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, '^annual', store, regexOptions)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, '^report', store, regexOptions)).toBe(false);
  });

  it('anchors against one searchable value at a time, not the values run together', () => {
    const entry = createFileEntry({
      name: 'photo.png',
      ext: 'png',
      mime: 'image/png',
    });
    const store = createMockDirSizesStore();

    expect(fileBrowserEntryMatchesQuickSearch(entry, '^image/png$', store, regexOptions)).toBe(true);
    // `.` must not cross from one value into the next.
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'photo.png.image', store, regexOptions)).toBe(false);
  });

  it('ignores case the way the literal search does', () => {
    const entry = createFileEntry({ name: 'README.md' });
    const store = createMockDirSizesStore();

    expect(fileBrowserEntryMatchesQuickSearch(entry, '^readme', store, regexOptions)).toBe(true);
  });

  it('applies a pattern inside a property query', () => {
    const entry = createFileEntry({
      name: 'photo.png',
      path: 'C:/pics/2024/photo.png',
    });
    const store = createMockDirSizesStore();

    expect(fileBrowserEntryMatchesQuickSearch(entry, String.raw`path: /\d{4}/`, store, regexOptions)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, String.raw`path: /\d{5}/`, store, regexOptions)).toBe(false);
  });

  it('still resolves numeric property predicates', () => {
    const entry = createFileEntry({ size: 3 * MB });
    const store = createMockDirSizesStore();

    expect(fileBrowserEntryMatchesQuickSearch(entry, 'size: >=2mb', store, regexOptions)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'size: <1mb', store, regexOptions)).toBe(false);
  });

  it('accepts a shell wildcard, which is what most people type first', () => {
    const entry = createFileEntry({
      name: 'photo.png',
      ext: 'png',
    });
    const store = createMockDirSizesStore();

    // `*.png` is not valid regex syntax, so without wildcard handling this would not
    // compile and would match nothing at all.
    expect(fileBrowserEntryMatchesQuickSearch(entry, '*.png', store, regexOptions)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, '*.jpg', store, regexOptions)).toBe(false);
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'photo.*', store, regexOptions)).toBe(true);
  });

  it('anchors a wildcard to the whole name', () => {
    const entry = createFileEntry({
      name: 'photo.png.txt',
      ext: 'txt',
    });
    const store = createMockDirSizesStore();

    expect(fileBrowserEntryMatchesQuickSearch(entry, '*.png', store, regexOptions)).toBe(false);
    expect(fileBrowserEntryMatchesQuickSearch(entry, '*.png.*', store, regexOptions)).toBe(true);
  });

  it('matches nothing while the pattern is unfinished', () => {
    const entry = createFileEntry({ name: 'anything.txt' });
    const store = createMockDirSizesStore();

    expect(fileBrowserEntryMatchesQuickSearch(entry, '^(unclosed', store, regexOptions)).toBe(false);
  });

  it('treats the query literally when the pattern option is off', () => {
    const entry = createFileEntry({ name: 'report-2024.pdf' });
    const literal = createFileEntry({ name: String.raw`report-\d{4}.pdf` });
    const store = createMockDirSizesStore();

    expect(fileBrowserEntryMatchesQuickSearch(entry, String.raw`report-\d{4}`, store)).toBe(false);
    expect(fileBrowserEntryMatchesQuickSearch(literal, String.raw`report-\d{4}`, store)).toBe(true);
  });

  it('reports why a pattern cannot be compiled', () => {
    expect(compileFileBrowserQuickSearchPattern('^report').error).toBeNull();
    expect(compileFileBrowserQuickSearchPattern('^(unclosed').error).not.toBeNull();
    // The property prefix is not part of the pattern.
    expect(compileFileBrowserQuickSearchPattern('path: ^(unclosed').error).not.toBeNull();
    expect(compileFileBrowserQuickSearchPattern('path: ^ok').error).toBeNull();
  });
});

describe('quick search wildcards without the pattern toggle', () => {
  it('reads * and ? as wildcards even when the toggle is off', () => {
    const entry = createFileEntry({
      name: 'photo.png',
      ext: 'png',
    });
    const store = createMockDirSizesStore();

    expect(fileBrowserEntryMatchesQuickSearch(entry, '*.png', store)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, '*.jpg', store)).toBe(false);
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'ph?to.png', store)).toBe(true);
  });

  it('anchors a wildcard to the whole value with the toggle off', () => {
    const entry = createFileEntry({
      name: 'photo.png.txt',
      ext: 'txt',
    });
    const store = createMockDirSizesStore();

    expect(fileBrowserEntryMatchesQuickSearch(entry, '*.png', store)).toBe(false);
    // The plain substring search is unchanged, so this still matches.
    expect(fileBrowserEntryMatchesQuickSearch(entry, '.png', store)).toBe(true);
  });

  it('leaves a query without wildcards as a substring search', () => {
    const entry = createFileEntry({ name: 'annual-report.pdf' });
    const store = createMockDirSizesStore();

    expect(fileBrowserEntryMatchesQuickSearch(entry, 'report', store)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'ann', store)).toBe(true);
    // A regex metacharacter is a literal here, which a substring search gets right for free.
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'annual.report', store)).toBe(false);
  });

  it('applies a wildcard inside a property query', () => {
    const entry = createFileEntry({
      name: 'photo.png',
      path: 'C:/pics/2024/photo.png',
    });
    const store = createMockDirSizesStore();

    expect(fileBrowserEntryMatchesQuickSearch(entry, 'path: *2024*', store)).toBe(true);
    expect(fileBrowserEntryMatchesQuickSearch(entry, 'path: *2025*', store)).toBe(false);
  });
});
