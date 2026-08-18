// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { describe, expect, it } from 'vitest';
import type { DirEntry } from '@/types/dir-entry';
import { createFileBrowserVirtualRows } from '../use-file-browser-virtual-layout';

const BASE_PATH = 'C:/project';

function createEntry(name: string, parentPath: string, overrides: Partial<DirEntry> = {}): DirEntry {
  const isDirectory = overrides.is_dir ?? false;

  return {
    name,
    ext: null,
    path: `${parentPath}/${name}`,
    size: 0,
    item_count: null,
    modified_time: 0,
    accessed_time: 0,
    created_time: 0,
    mime: null,
    is_file: !isDirectory,
    is_dir: isDirectory,
    is_symlink: false,
    is_hidden: false,
    ...overrides,
  };
}

function createSubtreeEntries(): DirEntry[] {
  return [
    createEntry('readme.md', BASE_PATH),
    createEntry('lib.rs', `${BASE_PATH}/src`),
    createEntry('main.rs', `${BASE_PATH}/src`),
  ];
}

function rowEnds(rows: readonly {
  start: number;
  size: number;
}[]): number[] {
  return rows.map(row => row.start + row.size);
}

describe('file browser virtual rows', () => {
  it('lays a plain listing out as one row per entry', () => {
    const rows = createFileBrowserVirtualRows({
      entries: createSubtreeEntries(),
      layout: 'list',
      viewportWidth: 1200,
    });

    expect(rows.map(row => row.type)).toEqual(['list-entry', 'list-entry', 'list-entry']);
  });

  it('puts a heading above each folder of a subtree search', () => {
    const rows = createFileBrowserVirtualRows({
      entries: createSubtreeEntries(),
      layout: 'list',
      viewportWidth: 1200,
      folderGrouping: { basePath: BASE_PATH },
    });

    expect(rows.map(row => row.type)).toEqual([
      'list-section',
      'list-entry',
      'list-section',
      'list-entry',
      'list-entry',
    ]);

    const [firstSection, , secondSection] = rows;
    expect(firstSection.type === 'list-section' && firstSection.label).toBe('');
    expect(secondSection.type === 'list-section' && secondSection.label).toBe('src');
    expect(secondSection.type === 'list-section' && secondSection.count).toBe(2);
  });

  it('leaves no gap or overlap between grouped rows', () => {
    const rows = createFileBrowserVirtualRows({
      entries: createSubtreeEntries(),
      layout: 'list',
      viewportWidth: 1200,
      folderGrouping: { basePath: BASE_PATH },
    });

    // Every row has to start exactly where the previous one ended, or the virtual list
    // scrolls to the wrong offsets.
    expect(rows.map(row => row.start)).toEqual([0, ...rowEnds(rows).slice(0, -1)]);
  });

  it('numbers grouped entries in the order they are shown', () => {
    const rows = createFileBrowserVirtualRows({
      entries: createSubtreeEntries(),
      layout: 'list',
      viewportWidth: 1200,
      folderGrouping: { basePath: BASE_PATH },
    });

    const entryRows = rows.filter(row => row.type === 'list-entry');

    expect(entryRows.map(row => row.type === 'list-entry' && row.entryIndex)).toEqual([0, 1, 2]);
  });

  it('groups grid cards by folder instead of by kind', () => {
    const entries = [
      ...createSubtreeEntries(),
      createEntry('src', BASE_PATH, {
        is_dir: true,
        is_file: false,
      }),
    ];

    const rows = createFileBrowserVirtualRows({
      entries,
      layout: 'grid',
      viewportWidth: 1200,
      folderGrouping: { basePath: BASE_PATH },
    });

    const sectionKeys = rows
      .filter(row => row.type === 'grid-section')
      .map(row => row.type === 'grid-section' ? row.sectionKey : '');

    expect(sectionKeys).toEqual([`folder:${BASE_PATH}`, `folder:${BASE_PATH}/src`]);
    expect(rows.map(row => row.start)).toEqual([0, ...rowEnds(rows).slice(0, -1)]);
  });

  it('keeps directory cards on rows of their own so a row has one height', () => {
    const entries = [
      createEntry('src', BASE_PATH, {
        is_dir: true,
        is_file: false,
      }),
      createEntry('readme.md', BASE_PATH),
    ];

    const rows = createFileBrowserVirtualRows({
      entries,
      layout: 'grid',
      viewportWidth: 1200,
      folderGrouping: { basePath: BASE_PATH },
    });

    const itemRows = rows.filter(row => row.type === 'grid-items');

    expect(itemRows.map(row => row.type === 'grid-items' && row.variant)).toEqual(['dir', 'other']);
  });
});
