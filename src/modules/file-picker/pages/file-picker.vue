<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<!--
  The file-open dialog a picker process serves: one request in, one selection out.

  The listing is picker-grade but assembled from the navigator's own plumbing rather than a
  shared component (that extraction is the next stage): the real icon pipeline
  (FileBrowserEntryIcon), the navigator's sort comparator behind sortable Name/Size/Modified
  headers, image and video thumbnails from the shared disk cache, the user's hidden-files
  setting (Ctrl+H to override per dialog), and a virtualized list so system directories with
  thousands of entries stay responsive. Portal file-type filters are still deferred.
-->

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { homeDir } from '@tauri-apps/api/path';
import { useI18n } from 'vue-i18n';
import {
  ArrowUpIcon,
  ChevronDownIcon,
  ChevronUpIcon,
  EyeIcon,
  EyeOffIcon,
} from '@lucide/vue';
import {
  ScrollAreaCorner,
  ScrollAreaRoot,
  ScrollAreaViewport,
} from 'reka-ui';
import { Button } from '@/components/ui/button';
import { ScrollBar } from '@/components/ui/scroll-area';
import FileBrowserEntryIcon from '@/modules/navigator/components/file-browser/file-browser-entry-icon.vue';
import {
  formatBytes,
  formatDate,
  isImageFile,
  isVideoFile,
} from '@/modules/navigator/components/file-browser/utils';
import { sortFileBrowserEntries } from '@/modules/navigator/components/file-browser/utils/file-browser-sort';
import { FILE_BROWSER_SORT_COLUMN_LABEL_KEYS } from '@/modules/navigator/components/file-browser/utils/file-browser-sort-columns';
import { useImageThumbnails } from '@/modules/navigator/components/file-browser/composables/use-image-thumbnails';
import { useVideoThumbnails } from '@/modules/navigator/components/file-browser/composables/use-video-thumbnails';
import { useVerticalVirtualList } from '@/composables/use-vertical-virtual-list';
import { usePlatformStore } from '@/stores/runtime/platform';
import { useNavigatorIconsStore } from '@/stores/runtime/navigator-icons';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import {
  getNavigableParentPath,
  resolveDirectoryContents,
  virtualLocationPathExists,
} from '@/utils/virtual-locations';
import type { DirEntry } from '@/types/dir-entry';
import type { ListSortColumn, ListSortDirection } from '@/types/user-settings';

interface PickerRequest {
  title: string;
  multiple: boolean;
  directory: boolean;
  currentFolder: string | null;
  save: boolean;
  suggestedName: string | null;
}

const ROW_HEIGHT = 32;
const PICKER_SORT_COLUMNS: ListSortColumn[] = ['name', 'size', 'modified'];

const { t } = useI18n();
const platformStore = usePlatformStore();
const navigatorIconsStore = useNavigatorIconsStore();
const userSettingsStore = useUserSettingsStore();

const request = ref<PickerRequest | null>(null);
const currentPath = ref('');
const entries = ref<DirEntry[]>([]);
const selectedPaths = ref<Set<string>>(new Set());
const listingError = ref(false);
const fileName = ref('');
/**
 * Saving over an existing file takes two activations: the first arms the button as
 * "Replace", the second goes through. Any change of name, folder, or selection disarms it —
 * the confirmation belongs to one specific file.
 */
const isOverwriteArmed = ref(false);

const sortColumn = ref<ListSortColumn>('name');
const sortDirection = ref<ListSortDirection>('asc');
/** `null` until toggled: the dialog follows the user's navigator setting by default. */
const hiddenFilesOverride = ref<boolean | null>(null);

const showHiddenFiles = computed(() =>
  hiddenFilesOverride.value ?? userSettingsStore.userSettings.navigator.showHiddenFiles);

const {
  getImageThumbnail,
  getImageThumbnailPlaceholder,
  clearThumbnails: clearImageThumbnails,
} = useImageThumbnails();
const {
  getVideoThumbnail,
  clearThumbnails: clearVideoThumbnails,
} = useVideoThumbnails();

const title = computed(() => request.value?.title
  || (request.value?.save ? t('filePicker.defaultSaveTitle') : t('filePicker.defaultTitle')));

