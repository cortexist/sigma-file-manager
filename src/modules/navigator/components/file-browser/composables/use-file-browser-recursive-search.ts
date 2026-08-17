// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import {
  computed, onUnmounted, ref, watch, type ComputedRef, type Ref,
} from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { DirEntry } from '@/types/dir-entry';
import { parseQuickSearchQuery } from '@/modules/navigator/components/file-browser/utils/file-browser-quick-search-query';

/**
 * Long enough that a walk is not started for every keystroke, short enough that the
 * results still feel like they belong to what was just typed.
 */
const SEARCH_DEBOUNCE_MS = 250;

type RecursiveSearchResults = {
  entries: DirEntry[];
  truncated: boolean;
  superseded: boolean;
  scannedCount: number;
};

let nextSearchKeyId = 0;

/**
 * The name filter the backend can apply while it walks. Only a bare query or an explicit
 * `name:` query is about names; every other property is decided from metadata the walk
 * does not read, so those searches walk unfiltered and are narrowed here instead.
 */
export function getRecursiveSearchNameQuery(filterQuery: string): string {
  const parsed = parseQuickSearchQuery(filterQuery.trim());

  if (parsed.property === null) {
    return parsed.value.trim();
  }

  return parsed.property === 'name' ? parsed.value.trim() : '';
}

export function useFileBrowserRecursiveSearch(options: {
  currentPath: Ref<string> | ComputedRef<string>;
  filterQuery: Ref<string>;
  isEnabled: ComputedRef<boolean>;
  useRegex: ComputedRef<boolean>;
  includeHidden: ComputedRef<boolean>;
}) {
  const searchKey = `file-browser-recursive-search:${nextSearchKeyId += 1}`;

  const entries = ref<DirEntry[]>([]);
  const isSearching = ref(false);
  const isTruncated = ref(false);
  const error = ref<string | null>(null);
  const debounceTimerId = ref<ReturnType<typeof setTimeout> | null>(null);
  let requestSequence = 0;

  const isActive = computed(() => {
    return options.isEnabled.value
      && options.currentPath.value !== ''
      && options.filterQuery.value.trim() !== '';
  });

  function cancelPendingSearch() {
    if (debounceTimerId.value !== null) {
      clearTimeout(debounceTimerId.value);
      debounceTimerId.value = null;
    }

    if (!isSearching.value) {
      return;
    }

    // The walk itself runs on a blocking thread that no promise can interrupt, so the
    // backend is told to stop rather than left running for results nobody will read.
    void invoke('cancel_dir_search', { searchKey });
  }

  function reset() {
    cancelPendingSearch();
    requestSequence += 1;
    entries.value = [];
    isSearching.value = false;
    isTruncated.value = false;
    error.value = null;
  }

  async function runSearch(path: string, filterQuery: string) {
    requestSequence += 1;
    const currentRequest = requestSequence;
    isSearching.value = true;

    try {
      const results = await invoke<RecursiveSearchResults>('search_dir_recursive', {
        path,
        options: {
          searchKey,
          query: getRecursiveSearchNameQuery(filterQuery),
          regex: options.useRegex.value,
          includeHidden: options.includeHidden.value,
        },
      });

      if (currentRequest !== requestSequence || results.superseded) {
        return;
      }

      entries.value = results.entries;
      isTruncated.value = results.truncated;
      error.value = null;
    }
    catch (invokeError) {
      if (currentRequest !== requestSequence) {
        return;
      }

      entries.value = [];
      isTruncated.value = false;
      error.value = String(invokeError);
    }
    finally {
      if (currentRequest === requestSequence) {
        isSearching.value = false;
      }
    }
  }

  function scheduleSearch() {
    cancelPendingSearch();

    const path = options.currentPath.value;
    const filterQuery = options.filterQuery.value;

    debounceTimerId.value = setTimeout(() => {
      debounceTimerId.value = null;
      void runSearch(path, filterQuery);
    }, SEARCH_DEBOUNCE_MS);
  }

  watch(
    () => [
      isActive.value,
      options.currentPath.value,
      options.filterQuery.value,
      options.useRegex.value,
      options.includeHidden.value,
    ] as const,
    ([active]) => {
      if (!active) {
        reset();
        return;
      }

      scheduleSearch();
    },
    { immediate: true },
  );

  onUnmounted(() => {
    cancelPendingSearch();
  });

  return {
    isActive,
    entries,
    isSearching,
    isTruncated,
    error,
  };
}
