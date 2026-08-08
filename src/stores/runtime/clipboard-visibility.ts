// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import type { DirEntry } from '@/types/dir-entry';
import type { ClipboardSettings } from '@/types/user-settings';
import { arePathsEquivalent } from '@/utils/file-operation-paths';

export type ClipboardOrigin = 'internal' | 'external' | '';
type FileClipboardOperationType = 'copy' | 'move' | '';

/**
 * Set comparison of the paths alone, ignoring copy-vs-move.
 *
 * Needed on its own because not every platform round-trips the operation: a Linux system
 * clipboard carries only a file list, so the paths are the only part of our own clipboard
 * that can be recognized when reading it back.
 */
export function hasSameFileClipboardPaths(
  localItems: DirEntry[],
  systemPaths: string[],
): boolean {
  if (localItems.length !== systemPaths.length) {
    return false;
  }

  const unmatchedSystemPaths = [...systemPaths];

  for (const localItem of localItems) {
    const matchingPathIndex = unmatchedSystemPaths.findIndex(systemPath =>
      arePathsEquivalent(localItem.path, systemPath),
    );

    if (matchingPathIndex === -1) {
      return false;
    }

    unmatchedSystemPaths.splice(matchingPathIndex, 1);
  }

  return unmatchedSystemPaths.length === 0;
}

export function hasSameFileClipboardContent(
  localItems: DirEntry[],
  localType: FileClipboardOperationType,
  systemPaths: string[],
  systemOperation: 'copy' | 'move',
): boolean {
  if (localType !== systemOperation) {
    return false;
  }

  return hasSameFileClipboardPaths(localItems, systemPaths);
}

export function shouldShowClipboardUi(options: {
  hasItems: boolean;
  origin: ClipboardOrigin;
  hasImageContent: boolean;
  hasFileItems: boolean;
  settings: ClipboardSettings;
}): boolean {
  if (!options.hasItems) {
    return false;
  }

  if (options.origin === 'internal') {
    return true;
  }

  if (options.hasImageContent) {
    return options.settings.showToolbarForExternalImages;
  }

  if (options.hasFileItems) {
    return options.settings.showToolbarForExternalPaths;
  }

  return false;
}