const saveDestination = computed(() => {
  const name = fileName.value.trim();

  if (!name) return '';
  return `${currentPath.value.replace(/\/+$/, '')}/${name}`;
});

const saveNameCollides = computed(() => {
  const name = fileName.value.trim();
  return entries.value.some(entry => entry.is_file && entry.name === name);
});

const visibleEntries = computed(() => {
  const filtered = showHiddenFiles.value
    ? entries.value
    : entries.value.filter(entry => !entry.is_hidden);

  // The navigator's comparator; directories always come first regardless of column.
  return sortFileBrowserEntries(filtered, sortColumn.value, sortDirection.value);
});

const virtualList = useVerticalVirtualList({
  items: visibleEntries,
  getItemSize: () => ROW_HEIGHT,
});
const visibleRows = virtualList.visibleItems;
const listSpacerStyle = virtualList.spacerStyle;
const listWindowStyle = virtualList.windowStyle;

/** Drive listings and other virtual directories are browsable but not a place to put files. */
const currentPathIsVirtual = computed(() => virtualLocationPathExists(currentPath.value));

const canConfirm = computed(() => {
  if (request.value?.save) {
    return fileName.value.trim().length > 0 && !currentPathIsVirtual.value;
  }

  // Picking a directory accepts the one on screen when nothing is highlighted.
  if (request.value?.directory) {
    return selectedPaths.value.size > 0 || !currentPathIsVirtual.value;
  }

  return selectedPaths.value.size > 0;
});

const confirmLabel = computed(() => {
  if (request.value?.save) {
    return isOverwriteArmed.value ? t('filePicker.replace') : t('filePicker.save');
  }

  return request.value?.directory ? t('filePicker.selectFolder') : t('filePicker.open');
});

async function listDirectory(path: string) {
  try {
    const contents = await resolveDirectoryContents(path);
    currentPath.value = path;
    entries.value = contents.entries;
    selectedPaths.value = new Set();
    listingError.value = false;
    isOverwriteArmed.value = false;
    clearImageThumbnails();
    clearVideoThumbnails();
    virtualList.setScrollTop(0);
    navigatorIconsStore.prefetchForDirectoryEntries(contents.entries);
  }
  catch {
    // The previous listing stays useful; a dialog with nowhere to stand shows its error row.
    listingError.value = entries.value.length === 0;
  }
}

/**
 * Thumbnails use the default dimensions on purpose: the disk cache keys include the
 * requested size, so matching the navigator's requests means a folder already browsed in
 * the app renders its previews here instantly.
 */
function getEntryThumbnail(entry: DirEntry): string | undefined {
  if (isImageFile(entry)) {
    return getImageThumbnail(entry) ?? getImageThumbnailPlaceholder(entry);
  }

  if (isVideoFile(entry)) {
    return getVideoThumbnail(entry);
  }

  return undefined;
}

function entrySizeText(entry: DirEntry): string {
  return entry.is_file ? formatBytes(Number(entry.size) || 0) : '';
}

function entryModifiedText(entry: DirEntry): string {
  return entry.modified_time ? formatDate(Number(entry.modified_time)) : '';
}

function setSortColumn(column: ListSortColumn) {
  if (sortColumn.value === column) {
    sortDirection.value = sortDirection.value === 'asc' ? 'desc' : 'asc';
    return;
  }

  sortColumn.value = column;
  sortDirection.value = 'asc';
}

function toggleHiddenFiles() {
  hiddenFilesOverride.value = !showHiddenFiles.value;
}

function isSelectable(entry: DirEntry): boolean {
  return request.value?.directory ? entry.is_dir : entry.is_file;
}

/** In save mode a click on a file adopts its name — the natural way to say "that one". */
function adoptFileName(entry: DirEntry) {
  if (request.value?.save && entry.is_file) {
    fileName.value = entry.name;
    isOverwriteArmed.value = false;
  }
}

function toggleSelection(entry: DirEntry, event: MouseEvent) {
  adoptFileName(entry);

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
  const parent = getNavigableParentPath(currentPath.value, platformStore.currentPlatform);

  if (parent && parent !== currentPath.value) {
    void listDirectory(parent);
  }
}

function finish(paths: string[]) {
  void invoke('file_picker_finish', { paths });
}

