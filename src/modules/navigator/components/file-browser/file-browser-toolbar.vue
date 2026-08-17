<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import FileBrowserToolbarAddressBar from './file-browser-toolbar-address-bar.vue';
import FileBrowserToolbarCreateButton from './file-browser-toolbar-create-button.vue';
import FileBrowserToolbarFilter from './file-browser-toolbar-filter.vue';

withDefaults(defineProps<{
  pathInput: string;
  filterQuery: string;
  isFilterOpen: boolean;
  focusFilterInput: boolean;
  isSplitView?: boolean;
  isRecursiveSearchRunning?: boolean;
  isRecursiveSearchTruncated?: boolean;
  recursiveSearchError?: string | null;
}>(), {
  isSplitView: false,
  isRecursiveSearchRunning: false,
  isRecursiveSearchTruncated: false,
  recursiveSearchError: null,
});

const emit = defineEmits<{
  (event: 'update:pathInput', value: string): void;
  (event: 'update:filterQuery', value: string): void;
  (event: 'update:isFilterOpen', value: boolean): void;
  (event: 'submitPath'): void;
  (event: 'navigateTo', path: string): void;
  (event: 'openFile', path: string): void;
  (event: 'openAddressEditor'): void;
  (event: 'createNewDirectory'): void;
  (event: 'createNewFile'): void;
  (event: 'filterInputFocused'): void;
}>();

function handleAddressBarNavigate(path: string) {
  emit('update:pathInput', path);
  emit('navigateTo', path);
}
</script>

<template>
  <div class="file-browser-toolbar">
    <div
      class="file-browser-toolbar__layout"
      :class="{ 'file-browser-toolbar__layout--split-view': isSplitView }"
    >
      <FileBrowserToolbarAddressBar
        class="file-browser-toolbar__address-bar"
        :current-path="pathInput"
        @navigate="handleAddressBarNavigate"
        @open-file="emit('openFile', $event)"
        @edit="emit('openAddressEditor')"
      />

      <div class="file-browser-toolbar__actions">
        <div class="file-browser-toolbar__create-button">
          <FileBrowserToolbarCreateButton
            @create-new-directory="emit('createNewDirectory')"
            @create-new-file="emit('createNewFile')"
          />
        </div>
        <FileBrowserToolbarFilter
          :filter-query="filterQuery"
          :is-recursive-search-running="isRecursiveSearchRunning"
          :is-recursive-search-truncated="isRecursiveSearchTruncated"
          :recursive-search-error="recursiveSearchError"
          :is-filter-open="isFilterOpen"
          :focus-input="focusFilterInput"
          @update:filter-query="emit('update:filterQuery', $event)"
          @update:is-filter-open="emit('update:isFilterOpen', $event)"
          @filter-input-focused="emit('filterInputFocused')"
        />
      </div>
    </div>
  </div>
</template>

<style scoped>
.file-browser-toolbar {
  position: relative;
  z-index: 10;
  border-bottom: 1px solid hsl(var(--border));
  container-type: inline-size;
}

.file-browser-toolbar__layout {
  display: flex;
  height: 48px;
  align-items: center;
  padding: 8px;
  gap: 12px;
}

.file-browser-toolbar__address-bar {
  min-width: 0;
  flex: 1;
}

.file-browser-toolbar__actions {
  display: flex;
  flex-shrink: 0;
  align-items: center;
  gap: 4px;
}

@container (width < 300px) {
  .file-browser-toolbar__layout {
    height: auto;
    min-height: 48px;
    flex-wrap: wrap;
    gap: 4px 12px;
  }

  .file-browser-toolbar__address-bar {
    flex-basis: 100%;
    order: 2;
  }

  .file-browser-toolbar__actions {
    margin-inline-start: auto;
  }
}
</style>
