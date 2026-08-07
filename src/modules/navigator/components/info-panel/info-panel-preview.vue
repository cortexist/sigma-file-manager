<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import {
  FolderIcon,
  FolderOpenIcon,
  FileIcon,
  Loader2Icon,
} from '@lucide/vue';
import { useInfoPanelImagePreview } from '@/modules/navigator/components/info-panel/composables/use-info-panel-image-preview';
import { useInfoPanelVideoPreview } from '@/modules/navigator/components/info-panel/composables/use-info-panel-video-preview';
import { MediaPlayer } from '@/components/ui/media-player';
import { ImageViewer } from '@/components/ui/image-viewer';
import { useAudioCovers } from '@/composables/use-audio-covers';
import { useArtistShow } from '@/composables/use-artist-show';
import UbuntuWslIcon from '@/components/icons/ubuntu-wsl-icon.vue';
import { ScrollArea } from '@/components/ui/scroll-area';
import { isWslPath } from '@/utils/normalize-path';
import type { DirEntry } from '@/types/dir-entry';
import { determineFileType } from '@/stores/runtime/quick-view';
import { decodeTextFileBytesWithEncoding } from '@/utils/decode-text-file-bytes';

const { t } = useI18n();

interface ReadTextPreviewResult {
  bytes: number[];
  truncated: boolean;
}

const INFO_PANEL_TEXT_PREVIEW_MAX_BYTES = 48 * 1024;

const props = defineProps<{
  selectedEntry: DirEntry | null;
  isCurrentDir?: boolean;
}>();

const {
  previewRef,
  isImageFile,
  mediaSrc,
  playableMediaSrc,
  imageOriginalSrc,
  imageThumbnailSrc,
  imagePreviewPlaceholderSrc,
} = useInfoPanelImagePreview(() => props.selectedEntry);

const {
  isVideoFile,
  muteVideoPreviewByDefault,
  autoplayVideoPreview,
} = useInfoPanelVideoPreview(() => props.selectedEntry);

const audioCovers = useAudioCovers();
const artistShow = useArtistShow();

const textPreviewContent = ref('');
const textPreviewLoading = ref(false);
const textPreviewFailed = ref(false);
let textPreviewRequestSequence = 0;

const infoPanelPreviewKind = computed(() => {
  const entry = props.selectedEntry;

  if (!entry?.path || entry.is_dir) {
    return null;
  }

  return determineFileType(entry.path);
});

/**
 * Same order Quick View uses: the picture embedded in the track, then a cover file beside
 * it. Nothing here means the player falls back to its music glyph.
 */
const audioArtworkSrc = computed((): string | undefined => {
  const entry = props.selectedEntry;

  if (!entry?.path || infoPanelPreviewKind.value !== 'audio') {
    return undefined;
  }

  void audioCovers.embeddedCovers.value;
  void audioCovers.siblingCovers.value;

  const embedded = audioCovers.getEmbeddedCover(entry);

  if (embedded) {
    return embedded;
  }

  return audioCovers.getSiblingCover(entry.path);
});

/**
 * Resolved as soon as an audio file is selected rather than when the player goes fullscreen,
 * so the material is already in hand by the time the idle countdown runs out.
 */
watch(
  [() => props.selectedEntry?.path, infoPanelPreviewKind],
  ([path, kind]) => {
    void artistShow.load(kind === 'audio' ? path : null);
  },
  { immediate: true },
);

const showWslDirectoryIcon = computed(() => {
  if (!props.selectedEntry?.is_dir) return false;

  return isWslPath(props.selectedEntry.path);
});

watch(
  () => props.selectedEntry?.path,
  async () => {
    const entryAtStart = props.selectedEntry;
    const pathAtStart = entryAtStart?.path;

    textPreviewContent.value = '';
    textPreviewFailed.value = false;
    textPreviewLoading.value = false;

    if (!pathAtStart || entryAtStart?.is_dir) {
      return;
    }

    await nextTick();

    if (props.selectedEntry?.path !== pathAtStart) {
      return;
    }

    const previewKind = determineFileType(pathAtStart);

    if (previewKind === 'text') {
      textPreviewLoading.value = true;
      const requestSequence = ++textPreviewRequestSequence;

      try {
        const preview = await invoke<ReadTextPreviewResult>('read_text_preview', {
          path: pathAtStart,
          maxBytes: INFO_PANEL_TEXT_PREVIEW_MAX_BYTES,
        });

        if (requestSequence !== textPreviewRequestSequence || props.selectedEntry?.path !== pathAtStart) {
          return;
        }

        const bytes = new Uint8Array(preview.bytes);
        const { text } = decodeTextFileBytesWithEncoding(bytes);
        textPreviewContent.value = preview.truncated ? `${text}\n...` : text;
      }
      catch {
        if (requestSequence !== textPreviewRequestSequence || props.selectedEntry?.path !== pathAtStart) {
          return;
        }

        textPreviewFailed.value = true;
      }
      finally {
        if (requestSequence === textPreviewRequestSequence && props.selectedEntry?.path === pathAtStart) {
          textPreviewLoading.value = false;
        }
      }
    }
  },
  { immediate: true },
);
</script>