function confirmSelection() {
  if (!canConfirm.value) return;

  if (request.value?.save) {
    if (saveNameCollides.value && !isOverwriteArmed.value) {
      isOverwriteArmed.value = true;
      return;
    }

    finish([saveDestination.value]);
    return;
  }

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

  // The GTK file chooser's shortcut; it works from the name field there too.
  if (event.ctrlKey && !event.shiftKey && !event.altKey && event.key.toLowerCase() === 'h') {
    event.preventDefault();
    toggleHiddenFiles();
  }
}

onMounted(async () => {
  window.addEventListener('keydown', onKeydown);

  request.value = await invoke<PickerRequest | null>('file_picker_request');

  if (!request.value) {
    // Not a picker process; this page has nothing to serve.
    return;
  }

  fileName.value = request.value.suggestedName ?? '';

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
      <Button
        variant="ghost"
        size="sm"
        class="file-picker__hidden-toggle"
        :aria-label="t('filePicker.showHiddenFiles')"
        :title="t('filePicker.showHiddenFiles')"
        @click="toggleHiddenFiles"
      >
        <component
          :is="showHiddenFiles ? EyeIcon : EyeOffIcon"
          :size="16"
        />
      </Button>
    </div>

    <div class="file-picker__columns">
      <span
        class="file-picker__entry-preview"
        aria-hidden="true"
      />
      <button
        v-for="column in PICKER_SORT_COLUMNS"
        :key="column"
        type="button"
        class="file-picker__column-button"
        :class="[
          `file-picker__column-button--${column}`,
          { 'file-picker__column-button--active': sortColumn === column },
        ]"
        @click="setSortColumn(column)"
      >
        <span class="file-picker__column-label">{{ t(FILE_BROWSER_SORT_COLUMN_LABEL_KEYS[column]) }}</span>
        <component
          :is="sortDirection === 'asc' ? ChevronUpIcon : ChevronDownIcon"
          v-if="sortColumn === column"
          :size="12"
        />
      </button>
    </div>

    <ScrollAreaRoot
      type="auto"
      class="sigma-ui-scroll-area file-picker__listing"
    >
      <ScrollAreaViewport
        :ref="virtualList.setScrollViewportRef"
        class="sigma-ui-scroll-area__viewport file-picker__viewport"
        @scroll.passive="virtualList.handleScroll"
      >
        <p
          v-if="listingError"
          class="file-picker__empty"
        >
          {{ t('filePicker.listingFailed') }}
        </p>
        <div
          v-else
          class="file-picker__scroll-inner"
          :style="listSpacerStyle"
        >
          <div
            class="file-picker__virtual-window"
            :style="listWindowStyle"
          >
            <button
              v-for="row in visibleRows"
              :key="row.item.path"
              type="button"
              class="file-picker__entry"
              :style="{ height: `${row.size}px` }"
              :class="{
                'file-picker__entry--selected': selectedPaths.has(row.item.path),
                'file-picker__entry--inert': !isSelectable(row.item) && !row.item.is_dir,
              }"
              @click="toggleSelection(row.item, $event)"
              @dblclick="activateEntry(row.item)"
            >
              <span class="file-picker__entry-preview">
                <img
                  v-if="getEntryThumbnail(row.item)"
                  :src="getEntryThumbnail(row.item)"
                  class="file-picker__entry-thumbnail"
                  alt=""
                  draggable="false"
                >
                <FileBrowserEntryIcon
                  v-else
                  :entry="row.item"
                  :size="18"
                  class="file-picker__entry-icon"
                />
              </span>
              <span class="file-picker__entry-name">{{ row.item.name }}</span>
              <span class="file-picker__entry-size">{{ entrySizeText(row.item) }}</span>
              <span class="file-picker__entry-modified">{{ entryModifiedText(row.item) }}</span>
            </button>
          </div>
        </div>
      </ScrollAreaViewport>
      <ScrollBar orientation="vertical" />
      <ScrollAreaCorner />
    </ScrollAreaRoot>

    <div
      v-if="request?.save"
      class="file-picker__name-row"
    >
      <input
        v-model="fileName"
        type="text"
        class="file-picker__name-input"
        :placeholder="t('filePicker.fileNamePlaceholder')"
        @input="isOverwriteArmed = false"
        @keydown.enter.stop="confirmSelection"
      >
      <span
        v-if="isOverwriteArmed"
        class="file-picker__overwrite-warning"
      >
        {{ t('filePicker.replaceWarning') }}
      </span>
    </div>

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
        {{ confirmLabel }}
      </Button>
    </footer>
  </div>
