// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { FILE_EXTENSIONS } from '@/constants';
import type { DirEntry } from '@/types/dir-entry';
import type { GroupedEntries } from './types';

export function isFileBrowserImageEntry(entry: DirEntry): boolean {
  if (entry.is_dir) return false;
  const extension = entry.ext?.toLowerCase();
  return extension ? FILE_EXTENSIONS.IMAGE.includes(extension) : false;
}

export function isFileBrowserVideoEntry(entry: DirEntry): boolean {
  if (entry.is_dir) return false;
  const extension = entry.ext?.toLowerCase();
  return extension ? FILE_EXTENSIONS.VIDEO.includes(extension) : false;
}

export function isFileBrowserAudioEntry(entry: DirEntry): boolean {
  if (entry.is_dir) return false;
  const extension = entry.ext?.toLowerCase();
  return extension ? FILE_EXTENSIONS.AUDIO.includes(extension) : false;
}

export function groupFileBrowserEntries(entries: readonly DirEntry[]): GroupedEntries {
  const dirs: DirEntry[] = [];
  const images: DirEntry[] = [];
  const videos: DirEntry[] = [];
  const others: DirEntry[] = [];

  for (const entry of entries) {
    if (entry.is_dir) {
      dirs.push(entry);
    }
    else if (isFileBrowserImageEntry(entry)) {
      images.push(entry);
    }
    else if (isFileBrowserVideoEntry(entry)) {
      videos.push(entry);
    }
    else {
      others.push(entry);
    }
  }

  return {
    dirs,
    images,
    videos,
    others,
  };
}

export function getFileBrowserGridEntryOrder(entries: readonly DirEntry[]): DirEntry[] {
  const groupedEntries = groupFileBrowserEntries(entries);
  return [
    ...groupedEntries.dirs,
    ...groupedEntries.images,
    ...groupedEntries.videos,
    ...groupedEntries.others,
  ];
}

/** One directory's worth of results, as shown under a heading of its own. */
export interface FileBrowserFolderGroup {
  /** The directory the entries came out of, in full. */
  key: string;
  /** That directory relative to the searched one; empty for the searched directory. */
  label: string;
  entries: DirEntry[];
}

/**
 * Results from a subtree search arrive as a flat list of paths from all over the tree, so
 * they are gathered back under the directory each one lives in. Grouping is by path rather
 * than by kind: a search answers "where is it", and the folder is the answer.
 */
export function groupFileBrowserEntriesByFolder(
  entries: readonly DirEntry[],
  basePath: string,
): FileBrowserFolderGroup[] {
  const groups = new Map<string, FileBrowserFolderGroup>();
  const normalizedBase = basePath.replace(/\/+$/, '');

  for (const entry of entries) {
    const separatorIndex = entry.path.lastIndexOf('/');
    const parentPath = separatorIndex > 0 ? entry.path.slice(0, separatorIndex) : entry.path;
    let group = groups.get(parentPath);

    if (!group) {
      group = {
        key: parentPath,
        label: getFolderGroupLabel(parentPath, normalizedBase),
        entries: [],
      };
      groups.set(parentPath, group);
    }

    group.entries.push(entry);
  }

  // The searched directory leads, then the subdirectories in path order, so the same search
  // always reads the same way regardless of which order the walk happened to return.
  return Array.from(groups.values()).sort((groupA, groupB) => {
    if (groupA.label === groupB.label) return 0;
    if (groupA.label === '') return -1;
    if (groupB.label === '') return 1;
    return groupA.label.localeCompare(groupB.label);
  });
}

function getFolderGroupLabel(parentPath: string, normalizedBase: string): string {
  if (!normalizedBase || parentPath === normalizedBase) {
    return parentPath === normalizedBase ? '' : parentPath;
  }

  return parentPath.startsWith(`${normalizedBase}/`)
    ? parentPath.slice(normalizedBase.length + 1)
    : parentPath;
}

export interface FileBrowserFolderGrouping {
  /** The directory the search started from; group labels are relative to it. */
  basePath: string;
}

export function getFileBrowserVisualEntryOrder(
  entries: readonly DirEntry[],
  layout: 'list' | 'grid' | undefined,
  folderGrouping?: FileBrowserFolderGrouping | null,
): DirEntry[] {
  if (folderGrouping) {
    return groupFileBrowserEntriesByFolder(entries, folderGrouping.basePath)
      .flatMap(group => layout === 'grid' ? getFileBrowserGridEntryOrder(group.entries) : group.entries);
  }

  if (layout === 'grid') {
    return getFileBrowserGridEntryOrder(entries);
  }

  return [...entries];
}
