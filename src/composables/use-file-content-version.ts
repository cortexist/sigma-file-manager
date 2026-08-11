// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import {
  computed,
  onScopeDispose,
  ref,
  toValue,
  watch,
  type MaybeRefOrGetter,
  type Ref,
} from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { fileContentVersion } from '@/utils/file-content-version';
import normalizePath, { getParentDirectory } from '@/utils/normalize-path';
import type { DirEntry } from '@/types/dir-entry';

/**
 * How long the file has to hold still before its new version is published.
 *
 * A version change re-fetches the file, so publishing one per write event would restart the
 * player every few hundred milliseconds for the whole of a download — the file is *always*
 * different from the last look while it is still arriving. Waiting for quiet turns that into a
 * single reload once the writer is done, at the cost of a log file that never stops being
 * appended to never refreshing, which is the better way round.
 */
const CONTENT_SETTLE_MS = 700;

/** How long to wait on a stat before giving up; a slow mount must not hang the viewer. */
const STAT_TIMEOUT_MS = 2500;

interface DirChangePayload {
  watchedPath: string;
  changedPath: string;
  kind: string;
}

async function readEntry(path: string): Promise<DirEntry | null> {
  try {
    return await invoke<DirEntry | null>('get_dir_entry_with_timeout', {
      path,
      timeoutMs: STAT_TIMEOUT_MS,
    });
  }
  catch {
    // A file that cannot be stat'd (deleted, or on a mount that went away) keeps whatever it
    // last showed: blanking it would replace a readable view with nothing on a transient error.
    return null;
  }
}

/**
 * Keeps a file's own record of itself current, by watching the file rather than being told.
 *
 * Every surface that shows one file needs this, and none of them can get it from the listing
 * they were opened from. Quick View holds nothing but a path, and when another file manager
 * launched it there is no navigator in the process at all. The info panel is handed a
 * `DirEntry`, but that object is the one captured when the entry was clicked — a directory
 * refresh replaces the listing without re-pointing the selection at it, and a file picked out
 * of search results is not in the watched directory to begin with. Watching here is what makes
 * each of them right on its own terms, instead of correct only when something else happens to
 * be looking.
 *
 * The directory watcher is shared and reference-counted in Rust, which is what lets this join
 * a directory the navigator is already watching without either one cutting the other off.
 *
 * `dir-change` is a global event with no relation to who started the watcher, and its payload
 * names at most one changed path — the first of the batch, or none at all when the report was
 * coalesced. So this treats any report about the containing directory as "look again" and lets
 * the stat decide whether anything actually changed.
 */
export function useWatchedFileEntry(
  path: MaybeRefOrGetter<string | null | undefined>,
  options: {
    /**
     * An entry for this path that the caller already has, adopted instead of opening with a
     * stat of a file it was just handed. Only the re-reads after a change need the disk.
     */
    initial?: MaybeRefOrGetter<DirEntry | null | undefined>;
  } = {},
): {
  entry: Ref<DirEntry | null>;
  version: Ref<string | null>;
} {
  const entry = ref<DirEntry | null>(null);
  const version = computed(() => fileContentVersion(entry.value));

  let watchedDirectory: string | null = null;
  let unlistenDirChange: UnlistenFn | null = null;
  let settleTimer: ReturnType<typeof setTimeout> | null = null;
  let generation = 0;
  let isDisposed = false;

  /**
   * Watch and unwatch run one after another rather than whenever their turn in the event loop
   * comes up. Moving between directories quickly issues both for each step, and interleaved
   * they can land in the order unwatch-then-watch for the directory being *left*, which leaves
   * a watcher running that nothing will ever release.
   */
  let watcherOperations: Promise<void> = Promise.resolve();

  function enqueueWatcherOperation(operation: () => Promise<void>): Promise<void> {
    watcherOperations = watcherOperations.then(operation, operation);
    return watcherOperations;
  }

  function cancelPendingLook() {
    if (settleTimer) {
      clearTimeout(settleTimer);
      settleTimer = null;
    }
  }

  async function lookAtFile(target: string, token: number) {
    const next = await readEntry(target);

    // A late answer about a file that is no longer on screen says nothing about this one.
    if (token === generation && next) {
      entry.value = next;
    }
  }

  function lookAgainWhenSettled(target: string) {
    cancelPendingLook();
    const token = generation;

    settleTimer = setTimeout(() => {
      settleTimer = null;
      void lookAtFile(target, token);
    }, CONTENT_SETTLE_MS);
  }

  async function stopWatching() {
    const previous = watchedDirectory;
    watchedDirectory = null;

    if (previous) {
      try {
        await invoke('unwatch_directory', { path: previous });
      }
      catch {
        // Nothing to release, or the backend is already gone. Either way this window is done
        // with the directory, and holding the name would only make the next start a no-op.
      }
    }
  }

  async function startWatching(directory: string) {
    if (watchedDirectory === directory) {
      return;
    }

    await stopWatching();
    watchedDirectory = directory;

    try {
      await invoke('watch_directory', { path: directory });
    }
    catch {
      // Unwatchable directories (a virtual location, a mount that vanished) simply leave the
      // version where it is. The file still displays; it just will not notice a rewrite.
      if (watchedDirectory === directory) {
        watchedDirectory = null;
      }

      return;
    }

    if (!unlistenDirChange) {
      const unlisten = await listen<DirChangePayload>('dir-change', (event) => {
        const target = toValue(path);

        if (target && watchedDirectory && event.payload.watchedPath === watchedDirectory) {
          lookAgainWhenSettled(target);
        }
      });

      // The scope can end while the listener is still being registered, and the teardown that
      // has already run cannot release a listener that did not exist yet.
      if (isDisposed) {
        unlisten();
        return;
      }

      unlistenDirChange = unlisten;
    }
  }

  watch(() => toValue(path), (target) => {
    cancelPendingLook();
    const token = ++generation;

    if (!target) {
      entry.value = null;
      void enqueueWatcherOperation(stopWatching);
      return;
    }

    // A different file is shown at once — only a redisplay of the same one waits for quiet.
    const known = toValue(options.initial);
    entry.value = known?.path === target ? known : null;

    if (!entry.value) {
      void lookAtFile(target, token);
    }

    const directory = getParentDirectory(target);

    if (directory) {
      const normalizedDirectory = normalizePath(directory);
      void enqueueWatcherOperation(() => startWatching(normalizedDirectory));
    }
    else {
      void enqueueWatcherOperation(stopWatching);
    }
  }, { immediate: true });

  onScopeDispose(() => {
    generation += 1;
    isDisposed = true;
    cancelPendingLook();

    // Queued behind whatever is in flight, so a watch still being set up is released rather
    // than left running for a window that has gone.
    void enqueueWatcherOperation(async () => {
      unlistenDirChange?.();
      unlistenDirChange = null;
      await stopWatching();
    });
  });

  return {
    entry,
    version,
  };
}

/** The version alone, for a caller with nothing to show from the entry itself. */
export function useWatchedFileContentVersion(
  path: MaybeRefOrGetter<string | null | undefined>,
): Ref<string | null> {
  return useWatchedFileEntry(path).version;
}