</template>

<style scoped>
/*
  The page mounts as a flex item of #app, so without a pinned width its min-content —
  the longest nowrap filename in the listing — inflates the whole layout and pushes the
  right-aligned footer past the window edge. Pinning width and zeroing min-width keeps
  the page exactly window-sized no matter what the current folder contains.
*/

.file-picker {
  --file-picker-size-column-width: 84px;
  --file-picker-modified-column-width: 150px;

  display: flex;
  overflow: hidden;
  width: 100vw;
  min-width: 0;
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

.file-picker__hidden-toggle {
  margin-left: auto;
}

/* Mirrors the row layout (preview spacer + cells) so headers align with their columns. */
.file-picker__columns {
  display: flex;
  align-items: center;
  padding: 0 18px 4px;
  gap: 10px;
}

.file-picker__column-button {
  display: flex;
  align-items: center;
  padding: 2px 0;
  border: none;
  background: transparent;
  color: rgb(255 255 255 / 45%);
  cursor: default;
  font: inherit;
  font-size: 12px;
  gap: 4px;
}

.file-picker__column-button:hover,
.file-picker__column-button--active {
  color: rgb(255 255 255 / 75%);
}

.file-picker__column-button--name {
  min-width: 0;
  flex: 1;
}

.file-picker__column-button--size {
  width: var(--file-picker-size-column-width);
  flex: none;
  justify-content: flex-end;
}

.file-picker__column-button--modified {
  width: var(--file-picker-modified-column-width);
  flex: none;
}

.file-picker__listing {
  min-height: 0;
  flex: 1;
}

.file-picker__viewport {
  width: 100%;
  height: 100%;
}

/*
  The scroll-area viewport wraps content in an inline-styled `display: table` element,
  and a table is never narrower than its widest row — which defeats the entry names'
  ellipsis and grows the listing to the longest filename. This listing only scrolls
  vertically, so a block wrapper (which clamps to the viewport width) is correct.
  !important is needed to beat the inline styles.
*/

.file-picker__listing :deep(.sigma-ui-scroll-area__viewport > div) {
  display: block !important;
  min-width: 0 !important;
}

.file-picker__virtual-window {
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
  padding: 0 10px;
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

.file-picker__entry-preview {
  display: flex;
  width: 24px;
  height: 24px;
  flex: none;
  align-items: center;
  justify-content: center;
}

.file-picker__entry-icon {
  opacity: 0.7;
}

.file-picker__entry-thumbnail {
  width: 24px;
  height: 24px;
  border-radius: 3px;
  object-fit: cover;
}

.file-picker__entry-name {
  overflow: hidden;
  min-width: 0;
  flex: 1;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-picker__entry-size {
  width: var(--file-picker-size-column-width);
  flex: none;
  color: rgb(255 255 255 / 45%);
  font-size: 12px;
  text-align: right;
  white-space: nowrap;
}

.file-picker__entry-modified {
  overflow: hidden;
  width: var(--file-picker-modified-column-width);
  flex: none;
  color: rgb(255 255 255 / 45%);
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-picker__name-row {
  display: flex;
  align-items: center;
  padding: 8px 16px 0;
  gap: 10px;
}

.file-picker__name-input {
  flex: 1;
  padding: 7px 10px;
  border: 1px solid rgb(255 255 255 / 14%);
  border-radius: 6px;
  background: rgb(255 255 255 / 5%);
  color: inherit;
  font: inherit;
  outline: none;
}

.file-picker__name-input:focus {
  border-color: rgb(255 255 255 / 32%);
}

.file-picker__overwrite-warning {
  flex: none;
  color: #f0a30a;
  font-size: 12px;
}

.file-picker__actions {
  display: flex;
  justify-content: flex-end;
  padding: 10px 16px 14px;
  gap: 8px;
}
</style>
