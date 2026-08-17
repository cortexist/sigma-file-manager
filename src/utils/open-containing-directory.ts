// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import type { DirEntry } from '@/types/dir-entry';
import { useWorkspacesStore } from '@/stores/storage/workspaces';
import { useGlobalSearchStore } from '@/stores/runtime/global-search';
import { getParentPath } from '@/utils/file-operation-paths';
import router from '@/router';

/**
 * Opens the folder an entry lives in and selects the entry there.
 *
 * This is the same thing another application asks for through `FileManager1`'s "Show in
 * Folder", so it goes the same way: the folder's tab is opened or focused, and the entry is
 * marked to be revealed once that listing has loaded.
 */
export async function openContainingDirectory(entry: DirEntry): Promise<boolean> {
  const parentPath = getParentPath(entry.path);

  if (!parentPath || parentPath === entry.path) {
    return false;
  }

  const workspacesStore = useWorkspacesStore();
  const globalSearchStore = useGlobalSearchStore();

  // Results are shown over the navigator, so they would cover the folder just opened.
  if (globalSearchStore.isOpen) {
    globalSearchStore.close();
  }

  if (router.currentRoute.value.name !== 'navigator') {
    await router.push({ name: 'navigator' });
  }

  await workspacesStore.openOrFocusTabGroup(parentPath);
  workspacesStore.setPendingLaunchReveal(parentPath, entry.path);

  return true;
}
