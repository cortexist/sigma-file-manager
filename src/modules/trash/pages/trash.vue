<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<!--
  What deleting a file did, and how to undo it.

  Deleting has always gone to the system trash rather than removing anything, but there was no
  way to see what was in there or put any of it back — so a deletion was reversible in principle
  and irreversible in practice, from inside this app. This is the other half.

  The listing cannot come from a directory read. A trashed file may have been renamed on its way
  in to avoid a collision, and where it came from lives in a sidecar file beside it, so the trash
  is read through the backend's `trash_*` commands and each item is addressed by the id they
  hand out. Everything an item shows about itself — its icon, the folder it will go back to — is
  derived from its *original* path, because that is the file a person recognises.
-->

<script setup lang="ts">
import { computed, markRaw, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import {
  RefreshCwIcon,
  RotateCcwIcon,
  Trash2Icon,
} from '@lucide/vue';
import { PageDefaultLayout } from '@/layouts';
import { Button } from '@/components/ui/button';
import { EmptyState } from '@/components/ui/empty-state';
import { toast, ToastStatic } from '@/components/ui/toaster';
import SearchTrashIcon from '@/components/icons/search-trash-icon.vue';
import FileBrowserEntryIcon from '@/modules/navigator/components/file-browser/file-browser-entry-icon.vue';
import PermanentDeleteConfirmDialog from '@/modules/navigator/components/file-browser/permanent-delete-confirm-dialog.vue';
import { formatBytes, formatDate } from '@/modules/navigator/components/file-browser/utils';
import { usePermanentDeleteConfirm } from '@/composables/use-permanent-delete-confirm';
import { getPathDisplayValue } from '@/utils/normalize-path';
import type { DirEntry } from '@/types/dir-entry';

interface TrashEntry {
  id: string;
  name: string;
  originalPath: string;
  originalParent: string;
  deletedTime: number;
  size: number;
  isDir: boolean;
  itemCount: number | null;
}

interface TrashEntrySize {
  id: string;
  size: number;
}

interface TrashOperationResult {
  success: boolean;
  completedCount: number;
  failedCount: number;
  error: string | null;
}

const { t } = useI18n();
const permanentDeleteConfirm = usePermanentDeleteConfirm();

const entries = ref<TrashEntry[]>([]);
const selectedIds = ref<Set<string>>(new Set());
const isLoading = ref(true);
const isWorking = ref(false);
const loadError = ref<string | null>(null);
const isListable = ref(true);
/** The row a range selection measures from, as in any list that supports shift-click. */
let selectionAnchorIndex: number | null = null;

/**
 * Real size per item, which only arrives once the folders have been walked. Empty until then,
 * so a total is offered when it can be trusted rather than counting the files and quietly
 * leaving out the folders — which is where nearly all of the space usually is.
 */
const sizesById = ref<Map<string, number>>(new Map());
/** Guards against a measurement for a listing that has since been replaced. */
let sizingGeneration = 0;

const selectedEntries = computed(() => entries.value.filter(entry => selectedIds.value.has(entry.id)));
const hasSelection = computed(() => selectedEntries.value.length > 0);

/** What the count and size line describes: the selection, or the whole trash. */
const summarizedEntries = computed(() => (hasSelection.value ? selectedEntries.value : entries.value));

const summarizedSize = computed(() => {
  if (sizesById.value.size === 0) {
    return null;
  }

  let total = 0;

  for (const entry of summarizedEntries.value) {
    const size = sizesById.value.get(entry.id);

    // One unmeasured item makes the total a guess, and a guess about disk space is worse than
    // waiting a moment for the real number.
    if (size === undefined) {
      return null;
    }

    total += size;
  }

  return total;
});

const summaryText = computed(() => {
  const count = hasSelection.value
    ? t('fileBrowser.selectedItems', { count: summarizedEntries.value.length })
    : t('item', entries.value.length);

  return summarizedSize.value === null
    ? count
    : `${count} · ${formatBytes(summarizedSize.value)}`;
});

/**
 * The icon pipeline reads a `DirEntry`, and it only ever touches these fields. Built from the
 * original path so a trashed file keeps the icon it had where it used to live, rather than
 * whatever its name inside the trash directory would suggest.
 */
function entryForIcon(entry: TrashEntry): DirEntry {
  const extension = entry.isDir ? null : (entry.name.split('.').pop() ?? null);

  return {
    path: entry.originalPath,
    name: entry.name,
    is_dir: entry.isDir,
    ext: extension === entry.name ? null : extension,
  } as DirEntry;
}

function entrySizeText(entry: TrashEntry): string {
  const measured = sizesById.value.get(entry.id);

  if (measured !== undefined) {
    return formatBytes(measured);
  }

  // Until the walk reaches it, a folder can only say how much it directly holds.
  if (entry.isDir) {
    return entry.itemCount === null ? '' : t('item', entry.itemCount);
  }

  return formatBytes(entry.size);
}

/**
 * Measures what is in the trash, after it has been listed.
 *
 * Deliberately not awaited by the load: walking a trashed build directory can take a moment,
 * and the listing is useful long before the sizes are. Failure is silent — the rows still show
 * what the trash API reported, and a total simply is not offered.
 */
async function loadSizes() {
  const generation = ++sizingGeneration;

  try {
    const sizes = await invoke<TrashEntrySize[]>('trash_sizes');

    if (generation === sizingGeneration) {
      sizesById.value = new Map(sizes.map(entry => [entry.id, entry.size]));
    }
  }
  catch {
    if (generation === sizingGeneration) {
      sizesById.value = new Map();
    }
  }
}

async function loadTrash() {
  isLoading.value = true;

  try {
    isListable.value = await invoke<boolean>('trash_is_listable');
    entries.value = await invoke<TrashEntry[]>('trash_list');
    loadError.value = null;
  }
  catch (error) {
    entries.value = [];
    loadError.value = error instanceof Error ? error.message : String(error);
  }
  finally {
    // A listing that no longer holds an item cannot keep it selected.
    const present = new Set(entries.value.map(entry => entry.id));
    selectedIds.value = new Set([...selectedIds.value].filter(id => present.has(id)));
    selectionAnchorIndex = null;
    isLoading.value = false;
    sizesById.value = new Map();
    void loadSizes();
  }
}

function selectRangeTo(index: number) {
  const from = selectionAnchorIndex ?? index;
  const [start, end] = from <= index ? [from, index] : [index, from];
  selectedIds.value = new Set(entries.value.slice(start, end + 1).map(entry => entry.id));
}

function handleRowClick(entry: TrashEntry, index: number, event: MouseEvent) {
  if (event.shiftKey && selectionAnchorIndex !== null) {
    selectRangeTo(index);
    return;
  }

  if (event.ctrlKey || event.metaKey) {
    const next = new Set(selectedIds.value);

    if (next.has(entry.id)) {
      next.delete(entry.id);
    }
    else {
      next.add(entry.id);
    }

    selectedIds.value = next;
    selectionAnchorIndex = index;
    return;
  }

  const isOnlySelection = selectedIds.value.has(entry.id) && selectedIds.value.size === 1;
  selectedIds.value = isOnlySelection ? new Set() : new Set([entry.id]);
  selectionAnchorIndex = isOnlySelection ? null : index;
}

function selectAll() {
  selectedIds.value = new Set(entries.value.map(entry => entry.id));
}

function clearSelection() {
  selectedIds.value = new Set();
  selectionAnchorIndex = null;
}

function report(title: string, description = '') {
  toast.custom(markRaw(ToastStatic), {
    componentProps: {
      data: {
        title,
        description,
      },
    },
  });
}

/**
 * Every operation reports what it managed rather than whether it succeeded outright. Restoring
 * a selection can partly fail — a file may have reappeared at an original path in the meantime —
 * and saying "failed" over five items that came back and one that did not would be wrong.
 */
function reportOutcome(result: TrashOperationResult, completedTitle: string) {
  if (result.completedCount > 0) {
    report(completedTitle, result.failedCount > 0 ? (result.error ?? '') : '');
    return;
  }

  report(t('trash.operationFailed'), result.error ?? '');
}

async function runOperation(operation: () => Promise<TrashOperationResult>, completedTitle: (count: number) => string) {
  if (isWorking.value) return;

  isWorking.value = true;

  try {
    const result = await operation();
    reportOutcome(result, completedTitle(result.completedCount));
  }
  catch (error) {
    report(t('trash.operationFailed'), error instanceof Error ? error.message : String(error));
  }
  finally {
    isWorking.value = false;
    await loadTrash();
  }
}

function restoreSelected() {
  const ids = selectedEntries.value.map(entry => entry.id);

  void runOperation(
    () => invoke<TrashOperationResult>('trash_restore', { ids }),
    count => t('trash.restoredItems', count),
  );
}

/**
 * The same confirmation the navigator asks for before a permanent delete, given the items by
 * their original paths — the wording is about what is being destroyed, and that is the file the
 * person deleted, not its name inside the trash.
 */
async function purgeSelected() {
  const targets = selectedEntries.value;

  if (targets.length === 0) return;

  const confirmed = await permanentDeleteConfirm.requestConfirm(targets.map(entryForIcon));

  if (!confirmed) return;

  const ids = targets.map(entry => entry.id);

  void runOperation(
    () => invoke<TrashOperationResult>('trash_purge', { ids }),
    count => t('trash.purgedItems', count),
  );
}

async function emptyTrash() {
  if (entries.value.length === 0) return;

  const confirmed = await permanentDeleteConfirm.requestConfirm(entries.value.map(entryForIcon));

  if (!confirmed) return;

  void runOperation(
    () => invoke<TrashOperationResult>('trash_empty'),
    count => t('trash.purgedItems', count),
  );
}

onMounted(loadTrash);
</script>

<template>
  <PageDefaultLayout
    :title="t('pages.trash')"
    :subtitle="t('trash.subtitle')"
    class="trash-page"
  >
    <div class="trash-page__toolbar">
      <span class="trash-page__count">{{ summaryText }}</span>

      <div class="trash-page__actions">
        <Button
          v-if="hasSelection"
          variant="ghost"
          size="sm"
          :disabled="isWorking"
          @click="restoreSelected"
        >
          <RotateCcwIcon :size="16" />
          {{ t('trash.restore') }}
        </Button>
        <Button
          v-if="hasSelection"
          variant="ghost"
          size="sm"
          class="trash-page__destructive"
          :disabled="isWorking"
          @click="purgeSelected"
        >
          <Trash2Icon :size="16" />
          {{ t('trash.deletePermanently') }}
        </Button>
        <Button
          v-if="hasSelection"
          variant="ghost"
          size="sm"
          @click="clearSelection"
        >
          {{ t('fileBrowser.deselectAll') }}
        </Button>
        <Button
          v-else-if="entries.length > 0"
          variant="ghost"
          size="sm"
          @click="selectAll"
        >
          {{ t('fileBrowser.selectAll') }}
        </Button>

        <Button
          variant="ghost"
          size="sm"
          :disabled="isLoading || isWorking"
          @click="loadTrash"
        >
          <RefreshCwIcon :size="16" />
          {{ t('fileBrowser.refresh') }}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          class="trash-page__destructive"
          :disabled="entries.length === 0 || isWorking"
          @click="emptyTrash"
        >
          {{ t('trash.emptyTrash') }}
        </Button>
      </div>
    </div>

    <EmptyState
      v-if="!isListable"
      :icon="SearchTrashIcon"
      :title="t('trash.unsupported')"
      :description="t('trash.unsupportedDescription')"
      bordered
    />
    <EmptyState
      v-else-if="loadError"
      :icon="SearchTrashIcon"
      :title="t('trash.loadFailed')"
      :description="loadError"
      bordered
    />
    <EmptyState
      v-else-if="!isLoading && entries.length === 0"
      :icon="SearchTrashIcon"
      :title="t('trash.empty')"
      :description="t('trash.emptyDescription')"
      bordered
    />

    <div
      v-else-if="!isLoading"
      class="trash-page__list"
    >
      <div class="trash-page__columns">
        <span class="trash-page__column trash-page__column--name">{{ t('name') }}</span>
        <span class="trash-page__column trash-page__column--origin">{{ t('trash.originalLocation') }}</span>
        <span class="trash-page__column trash-page__column--deleted">{{ t('trash.deleted') }}</span>
        <span class="trash-page__column trash-page__column--size">{{ t('size') }}</span>
      </div>

      <button
        v-for="(entry, index) in entries"
        :key="entry.id"
        type="button"
        class="trash-page__row"
        :data-selected="selectedIds.has(entry.id) || undefined"
        @click="handleRowClick(entry, index, $event)"
      >
        <FileBrowserEntryIcon
          :entry="entryForIcon(entry)"
          :size="18"
          class="trash-page__row-icon"
          :class="{ 'trash-page__row-icon--folder': entry.isDir }"
        />
        <span class="trash-page__row-name">{{ entry.name }}</span>
        <span
          class="trash-page__row-origin"
          :title="entry.originalPath"
        >{{ getPathDisplayValue(entry.originalParent) }}</span>
        <span class="trash-page__row-deleted">{{ formatDate(entry.deletedTime) }}</span>
        <span class="trash-page__row-size">{{ entrySizeText(entry) }}</span>
      </button>
    </div>

    <PermanentDeleteConfirmDialog
      :open="permanentDeleteConfirm.isOpen.value"
      :entries="permanentDeleteConfirm.pendingEntries.value"
      @update:open="permanentDeleteConfirm.handleOpenChange"
      @confirm="permanentDeleteConfirm.handleConfirm"
    />
  </PageDefaultLayout>
</template>

<style>
.trash-page__toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.trash-page__count {
  color: hsl(var(--muted-foreground));
  font-size: 13px;
}

.trash-page__actions {
  display: flex;
  align-items: center;
  gap: 4px;
}

.trash-page__destructive {
  color: hsl(var(--destructive));
}

.trash-page__list {
  display: flex;
  flex-direction: column;
}

/* Mirrors the row grid below, so the headings sit over their own columns. */
.trash-page__columns,
.trash-page__row {
  display: grid;
  align-items: center;
  gap: 12px;
  grid-template-columns: 24px minmax(0, 1fr) minmax(0, 1fr) 150px 90px;
}

.trash-page__columns {
  padding: 0 10px 6px;
  border-bottom: 1px solid hsl(var(--border));
  color: hsl(var(--muted-foreground));
  font-size: 12px;
  text-transform: uppercase;
}

.trash-page__column--name {
  grid-column: 2;
}

.trash-page__column--size {
  text-align: right;
}

.trash-page__row {
  position: relative;
  width: 100%;
  min-height: var(--navigator-list-view-entry-height, 42px);
  padding: 0 10px;
  border: none;
  border-radius: var(--radius-sm);
  border-bottom: 1px solid hsl(var(--border) / 50%);
  background: transparent;
  color: inherit;
  cursor: default;
  font: inherit;
  text-align: left;
}

.trash-page__row:hover {
  background: hsl(var(--foreground) / 5%);
}

/* The navigator's list selection, verbatim: an accent-tinted layer with a 1px accent ring. */
.trash-page__row[data-selected] {
  background-color: hsl(var(--primary) / 12%);
  box-shadow: inset 0 0 0 1px hsl(var(--primary) / 40%);
}

.trash-page__row-icon {
  color: hsl(var(--muted-foreground));
}

.trash-page__row-icon--folder {
  color: hsl(var(--primary));
}

.trash-page__row-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.trash-page__row-origin,
.trash-page__row-deleted,
.trash-page__row-size {
  overflow: hidden;
  color: hsl(var(--muted-foreground));
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.trash-page__row-size {
  text-align: right;
}
</style>
