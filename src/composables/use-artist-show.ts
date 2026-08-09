// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { ref } from 'vue';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { getParentDirectory, getPathLeafName } from '@/utils/normalize-path';
import { decodeTextFileBytes } from '@/utils/decode-text-file-bytes';
import {
  buildNowPlayingCards,
  parseArtistInfo,
  type NowPlayingCard,
} from '@/utils/artist-info';
import type { DirContents, DirEntry } from '@/types/dir-entry';

/**
 * Collects what the fullscreen audio show needs from a folder the user has assembled by hand.
 * The Zune pulled artist photography and biographies from its marketplace, which is exactly why
 * the feature died with the service; here the same material sits beside the music:
 *
 *   Dire Straits/
 *     .artist/                  (or `artist/`, hidden or not)
 *       artist.info
 *       any-name.jpg
 *     Dire Straits - Money For Nothing.mp3
 *
 * The folder is looked for beside the track and then one level up, so a
 * `Artist/Album/track.mp3` tree can keep one `.artist` folder for every album under it.
 */

const FOLDER_NAMES = ['.artist', 'artist'];
const IMAGE_EXTENSIONS = ['jpg', 'jpeg', 'png', 'webp', 'avif', 'gif'];
const INFO_FILE_NAME = 'artist.info';
const INFO_MAX_BYTES = 256 * 1024;

export interface ArtistShow {
  /** Backdrops in name order, already converted for the webview. */
  photos: string[];
  cards: NowPlayingCard[];
}

interface ReadTextPreviewResult {
  bytes: number[];
  truncated: boolean;
}

const EMPTY_SHOW: ArtistShow = {
  photos: [],
  cards: [],
};

/** Keyed by the track's directory, since every track in a folder resolves to the same show. */
const cache = new Map<string, ArtistShow>();

function hasImageExtension(name: string): boolean {
  const extension = name.split('.').pop()?.toLowerCase() ?? '';
  return IMAGE_EXTENSIONS.includes(extension);
}

async function readDirEntries(path: string): Promise<DirEntry[]> {
  try {
    const contents = await invoke<DirContents>('read_dir', { path });
    return contents.entries ?? [];
  }
  catch {
    // A missing or unreadable folder is the normal case, not an error worth surfacing.
    return [];
  }
}

async function findArtistFolder(trackDirectory: string): Promise<string | null> {
  const parent = getParentDirectory(trackDirectory);
  const searchRoots = parent && parent !== trackDirectory
    ? [trackDirectory, parent]
    : [trackDirectory];

  for (const root of searchRoots) {
    const entries = await readDirEntries(root);
    const match = entries.find(
      entry => !entry.is_file && FOLDER_NAMES.includes(entry.name.toLowerCase()),
    );

    if (match) {
      return match.path;
    }
  }

  return null;
}

async function readInfoFile(entries: DirEntry[]): Promise<string | null> {
  const named = entries.find(
    entry => entry.is_file && entry.name.toLowerCase() === INFO_FILE_NAME,
  );
  const fallback = entries.find(
    entry => entry.is_file && entry.name.toLowerCase().endsWith('.info'),
  );
  const target = named ?? fallback;

  if (!target) {
    return null;
  }

  try {
    const preview = await invoke<ReadTextPreviewResult>('read_text_preview', {
      path: target.path,
      maxBytes: INFO_MAX_BYTES,
    });

    return decodeTextFileBytes(new Uint8Array(preview.bytes));
  }
  catch {
    return null;
  }
}

/**
 * Resolves the show for a track. Always returns something: with no sidecar at all the cards are
 * rebuilt from the file name, which is often the only place the artist and title exist — files
 * pulled off the web routinely carry no tags whatsoever.
 */
export async function loadArtistShow(audioPath: string): Promise<ArtistShow> {
  const directory = getParentDirectory(audioPath);
  const fileName = getPathLeafName(audioPath);

  if (!directory) {
    return EMPTY_SHOW;
  }

  const cached = cache.get(directory);

  if (cached) {
    return cached;
  }

  const folder = await findArtistFolder(directory);
  const entries = folder ? await readDirEntries(folder) : [];
  const infoText = entries.length > 0 ? await readInfoFile(entries) : null;

  const photos = entries
    .filter(entry => entry.is_file && hasImageExtension(entry.name))
    .sort((left, right) => left.name.localeCompare(right.name))
    .map(entry => convertFileSrc(entry.path));

  const show: ArtistShow = {
    photos,
    cards: buildNowPlayingCards({
      info: infoText ? parseArtistInfo(infoText) : null,
      fileName,
    }),
  };

  cache.set(directory, show);

  return show;
}

export function clearArtistShowCache() {
  cache.clear();
}

/**
 * Reactive wrapper for a player that switches files: the result lands in `show` when it
 * resolves, and a file switched away from mid-read never overwrites the newer one.
 */
export function useArtistShow() {
  const show = ref<ArtistShow>(EMPTY_SHOW);
  let requestId = 0;

  async function load(audioPath: string | null | undefined) {
    requestId += 1;
    const currentRequest = requestId;

    if (!audioPath) {
      show.value = EMPTY_SHOW;
      return;
    }

    const resolved = await loadArtistShow(audioPath);

    if (currentRequest === requestId) {
      show.value = resolved;
    }
  }

  return {
    show,
    load,
  };
}
