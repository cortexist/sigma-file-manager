<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<!--
  The file-open dialog a picker process serves: one request in, one selection out.

  The listing is picker-grade and styled after the navigator, assembled from its plumbing
  rather than a shared component (that extraction is the next stage): the real icon pipeline,
  image and video thumbnails from the shared disk cache, the navigator's sort comparator, and
  entries grouped the way the navigator groups them — folders in a section on top, files in a
  section below — in either a list or a tile view. The toolbar borrows the navigator's own
  components outright: the back/forward/up/home/refresh cluster (backed by the dialog's own
  small history, not a Tab), the breadcrumb address bar, and the Ctrl+L path editor dialog —
  all of them props-and-emits driven, which is what makes the borrowing possible. The
  hidden-files switch follows the user's navigator setting until toggled (Ctrl+H works too)
  and never writes it back: a dialog borrows preferences, it does not edit them. The caller's
  portal type filters surface as a dropdown in the footer, narrowing the files (never the
  folders) to what the application can actually open. Virtualized throughout so system
  directories with thousands of entries stay responsive.
-->

<script setup lang="ts">
import {
  computed, onBeforeUnmount, onMounted, ref, watch,
} from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { homeDir } from '@tauri-apps/api/path';
import { useI18n } from 'vue-i18n';
import {
  ChevronDownIcon,
  ChevronUpIcon,
  FileIcon,
  FolderIcon,
  LayoutGridIcon,
  ListIcon,
} from '@lucide/vue';
import {
  ScrollAreaCorner,
  ScrollAreaRoot,
  ScrollAreaViewport,
} from 'reka-ui';
import { Button } from '@/components/ui/button';
import { ScrollBar } from '@/components/ui/scroll-area';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectItemText,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { Toaster } from '@/components/ui/toaster';
import FileBrowserEntryIcon from '@/modules/navigator/components/file-browser/file-browser-entry-icon.vue';
import FileBrowserGridSectionBar from '@/modules/navigator/components/file-browser/file-browser-grid-section-bar.vue';
import FileBrowserToolbarNavButtons from '@/modules/navigator/components/file-browser/file-browser-toolbar-nav-buttons.vue';
import FileBrowserToolbarAddressBar from '@/modules/navigator/components/file-browser/file-browser-toolbar-address-bar.vue';
import AddressBarEditorDialog from '@/modules/navigator/components/file-browser/address-bar-editor-dialog.vue';
import FilePickerTileCard from '@/modules/file-picker/components/file-picker-tile-card.vue';
import FilePickerInfusion from '@/modules/file-picker/components/file-picker-infusion.vue';
import {
  formatBytes,
  formatDate,
  isAudioFile,
  isImageFile,
  isVideoFile,
} from '@/modules/navigator/components/file-browser/utils';
import { useAudioCovers } from '@/composables/use-audio-covers';
import { sortFileBrowserEntries } from '@/modules/navigator/components/file-browser/utils/file-browser-sort';
import { FILE_BROWSER_SORT_COLUMN_LABEL_KEYS } from '@/modules/navigator/components/file-browser/utils/file-browser-sort-columns';
import { getGridColumnCount } from '@/modules/navigator/components/file-browser/utils/file-browser-virtual-rows';
import { FILE_BROWSER_GRID_GAP_DEFAULT } from '@/modules/navigator/components/file-browser/utils/file-browser-layout-gaps';
import { useImageThumbnails } from '@/modules/navigator/components/file-browser/composables/use-image-thumbnails';
import { useVideoThumbnails } from '@/modules/navigator/components/file-browser/composables/use-video-thumbnails';
import { buildSectionedVirtualRows, useVerticalVirtualList } from '@/composables/use-vertical-virtual-list';
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

interface PickerFilter {
  name: string;
  globs: string[];
  mimes: string[];
}

interface PickerRequest {
  title: string;
  multiple: boolean;
  directory: boolean;
  currentFolder: string | null;
  save: boolean;
  suggestedName: string | null;
  filters: PickerFilter[];
  currentFilter: string | null;
}

type PickerViewLayout = 'list' | 'grid';

type PickerSectionKey = 'dirs' | 'files';

/**
 * Fallbacks only. The real row heights are the `--file-picker-*-height` custom properties on
 * the page root, read back below — a virtualized list has to know every row's height in
 * JavaScript to place the scroll window, so the numbers cannot live in CSS alone, but they
 * should still only be *written* in one place.
 */
