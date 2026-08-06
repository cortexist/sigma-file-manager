// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { ref } from 'vue';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { getParentDirectory } from '@/utils/normalize-path';
import type { DirContents, DirEntry } from '@/types/dir-entry';

/**
 * Names a cover file next to the audio is conventionally given, in the order they are
 * preferred. Matched case-insensitively against the stem, so `Folder.jpg` counts.
 */
const SIBLING_COVER_STEMS = ['cover', 'folder', 'album', 'albumart', 'artist'];
const SIBLING_COVER_EXTENSIONS = ['jpg', 'jpeg', 'png', 'webp'];

function coverCacheKey(entry: DirEntry): string {
  return `${entry.path}|${entry.modified_time}|${entry.size}`;
}

/**
 * Resolves artwork for audio files: the picture embedded in the file first, then a cover
 * image sitting beside it. Both answers are cached, including the negative ones, so a track
 * without artwork is not re-examined every time it scrolls past.
 */
export function useAudioCovers() {
  const embeddedCovers = ref<Record<string, string>>({});
  const siblingCovers = ref<Record<string, string>>({});

  const withoutEmbeddedCover = new Set<string>();
  const withoutSiblingCover = new Set<string>();
  const embeddedInFlight = new Set<string>();
  const siblingInFlight = new Set<string>();

  /**
   * Returns the artwork URL when it is already known, and otherwise starts extracting it and
   * returns `undefined`. Callers re-read this reactively once the value lands.
   */
  function getEmbeddedCover(entry: DirEntry): string | undefined {
    const key = coverCacheKey(entry);
    const cached = embeddedCovers.value[key];

    if (cached || withoutEmbeddedCover.has(key) || embeddedInFlight.has(key)) {
      return cached;
    }

    embeddedInFlight.add(key);

    void invoke<string | null>('extract_audio_cover', {
      path: entry.path,
      modifiedTime: entry.modified_time,
      size: entry.size,
    })
      .then((coverPath) => {
        if (coverPath) {
          embeddedCovers.value = {
            ...embeddedCovers.value,
            [key]: convertFileSrc(coverPath),
          };
          return;
        }

        withoutEmbeddedCover.add(key);
      })
      .catch(() => {
        withoutEmbeddedCover.add(key);
      })
      .finally(() => {
        embeddedInFlight.delete(key);
      });

    return undefined;
  }

  function pickSiblingCover(entries: DirEntry[]): DirEntry | undefined {
    for (const stem of SIBLING_COVER_STEMS) {
      const match = entries.find((entry) => {
        if (!entry.is_file) return false;

        const name = entry.name.toLowerCase();
        return SIBLING_COVER_EXTENSIONS.some(extension => name === `.${stem}.${extension}` || name === `${stem}.${extension}`);
      });

      if (match) {
        return match;
      }
    }

    return undefined;
  }

  /** Cached per directory, since every track in an album shares the same answer. */
  function getSiblingCover(audioPath: string): string | undefined {
    const directory = getParentDirectory(audioPath);

    if (!directory) {
      return undefined;
    }

    const cached = siblingCovers.value[directory];

    if (cached || withoutSiblingCover.has(directory) || siblingInFlight.has(directory)) {
      return cached;
    }

    siblingInFlight.add(directory);

    void invoke<DirContents>('read_dir', { path: directory })
      .then((contents) => {
        const match = pickSiblingCover(contents.entries);

        if (match) {
          siblingCovers.value = {
            ...siblingCovers.value,
            [directory]: convertFileSrc(match.path),
          };
          return;
        }

        withoutSiblingCover.add(directory);
      })
      .catch(() => {
        withoutSiblingCover.add(directory);
      })
      .finally(() => {
        siblingInFlight.delete(directory);
      });

    return undefined;
  }

  function clearAudioCovers() {
    embeddedCovers.value = {};
    siblingCovers.value = {};
    withoutEmbeddedCover.clear();
    withoutSiblingCover.clear();
  }

  return {
    embeddedCovers,
    siblingCovers,
    getEmbeddedCover,
    getSiblingCover,
    clearAudioCovers,
  };
}
