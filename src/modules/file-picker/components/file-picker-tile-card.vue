<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<!--
  One tile in the picker's tile view, shaped like the navigator's grid cards: directories as
  a compact icon-and-name row, files as a preview card with the name over the artwork. Kept
  free of the navigator's context on purpose — the picker page hands in everything.
-->

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import FileBrowserEntryIcon from '@/modules/navigator/components/file-browser/file-browser-entry-icon.vue';
import { formatBytes } from '@/modules/navigator/components/file-browser/utils';
import type { DirEntry } from '@/types/dir-entry';

const props = defineProps<{
  entry: DirEntry;
  variant: 'dir' | 'image' | 'video' | 'other';
  thumbnailSrc?: string;
  selected: boolean;
  inert: boolean;
}>();

// Any non-directory card with artwork goes full-bleed — the navigator's rule, which is how
// an audio file's embedded cover renders on an 'other' card there.
const { t } = useI18n();

const showsArtwork = computed(() => !!props.thumbnailSrc && props.variant !== 'dir');

const metaText = computed(() => {
  if (props.entry.is_dir) {
    return '';
  }

  const extension = props.entry.ext?.toUpperCase();
  const size = formatBytes(Number(props.entry.size) || 0);
  return extension ? `${extension} · ${size}` : size;
});
</script>

<template>
  <button
    type="button"
    class="file-picker-tile-card"
    :class="{
      [`file-picker-tile-card--${variant}`]: true,
      'file-picker-tile-card--artwork': showsArtwork,
      'file-picker-tile-card--hidden': entry.is_hidden,
      'file-picker-tile-card--unresponsive': entry.mount_status === 'unresponsive',
      'file-picker-tile-card--inert': inert,
    }"
    :title="entry.mount_status === 'unresponsive' ? t('fileBrowser.storageNotResponding') : undefined"
    :data-mount-status="entry.mount_status || undefined"
    :data-selected="selected || undefined"
  >
    <template v-if="variant === 'dir'">
      <FileBrowserEntryIcon
        :entry="entry"
        :size="24"
        class="file-picker-tile-card__dir-icon file-picker-tile-card__icon--folder"
      />
      <span class="file-picker-tile-card__name">{{ entry.name }}</span>
    </template>

    <template v-else>
      <span
        class="file-picker-tile-card__preview"
        :class="{ 'file-picker-tile-card__preview--icon': !showsArtwork }"
      >
        <img
          v-if="showsArtwork"
          :src="thumbnailSrc"
          class="file-picker-tile-card__image"
          alt=""
          draggable="false"
        >
        <FileBrowserEntryIcon
          v-else
          :entry="entry"
          :size="48"
          class="file-picker-tile-card__file-icon"
        />
      </span>
      <span
        class="file-picker-tile-card__info"
        :class="{ 'file-picker-tile-card__info--overlay': showsArtwork }"
      >
        <span class="file-picker-tile-card__name">{{ entry.name }}</span>
        <span class="file-picker-tile-card__meta">{{ metaText }}</span>
      </span>
    </template>

    <!-- The navigator's overlay stack: selection and hover paint above any artwork. -->
    <span class="file-picker-tile-card__overlay-container">
      <span class="file-picker-tile-card__overlay file-picker-tile-card__overlay--selected" />
      <span class="file-picker-tile-card__overlay file-picker-tile-card__overlay--hover" />
    </span>
  </button>
</template>

<style scoped>
.file-picker-tile-card {
  position: relative;
  display: flex;
  overflow: hidden;

  /* Both heights come from the page root so the virtual list agrees; see file-picker.vue. */
  height: var(--file-picker-tile-file-height, var(--navigator-grid-view-entry-height));
  flex-direction: column;
  padding: 0;
  border: 1px solid hsl(var(--border));
  border-radius: 8px;

  /* Shaded with the rest of the dialog; see `--file-picker-surface-shade` on the page root. */
  background: var(--file-picker-card-surface, hsl(var(--background-2)));
  color: inherit;
  cursor: default;
  font: inherit;
  text-align: start;
  user-select: none;
}

/* The navigator's grid-card overlays, verbatim: radius one inside the card's 8px border. */
.file-picker-tile-card__overlay-container {
  position: absolute;
  z-index: 3;
  inset: 0;
  pointer-events: none;
}

.file-picker-tile-card__overlay {
  position: absolute;
  border-radius: 7px;
  inset: 0;
  pointer-events: none;
}

.file-picker-tile-card__overlay--selected {
  background-color: hsl(var(--secondary) / 20%);
  box-shadow: inset 0 0 0 2px hsl(var(--primary) / 60%);
  opacity: 0;
}

.file-picker-tile-card[data-selected] .file-picker-tile-card__overlay--selected {
  opacity: 1;
}

.file-picker-tile-card--artwork[data-selected] .file-picker-tile-card__overlay--selected {
  background-color: hsl(var(--secondary) / 50%);
}

.file-picker-tile-card__overlay--hover {
  background-color: hsl(var(--foreground) / 5%);
  opacity: 0;
  transition: opacity var(--hover-transition-duration-out) var(--hover-transition-easing-out);
}

.file-picker-tile-card:hover .file-picker-tile-card__overlay--hover {
  opacity: 1;
  transition: opacity var(--hover-transition-duration-in);
}

.file-picker-tile-card--hidden {
  opacity: 0.5;
}

.file-picker-tile-card--unresponsive {
  opacity: 0.5;
}

.file-picker-tile-card--unresponsive .file-picker-tile-card__dir-icon {
  filter: grayscale(1);
}

.file-picker-tile-card--inert {
  opacity: 0.45;
}

.file-picker-tile-card--dir {
  height: var(--file-picker-tile-dir-height, var(--navigator-grid-view-dir-entry-height));
  flex-direction: row;
  align-items: center;
  padding: 8px 12px;
  gap: 10px;
}

.file-picker-tile-card__dir-icon {
  flex: none;
}

/* Folder icons carry the accent, non-media file icons stay muted — the navigator's scheme. */
.file-picker-tile-card__icon--folder {
  color: hsl(var(--primary));
}

.file-picker-tile-card__file-icon {
  color: hsl(var(--muted-foreground));
}

.file-picker-tile-card__preview {
  z-index: 1;
  display: flex;
  width: 100%;
  height: 100%;
  align-items: center;
  justify-content: center;
}

/* No artwork: the icon sits top-left clear of the name block, like the navigator's cards. */
.file-picker-tile-card__preview--icon {
  align-items: flex-start;
  justify-content: flex-start;
  padding: 8px;
}

.file-picker-tile-card__image {
  position: absolute;
  width: 100%;
  height: 100%;
  inset: 0;
  object-fit: cover;
}

.file-picker-tile-card__info {
  position: absolute;
  z-index: 2;
  bottom: 0;
  display: flex;
  flex-direction: column;
  padding: 8px 10px;
  gap: 2px;
  inset-inline: 0;
}

.file-picker-tile-card__info--overlay {
  background: linear-gradient(to top, hsl(0deg 0% 0% / 80%) 0%, transparent 100%);
  color: white;
}

.file-picker-tile-card__name {
  overflow: hidden;
  font-size: 13px;
  font-weight: 500;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-picker-tile-card__meta {
  font-size: 11px;
  opacity: 0.8;
}
</style>