const ROW_HEIGHT_FALLBACKS = {
  list: 42,
  section: 42,
  tileDir: 52,
  tileFile: 120,
};
/** The virtual window's own horizontal padding, excluded from tile column math. */
const LISTING_PADDING_X = 16;
const PICKER_SORT_COLUMNS: ListSortColumn[] = ['name', 'size', 'modified'];

const { t } = useI18n();
const platformStore = usePlatformStore();
const navigatorIconsStore = useNavigatorIconsStore();
const userSettingsStore = useUserSettingsStore();

const rootRef = ref<HTMLElement | null>(null);
const rowHeights = ref({ ...ROW_HEIGHT_FALLBACKS });

/**
 * Pulls the row heights out of CSS so the stylesheet stays the one place they are set. Called
 * once the page is mounted; edit the custom properties and the dev server's reload re-reads
 * them. Changing a height purely in devtools moves the boxes but not the scroll math, since
 * the virtual list has already been told how tall each row is.
 */
function readRowHeights() {
  const element = rootRef.value;

  if (!element) {
    return;
  }

  const styles = getComputedStyle(element);

  function readPx(property: string, fallback: number) {
    const value = Number.parseFloat(styles.getPropertyValue(property));
    return Number.isFinite(value) && value > 0 ? value : fallback;
  }

  rowHeights.value = {
    list: readPx('--file-picker-list-row-height', ROW_HEIGHT_FALLBACKS.list),
    section: readPx('--file-picker-section-row-height', ROW_HEIGHT_FALLBACKS.section),
    tileDir: readPx('--file-picker-tile-dir-height', ROW_HEIGHT_FALLBACKS.tileDir),
    tileFile: readPx('--file-picker-tile-file-height', ROW_HEIGHT_FALLBACKS.tileFile),
  };
}

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

/** The dialog's own back/forward trail; the navigator's Tab history has no place here. */
const pathHistory = ref<string[]>([]);
const pathHistoryIndex = ref(-1);
const addressBarEditorRef = ref<InstanceType<typeof AddressBarEditorDialog> | null>(null);

const canGoBack = computed(() => pathHistoryIndex.value > 0);
const canGoForward = computed(() => pathHistoryIndex.value < pathHistory.value.length - 1);
const canGoUp = computed(() => {
  const parent = getNavigableParentPath(currentPath.value, platformStore.currentPlatform);
  return !!parent && parent !== currentPath.value;
});

/** The caller's type filter currently in force, by name; `null` when none were sent. */
const selectedFilterName = ref<string | null>(null);

const activeFilterMatchers = computed(() => {
  const filter = (request.value?.filters ?? [])
    .find(candidate => candidate.name === selectedFilterName.value);

  // A filter with no patterns admits everything; treating it as "no filter" keeps a
  // careless caller from blanking the listing.
  if (!filter || (filter.globs.length === 0 && filter.mimes.length === 0)) {
    return null;
  }

  return {
    globs: filter.globs.map(globToRegExp),
    mimes: filter.mimes.map(pattern => pattern.toLowerCase()),
  };
});

/**
 * Case-insensitive on purpose, unlike GTK's fnmatch: an app asking for `*.jpg` means the
 * photos, not the photos whose extension happens to be lowercase.
 */
function globToRegExp(glob: string): RegExp {
  const source = glob
    .replace(/[.+^${}()|[\]\\]/g, '\\$&')
    .replace(/\*/g, '.*')
    .replace(/\?/g, '.');

  return new RegExp(`^${source}$`, 'i');
}

function entryPassesTypeFilter(entry: DirEntry): boolean {
  const matchers = activeFilterMatchers.value;

  if (!matchers) {
    return true;
  }

  if (matchers.globs.some(glob => glob.test(entry.name))) {
    return true;
  }

  const mime = entry.mime?.toLowerCase();

  if (!mime) {
    return false;
  }

  return matchers.mimes.some((pattern) => {
    if (pattern === '*' || pattern === '*/*') {
      return true;
    }

    if (pattern.endsWith('/*')) {
      return mime.startsWith(pattern.slice(0, -1));
    }

    return mime === pattern;
  });
}