<template>
  <div class="info-panel-preview">
    <div
      v-if="!selectedEntry"
      class="info-panel-preview__placeholder"
    >
      <FileIcon :size="48" />
    </div>
    <div
      v-else-if="selectedEntry.is_dir"
      class="info-panel-preview__placeholder"
    >
      <UbuntuWslIcon
        v-if="showWslDirectoryIcon"
        :size="48"
        class="info-panel-preview__icon--folder"
      />
      <component
        v-else
        :is="isCurrentDir ? FolderOpenIcon : FolderIcon"
        :size="48"
        class="info-panel-preview__icon--folder"
      />
    </div>
    <div
      v-else-if="isImageFile"
      ref="previewRef"
      class="info-panel-preview__media-container"
    >
      <!-- Not keyed on the path: this element is the one `requestFullscreen` is called on, and
           remounting it when the selection changes dropped the viewer out of fullscreen. It
           resets its own zoom and pan when `src` changes. -->
      <ImageViewer
        :src="imageOriginalSrc"
        :preview-src="imageThumbnailSrc"
        :placeholder-src="imagePreviewPlaceholderSrc"
        :alt="selectedEntry.name"
        class="info-panel-preview__image-viewer animate-fade-in-x2"
      />
    </div>
    <div
      v-else-if="isVideoFile"
      class="info-panel-preview__media-container"
    >
      <!-- Unkeyed for the same reason as the image viewer above. Switching the autoplay
           setting on used to need a remount to take effect; the player now watches the prop
           and starts playing itself, so the instance can persist across the selection. -->
      <MediaPlayer
        :src="playableMediaSrc"
        kind="video"
        :autoplay="autoplayVideoPreview"
        :muted="muteVideoPreviewByDefault"
        class="info-panel-preview__video animate-fade-in-x2"
      />
    </div>
    <div
      v-else-if="infoPanelPreviewKind === 'audio'"
      class="info-panel-preview__media-container"
    >
      <MediaPlayer
        :src="playableMediaSrc"
        kind="audio"
        :poster="audioArtworkSrc"
        :now-playing="artistShow.show.value"
        class="info-panel-preview__audio animate-fade-in-x2"
      />
    </div>
    <div
      v-else-if="infoPanelPreviewKind === 'pdf'"
      class="info-panel-preview__media-container"
    >
      <iframe
        :src="mediaSrc"
        :title="t('fileBrowser.pdfPreviewFrameTitle')"
        class="info-panel-preview__pdf animate-fade-in-x2"
      />
    </div>
    <div
      v-else-if="infoPanelPreviewKind === 'text'"
      class="info-panel-preview__text-shell animate-fade-in-x2"
    >
      <div
        v-if="textPreviewLoading"
        class="info-panel-preview__text-status"
      >
        <Loader2Icon
          :size="32"
          class="info-panel-preview__spinner"
        />
      </div>
      <div
        v-else-if="textPreviewFailed"
        class="info-panel-preview__text-status"
      >
        <FileIcon :size="48" />
      </div>
      <ScrollArea
        v-else
        class="info-panel-preview__text-scroll"
      >
        <div class="info-panel-preview__text-body">
          {{ textPreviewContent }}
        </div>
      </ScrollArea>
    </div>
    <div
      v-else
      class="info-panel-preview__placeholder animate-fade-in-x2"
    >
      <FileIcon :size="48" />
    </div>
  </div>
</template>

<style scoped>
.info-panel-preview {
  display: flex;
  overflow: hidden;
  width: 100%;
  height: 100%;
  min-height: 0;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-sm);
  background-color: hsl(var(--background-3));
}

.info-panel-preview__placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
  color: hsl(var(--muted-foreground) / 30%);
}

.info-panel-preview__icon--folder {
  color: hsl(var(--primary) / 50%);
}

.info-panel-preview__media-container {
  position: relative;
  display: flex;
  overflow: hidden;
  width: 100%;
  height: 100%;
  align-items: center;
  justify-content: center;
}

.info-panel-preview__video {
  width: 100%;
  height: 100%;
  border-radius: var(--radius-sm);
  object-fit: cover;
}

/* `contain` rather than the `cover` this pane used before: a viewer that can be zoomed has
   to start by showing the whole frame, otherwise the first thing anyone does is zoom out to
   find the edges that were cropped away. Matches how video is presented. */

.info-panel-preview__image-viewer {
  width: 100%;
  height: 100%;
  border-radius: var(--radius-sm);
}

.info-panel-preview__audio {
  width: 100%;
  height: 100%;
}

.info-panel-preview__pdf {
  width: 100%;
  height: 100%;
  border: 0;
  border-radius: var(--radius-sm);
  background-color: hsl(var(--background));
}

.info-panel-preview__text-shell {
  display: flex;
  overflow: hidden;
  width: 100%;
  height: 100%;
  flex-direction: column;
}

.info-panel-preview__text-scroll {
  width: 100%;
  min-height: 0;
  flex: 1 1 0;
}

.info-panel-preview__text-scroll :deep(.sigma-ui-scroll-area__viewport) {
  max-height: 100%;
  overflow-anchor: none;
}

.info-panel-preview__text-status {
  display: flex;
  flex: 1 1 auto;
  align-items: center;
  justify-content: center;
  color: hsl(var(--muted-foreground) / 35%);
}

.info-panel-preview__text-body {
  box-sizing: border-box;
  padding: 8px 10px;
  color: hsl(var(--muted-foreground));
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace;
  font-size: 11px;
  line-height: 1.45;
  overflow-wrap: anywhere;
  white-space: pre-wrap;
}

.info-panel-preview__spinner {
  animation: info-panel-preview-spin 1s linear infinite;
  color: hsl(var(--muted-foreground) / 45%);
}

@keyframes info-panel-preview-spin {
  from {
    transform: rotate(0deg);
  }

  to {
    transform: rotate(360deg);
  }
}
</style>
