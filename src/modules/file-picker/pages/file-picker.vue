<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<!--
  The file-open dialog a picker process serves: one request in, one selection out.

  Deliberately a walking skeleton. It lists, navigates, and selects with the app's own data
  plumbing (`read_dir`, the entry types), but none of the navigator's presentation yet — no
  thumbnails, no icons themes, no filters. The point of this stage is that the portal → DBus →
  process → dialog → reply loop is real; the browsing surface it shows will grow into the
  shared component the navigator itself uses.
-->

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { homeDir } from '@tauri-apps/api/path';
import { useI18n } from 'vue-i18n';
import {
  ArrowUpIcon,
  FileIcon,
  FolderIcon,
} from '@lucide/vue';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { getParentDirectory } from '@/utils/normalize-path';
import { getFileName } from '@/stores/runtime/quick-view';
import type { DirContents, DirEntry } from '@/types/dir-entry';

interface PickerRequest {
  title: string;
  multiple: boolean;
  directory: boolean;
  currentFolder: string | null;
}

const { t } = useI18n();

const request = ref<PickerRequest | null>(null);
const currentPath = ref('');
const entries = ref<DirEntry[]>([]);
const selectedPaths = ref<Set<string>>(new Set());
const listingError = ref(false);

const title = computed(() => request.value?.title || t('filePicker.defaultTitle'));

/**
 * Directories first, names in natural order, dotfiles left out. The full browsing surface
 * will bring the navigator's sorting and hidden-file settings; a dialog guesses less.
 */
const visibleEntries = computed(() => {
  return entries.value
    .filter(entry => !getFileName(entry.path).startsWith('.'))
    .sort((a, b) => {
      if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
      return getFileName(a.path).localeCompare(getFileName(b.path), undefined, {
        numeric: true,
        sensitivity: 'base',
      });
    });
});

const canConfirm = computed(() => {
  // Picking a directory accepts the one on screen when nothing is highlighted.
  if (request.value?.directory) return true;
  return selectedPaths.value.size > 0;
});

async function listDirectory(path: string) {
  try {
    const contents = await invoke<DirContents>('read_dir', { path });
    currentPath.value = path;
    entries.value = contents.entries;
    selectedPaths.value = new Set();
    listingError.value = false;
  }
  catch {
    // The previous listing stays useful; a dialog with nowhere to stand shows its error row.
    listingError.value = entries.value.length === 0;
  }
}

function isSelectable(entry: DirEntry): boolean {
  return request.value?.directory ? entry.is_dir : entry.is_file;
}

function toggleSelection(entry: DirEntry, event: MouseEvent) {
  if (!isSelectable(entry)) {
    return;
  }

  const next = new Set(request.value?.multiple && (event.ctrlKey || event.metaKey)
    ? selectedPaths.value
    : []);

  if (selectedPaths.value.has(entry.path) && next.size === selectedPaths.value.size) {
    next.delete(entry.path);
  }
  else {
    next.add(entry.path);
  }

  selectedPaths.value = next;
}

function activateEntry(entry: DirEntry) {
  if (entry.is_dir) {
    void listDirectory(entry.path);
    return;
  }

  if (!request.value?.directory) {
    finish([entry.path]);
  }
}

function goUp() {
  const parent = getParentDirectory(currentPath.value);

  if (parent && parent !== currentPath.value) {
    void listDirectory(parent);
  }
}

function finish(paths: string[]) {
  void invoke('file_picker_finish', { paths });
}

function confirmSelection() {
  if (selectedPaths.value.size > 0) {
    finish([...selectedPaths.value]);
    return;
  }

  if (request.value?.directory) {
    finish([currentPath.value]);
  }
}

function cancel() {
  finish([]);
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    cancel();
  }

  if (event.key === 'Enter' && canConfirm.value) {
    confirmSelection();
  }
}

onMounted(async () => {
  window.addEventListener('keydown', onKeydown);

  request.value = await invoke<PickerRequest | null>('file_picker_request');

  if (!request.value) {
    // Not a picker process; this page has nothing to serve.
    return;
  }

  const startingFolder = request.value.currentFolder || await homeDir();
  await listDirectory(startingFolder);

  const currentWindow = getCurrentWindow();
  await currentWindow.setTitle(title.value);
  await currentWindow.show();
  await currentWindow.setFocus();
});
</script>

<template>
  <div class="file-picker">
    <header
      class="file-picker__header"
      data-tauri-drag-region
    >
      <span class="file-picker__title">{{ title }}</span>
      <span class="file-picker__path">{{ currentPath }}</span>
    </header>

    <div class="file-picker__toolbar">
      <Button
        variant="ghost"
        size="sm"
        :aria-label="t('filePicker.parentDirectory')"
        @click="goUp"
      >
        <ArrowUpIcon :size="16" />
      </Button>
    </div>

    <ScrollArea class="file-picker__listing">
      <p
        v-if="listingError"
        class="file-picker__empty"
      >
        {{ t('filePicker.listingFailed') }}
      </p>
      <button
        v-for="entry in visibleEntries"
        :key="entry.path"
        type="button"
        class="file-picker__entry"
        :class="{
          'file-picker__entry--selected': selectedPaths.has(entry.path),
          'file-picker__entry--inert': !isSelectable(entry) && !entry.is_dir,
        }"
        @click="toggleSelection(entry, $event)"
        @dblclick="activateEntry(entry)"
      >
        <component
          :is="entry.is_dir ? FolderIcon : FileIcon"
          :size="16"
          class="file-picker__entry-icon"
        />
        <span class="file-picker__entry-name">{{ getFileName(entry.path) }}</span>
      </button>
    </ScrollArea>

    <footer class="file-picker__actions">
      <Button
        variant="ghost"
        @click="cancel"
      >
        {{ t('filePicker.cancel') }}
      </Button>
      <Button
        :disabled="!canConfirm"
        @click="confirmSelection"
      >
        {{ request?.directory ? t('filePicker.selectFolder') : t('filePicker.open') }}
      </Button>
    </footer>
  </div>
</template>

<style scoped>
.file-picker {
  display: flex;
  height: 100vh;
  flex-direction: column;
  background: #242227;
  color: rgb(255 255 255 / 88%);
}

.file-picker__header {
  display: flex;
  align-items: baseline;
  padding: 12px 16px 8px;
  gap: 12px;
}

.file-picker__title {
  font-size: 14px;
  font-weight: 600;
}

.file-picker__path {
  overflow: hidden;
  color: rgb(255 255 255 / 45%);
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-picker__toolbar {
  display: flex;
  padding: 0 12px 6px;
}

.file-picker__listing {
  flex: 1;
  padding: 0 8px;
}

.file-picker__empty {
  padding: 24px 16px;
  color: rgb(255 255 255 / 45%);
}

.file-picker__entry {
  display: flex;
  width: 100%;
  align-items: center;
  padding: 6px 10px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: inherit;
  cursor: default;
  font: inherit;
  gap: 10px;
  text-align: left;
}

.file-picker__entry:hover {
  background: rgb(255 255 255 / 6%);
}

.file-picker__entry--selected,
.file-picker__entry--selected:hover {
  background: rgb(255 255 255 / 14%);
}

.file-picker__entry--inert {
  opacity: 0.45;
}

.file-picker__entry-icon {
  flex: none;
  opacity: 0.7;
}

.file-picker__entry-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-picker__actions {
  display: flex;
  justify-content: flex-end;
  padding: 10px 16px 14px;
  gap: 8px;
}
</style>