const sortColumn = ref<ListSortColumn>('name');
const sortDirection = ref<ListSortDirection>('asc');
/** `null` until toggled: the dialog follows the user's navigator settings by default. */
const hiddenFilesOverride = ref<boolean | null>(null);
const layoutOverride = ref<PickerViewLayout | null>(null);

const showHiddenFiles = computed(() =>
  hiddenFilesOverride.value ?? userSettingsStore.userSettings.navigator.showHiddenFiles);

const viewLayout = computed<PickerViewLayout>(() => layoutOverride.value
  ?? (userSettingsStore.userSettings.navigator.layout.type.name === 'grid' ? 'grid' : 'list'));

const {
  getImageThumbnail,
  getImageThumbnailPlaceholder,
  clearThumbnails: clearImageThumbnails,
} = useImageThumbnails();
const {
  getVideoThumbnail,
  clearThumbnails: clearVideoThumbnails,
} = useVideoThumbnails();
const { getEmbeddedCover, clearAudioCovers } = useAudioCovers();

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

const groupedEntries = computed(() => {
  const dirs: DirEntry[] = [];
  const files: DirEntry[] = [];

  for (const entry of visibleEntries.value) {
    if (entry.is_dir) {
      // Folders always show: the filter narrows what can be picked, not where one can go.
      dirs.push(entry);
      continue;
    }

    if (entryPassesTypeFilter(entry)) {
      files.push(entry);
    }
  }

  return {
    dirs,
    files,
  };
});

const viewportWidth = ref(0);

const tileColumnCount = computed(() => getGridColumnCount(
  Math.max(0, viewportWidth.value - LISTING_PADDING_X),
  FILE_BROWSER_GRID_GAP_DEFAULT,
));

const sectionLabels = computed<Record<PickerSectionKey, string>>(() => ({
  dirs: t('fileBrowser.folders'),
  files: t('files'),
}));

/**
 * List view is the same geometry with one column and no gutter, so both layouts come out of
 * the navigator's own row builder rather than a second implementation of the arithmetic.
 */
const pickerRows = computed(() => {
  const isList = viewLayout.value === 'list';
  const columnCount = isList ? 1 : tileColumnCount.value;

  return buildSectionedVirtualRows<PickerSectionKey, DirEntry>(
    [
      {
        key: 'dirs',
        items: groupedEntries.value.dirs,
        itemHeight: isList ? rowHeights.value.list : rowHeights.value.tileDir,
        columnCount,
      },
      {
        key: 'files',
        items: groupedEntries.value.files,
        itemHeight: isList ? rowHeights.value.list : rowHeights.value.tileFile,
        columnCount,
      },
    ],
    {
      headerHeight: rowHeights.value.section,
      gap: isList ? 0 : FILE_BROWSER_GRID_GAP_DEFAULT,
    },
  );
});

const virtualList = useVerticalVirtualList({
  items: pickerRows,
  getItemSize: row => row.size,
});
const visibleRows = virtualList.visibleItems;
const listSpacerStyle = virtualList.spacerStyle;
const listWindowStyle = virtualList.windowStyle;

let viewportResizeObserver: ResizeObserver | null = null;

watch(virtualList.scrollViewportRef, (viewport) => {
  viewportResizeObserver?.disconnect();
  viewportResizeObserver = null;

  if (!viewport) {
    viewportWidth.value = 0;
    return;
  }

  viewportWidth.value = viewport.clientWidth;

  if (typeof ResizeObserver !== 'undefined') {
    viewportResizeObserver = new ResizeObserver(() => {
      viewportWidth.value = viewport.clientWidth;
    });
    viewportResizeObserver.observe(viewport);
  }
});

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

async function listDirectory(path: string, recordHistory = true) {
  try {
    const contents = await resolveDirectoryContents(path);
    currentPath.value = path;
    entries.value = contents.entries;
    selectedPaths.value = new Set();
    listingError.value = false;
    isOverwriteArmed.value = false;
    clearImageThumbnails();
    clearVideoThumbnails();
    clearAudioCovers();
    virtualList.setScrollTop(0);
    navigatorIconsStore.prefetchForDirectoryEntries(contents.entries);

    if (recordHistory && pathHistory.value[pathHistoryIndex.value] !== path) {
      pathHistory.value = [...pathHistory.value.slice(0, pathHistoryIndex.value + 1), path];
      pathHistoryIndex.value = pathHistory.value.length - 1;
    }
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

  // The navigator's audio artwork: the cover embedded in the file's tags, if it has one.
  if (isAudioFile(entry)) {
    return getEmbeddedCover(entry);
  }

  return undefined;
}

function tileVariant(entry: DirEntry): 'dir' | 'image' | 'video' | 'other' {
  if (entry.is_dir) return 'dir';
  if (isImageFile(entry)) return 'image';
  if (isVideoFile(entry)) return 'video';
  return 'other';
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

function setViewLayout(layout: PickerViewLayout) {
  if (viewLayout.value === layout) {
    return;
  }

  layoutOverride.value = layout;
  virtualList.setScrollTop(0);
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

function goBack() {
  if (!canGoBack.value) return;
  pathHistoryIndex.value -= 1;
  void listDirectory(pathHistory.value[pathHistoryIndex.value], false);
}

function goForward() {
  if (!canGoForward.value) return;
  pathHistoryIndex.value += 1;
  void listDirectory(pathHistory.value[pathHistoryIndex.value], false);
}

async function goHome() {
  void listDirectory(await homeDir());
}

function refresh() {
  void listDirectory(currentPath.value, false);
}

function openAddressBarEditor() {
  void addressBarEditorRef.value?.open('path');
}

function handleAddressBarNavigate(path: string) {
  void listDirectory(path);
}

/**
 * A file path arriving from the address bar or the path editor. In a picker, naming a file
 * IS the answer; in save mode it donates its name and folder; a directory picker only cares
 * about the folder around it.
 */
function handleAddressBarOpenFile(path: string) {
  if (!request.value?.save && !request.value?.directory) {
    finish([path]);
    return;
  }

  if (request.value?.save) {
    const name = path.split('/').pop() ?? '';

    if (name) {
      fileName.value = name;
      isOverwriteArmed.value = false;
    }
  }

  const parent = getNavigableParentPath(path, platformStore.currentPlatform);

  if (parent) {
    void listDirectory(parent);
  }
}

async function handleAddressBarReveal(parentPath: string, entryPath: string) {
  await listDirectory(parentPath);
  const entry = entries.value.find(candidate => candidate.path === entryPath);

  if (entry && isSelectable(entry)) {
    selectedPaths.value = new Set([entryPath]);
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
  // While the path editor or the type-filter dropdown is up it owns the keyboard: its Escape
  // must not cancel the picker and its Enter must not confirm a selection behind it.
  if (document.querySelector('[role="dialog"][data-state="open"], [role="listbox"][data-state="open"]')) {
    return;
  }

  if (event.key === 'Escape') {
    cancel();
  }

  if (event.key === 'Enter' && canConfirm.value) {
    confirmSelection();
  }

  // The GTK file chooser's shortcuts; they work from the name field there too.
  if (event.ctrlKey && !event.shiftKey && !event.altKey && event.key.toLowerCase() === 'h') {
    event.preventDefault();
    toggleHiddenFiles();
  }

  if (event.ctrlKey && !event.shiftKey && !event.altKey && event.key.toLowerCase() === 'l') {
    event.preventDefault();
    openAddressBarEditor();
  }
}

onMounted(async () => {
  readRowHeights();
  window.addEventListener('keydown', onKeydown);

  request.value = await invoke<PickerRequest | null>('file_picker_request');

  if (!request.value) {
    // Not a picker process; this page has nothing to serve.
    return;
  }

  fileName.value = request.value.suggestedName ?? '';

  const filters = request.value.filters ?? [];
  selectedFilterName.value = filters.some(filter => filter.name === request.value?.currentFilter)
    ? request.value.currentFilter
    : filters[0]?.name ?? null;

  const startingFolder = request.value.currentFolder || await homeDir();
  await listDirectory(startingFolder);

  const currentWindow = getCurrentWindow();
  await currentWindow.setTitle(title.value);
  await currentWindow.show();
  await currentWindow.setFocus();
});

onBeforeUnmount(() => {
  viewportResizeObserver?.disconnect();
  viewportResizeObserver = null;
});
</script>

<template>
  <div
    ref="rootRef"
    class="file-picker"
  >
    <FilePickerInfusion />

    <header
      class="file-picker__header"
      data-tauri-drag-region
    >
      <span class="file-picker__title">{{ title }}</span>
    </header>

    <div class="file-picker__toolbar">
      <FileBrowserToolbarNavButtons
        :can-go-back="canGoBack"
        :can-go-forward="canGoForward"
        :can-go-up="canGoUp"
        :is-loading="false"
        @go-back="goBack"
        @go-forward="goForward"
        @go-up="goUp"
        @go-home="goHome"
        @refresh="refresh"
      />

      <FileBrowserToolbarAddressBar
        :current-path="currentPath"
        @navigate="handleAddressBarNavigate"
        @open-file="handleAddressBarOpenFile"
        @edit="openAddressBarEditor"
      />

      <div
        class="file-picker__layout-toggle"
        role="group"
        :aria-label="t('settings.navigator.navigatorViewLayout')"
      >
        <button
          type="button"
          class="file-picker__layout-option"
          :class="{ 'file-picker__layout-option--active': viewLayout === 'list' }"
          :title="t('list')"
          @click="setViewLayout('list')"
        >
          <ListIcon :size="16" />
        </button>
        <button
          type="button"
          class="file-picker__layout-option"
          :class="{ 'file-picker__layout-option--active': viewLayout === 'grid' }"
          :title="t('grid')"
          @click="setViewLayout('grid')"
        >
          <LayoutGridIcon :size="16" />
        </button>
      </div>

      <label class="file-picker__hidden-toggle">
        <span>{{ t('filter.showHiddenItems') }}</span>
        <Switch
          :model-value="showHiddenFiles"
          @update:model-value="hiddenFilesOverride = $event === true"
        />
      </label>
    </div>

    <div
      v-if="viewLayout === 'list'"
      class="file-picker__columns"
    >
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
            <template
              v-for="row in visibleRows"
              :key="row.item.type === 'section'
                ? `section:${row.item.key}`
                : `${row.item.key}:${row.item.rowIndex}`"
            >
              <div
                v-if="row.item.type === 'section'"
                class="file-picker__section-row"
                :style="{ height: `${row.size}px` }"
              >
                <FileBrowserGridSectionBar
                  :label="sectionLabels[row.item.key]"
                  :count="row.item.count"
                >
                  <template #icon>
                    <component
                      :is="row.item.key === 'dirs' ? FolderIcon : FileIcon"
                      :size="14"
                    />
                  </template>
                </FileBrowserGridSectionBar>
              </div>

              <!-- List view is a one-column section, so its rows arrive here holding a single entry. -->
              <button
                v-else-if="viewLayout === 'list'"
                type="button"
                class="file-picker__entry"
                :style="{ height: `${row.size}px` }"
                :class="{
                  'file-picker__entry--hidden': row.item.items[0].is_hidden,
                  'file-picker__entry--inert': !isSelectable(row.item.items[0]) && !row.item.items[0].is_dir,
                }"
                :data-selected="selectedPaths.has(row.item.items[0].path) || undefined"
                @click="toggleSelection(row.item.items[0], $event)"
                @dblclick="activateEntry(row.item.items[0])"
              >
                <span class="file-picker__entry-preview">
                  <img
                    v-if="getEntryThumbnail(row.item.items[0])"
                    :src="getEntryThumbnail(row.item.items[0])"
                    class="file-picker__entry-thumbnail"
                    alt=""
                    draggable="false"
                  >
                  <FileBrowserEntryIcon
                    v-else
                    :entry="row.item.items[0]"
                    :size="18"
                    class="file-picker__entry-icon"
                    :class="{ 'file-picker__entry-icon--folder': row.item.items[0].is_dir }"
                  />
                </span>
                <span class="file-picker__entry-name">{{ row.item.items[0].name }}</span>
                <span class="file-picker__entry-size">{{ entrySizeText(row.item.items[0]) }}</span>
                <span class="file-picker__entry-modified">{{ entryModifiedText(row.item.items[0]) }}</span>
              </button>

              <div
                v-else
                class="file-picker__tile-row"
                :style="{
                  height: `${row.size}px`,
                  gridTemplateColumns: `repeat(${tileColumnCount}, minmax(0, 1fr))`,
                }"
              >
                <FilePickerTileCard
                  v-for="entry in row.item.items"
                  :key="entry.path"
                  :entry="entry"
                  :variant="tileVariant(entry)"
                  :thumbnail-src="getEntryThumbnail(entry)"
                  :selected="selectedPaths.has(entry.path)"
                  :inert="!isSelectable(entry) && !entry.is_dir"
                  @click="toggleSelection(entry, $event)"
                  @dblclick="activateEntry(entry)"
                />
              </div>
            </template>
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
      <Select
        v-if="request && request.filters.length > 0 && !request.directory"
        :model-value="selectedFilterName ?? undefined"
        @update:model-value="selectedFilterName = ($event as string)"
      >
        <SelectTrigger
          class="file-picker__filter-select"
          :aria-label="t('filePicker.fileType')"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem
            v-for="filter in request.filters"
            :key="filter.name"
            :value="filter.name"
          >
            <SelectItemText>{{ filter.name }}</SelectItemText>
          </SelectItem>
        </SelectContent>
      </Select>
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

    <AddressBarEditorDialog
      ref="addressBarEditorRef"
      :current-path="currentPath"
      @open-directory="handleAddressBarNavigate"
      @open-file="handleAddressBarOpenFile"
      @reveal="handleAddressBarReveal"
    />
    <Toaster />
  </div>
</template>

<style scoped>
/*
  The page mounts as a flex item of #app, so without a pinned width its min-content —
  the longest nowrap filename in the listing — inflates the whole layout and pushes the
  right-aligned footer past the window edge. Pinning width and zeroing min-width keeps
  the page exactly window-sized no matter what the current folder contains.
*/

/*
  A dialog is a window floating over the app that asked for it, so its surfaces sit a step
  darker than the file manager's own — enough separation to read as a panel in front rather
  than more of the same material. One knob shades every surface together, keeping their
  relative order intact; the tile cards read it from here.
*/

.file-picker {
  /*
    Row heights: set them here and nowhere else. The listing is virtualized, so JavaScript
    reads these back (`readRowHeights`) to size the scroll window — a height set only in a
    rule below would move the box and leave the scroll math behind it. Every value is the
    navigator's own: one design language across both browsing surfaces, so the dialog never
    drifts from the app it fronts. The section height mirrors the navigator's
    GRID_SECTION_HEADER_HEIGHT (8px + 2px wrapper padding around its 32px section bar) —
    numeric because no global variable carries it yet.
  */
  --file-picker-list-row-height: var(--navigator-list-view-entry-height);
  --file-picker-section-row-height: 42px;
  --file-picker-tile-dir-height: var(--navigator-grid-view-dir-entry-height);
  --file-picker-tile-file-height: var(--navigator-grid-view-entry-height);
  --file-picker-surface-shade: 35%;
  --file-picker-surface: color-mix(in srgb, black var(--file-picker-surface-shade), hsl(var(--background-3)));
  --file-picker-card-surface: color-mix(in srgb, black var(--file-picker-surface-shade), hsl(var(--background-2)));
  --file-picker-size-column-width: 84px;
  --file-picker-modified-column-width: 150px;

  display: flex;
  overflow: hidden;
  width: 100vw;
  min-width: 0;
  height: 100vh;
  flex-direction: column;
  border: 1px solid hsl(var(--border) / 50%);
  background: var(--file-picker-surface);
  color: hsl(var(--foreground));
}

.file-picker__header {
  display: flex;
  align-items: baseline;
  padding: 12px 16px 8px;
  gap: 12px;
}

.file-picker__title {
  color: hsl(var(--muted-foreground));
  font-size: 14px;
}

.file-picker__toolbar {
  display: flex;
  align-items: center;
  padding: 0 12px 6px;
  border-bottom: 1px solid hsl(var(--border));
  margin: 0 8px 8px;
  color: hsl(var(--icon));
  gap: 8px;
}

.file-picker__layout-toggle {
  display: flex;
  overflow: hidden;
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
}

.file-picker__layout-option {
  display: flex;
  align-items: center;
  padding: 4px 10px;
  border: none;
  background: transparent;
  color: hsl(var(--muted-foreground));
  cursor: default;
}

.file-picker__layout-option:hover {
  background: hsl(var(--foreground) / 5%);
}

.file-picker__layout-option--active {
  background-color: hsl(var(--primary) / 15%);
  color: hsl(var(--primary));
}

.file-picker__layout-option--active:hover {
  background-color: hsl(var(--primary) / 25%);
}

.file-picker__hidden-toggle {
  display: flex;
  align-items: center;
  margin-left: auto;
  color: hsl(var(--muted-foreground));
  cursor: default;
  font-size: 12px;
  gap: 8px;
}

/* The navigator's compact switch sizing, from its Layout menu. */
.file-picker__hidden-toggle :deep(.sigma-ui-switch) {
  width: 1.75rem;
  height: 1rem;
}

.file-picker__hidden-toggle :deep(.sigma-ui-switch__thumb) {
  width: 0.75rem;
  height: 0.75rem;
}

.file-picker__hidden-toggle :deep(.sigma-ui-switch__thumb[data-state="checked"]) {
  transform: translateX(0.75rem);
}

/* Mirrors the row layout (preview spacer + cells) so headers align with their columns. */
.file-picker__columns {
  display: flex;
  align-items: center;
  padding: 0 18px 4px;
  border-bottom: 1px solid hsl(var(--border));
  margin: 0 8px;
  gap: 10px;
  line-height: 1rem;
}

.file-picker__column-button {
  display: flex;
  align-items: center;
  padding: 2px 0;
  border: none;
  background: transparent;
  color: hsl(var(--muted-foreground));
  cursor: default;
  font: inherit;
  font-size: 12px;
  gap: 4px;
  text-transform: uppercase;
}

.file-picker__column-button:hover,
.file-picker__column-button--active {
  color: hsl(var(--foreground));
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
  padding: 0 12px;
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
  color: hsl(var(--muted-foreground));
}

/*
  The navigator's section header geometry, verbatim: 8px above the bar, 2px below, and the
  bar (a `FileBrowserGridSectionBar`, the navigator's own component) in normal flow at the
  top — so in tile view the row's grid-gap slack falls below the bar as the gutter. The row's
  total height is `--file-picker-section-row-height`; the bar is not restyled here at all.
*/

.file-picker__section-row {
  padding: 8px 0 2px;
}

.file-picker__tile-row {
  display: grid;
  align-content: start;
  gap: 12px;
}

.file-picker__entry {
  position: relative;
  display: flex;
  width: 100%;
  align-items: center;
  padding: 0 10px;
  border: none;
  border-radius: 6px;
  border-bottom: 1px solid hsl(var(--border) / 50%);
  background: transparent;
  color: inherit;
  cursor: default;
  font: inherit;
  gap: 10px;
  text-align: left;
}

.file-picker__entry:hover {
  background: hsl(var(--foreground) / 5%);
}

/* The navigator's list selection, verbatim: an accent-tinted layer with a 1px accent ring. */
.file-picker__entry::before {
  position: absolute;
  z-index: 0;
  border-radius: 6px;
  content: "";
  inset: 0;
  opacity: 0;
  pointer-events: none;
}

.file-picker__entry[data-selected]::before {
  background-color: hsl(var(--primary) / 12%);
  box-shadow: inset 0 0 0 1px hsl(var(--primary) / 40%);
  opacity: 1;
}

.file-picker__entry--hidden {
  opacity: 0.5;
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
  color: hsl(var(--muted-foreground));
}

/* Folder icons carry the accent, same as the navigator's views. */
.file-picker__entry-icon--folder {
  color: hsl(var(--primary));
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
  color: hsl(var(--muted-foreground));
  font-size: 12px;
  text-align: right;
  white-space: nowrap;
}

.file-picker__entry-modified {
  overflow: hidden;
  width: var(--file-picker-modified-column-width);
  flex: none;
  color: hsl(var(--muted-foreground));
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
  border: 1px solid hsl(var(--border));
  border-radius: 6px;
  background: hsl(var(--foreground) / 5%);
  color: inherit;
  font: inherit;
  outline: none;
}

.file-picker__name-input:focus {
  border-color: hsl(var(--primary) / 60%);
}

.file-picker__overwrite-warning {
  flex: none;
  color: #f0a30a;
  font-size: 12px;
}

.file-picker__filter-select {
  max-width: 260px;
  margin-right: auto;
}

.file-picker__actions {
  display: flex;
  justify-content: flex-end;
  padding: 4px;
  border-radius: var(--radius-lg);
  margin: 8px;
  background-color: hsl(var(--background-3));
  color: hsl(var(--muted-foreground));
  gap: 8px;
}
</style>
