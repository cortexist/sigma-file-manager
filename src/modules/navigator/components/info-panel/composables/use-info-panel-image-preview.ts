// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
// Copyright © 2026 Cortexist, LLC (modifications). All rights reserved.

import {
  computed,
  ref,
  toValue,
  type MaybeRefOrGetter,
} from 'vue';
import { convertFileSrc } from '@tauri-apps/api/core';
import { convertMediaSrc } from '@/utils/media-src';
import { isImageFile as checkIsImage } from '@/modules/navigator/components/file-browser/utils';
import { useDevicePixelPreviewSize } from '@/modules/navigator/composables/use-device-pixel-preview-size';
import { useNavigatorImageThumbnails } from '@/modules/navigator/composables/use-navigator-image-thumbnails';
import { shouldUseImageThumbnail } from '@/modules/navigator/utils/resolve-image-display-src';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import type { DirEntry } from '@/types/dir-entry';

const DEFAULT_INFO_PANEL_THUMBNAIL_SIZE = {
  width: 560,
  height: 360,
};

export function useInfoPanelImagePreview(selectedEntry: MaybeRefOrGetter<DirEntry | null>) {
  const {
    getImageThumbnail,
    getImageThumbnailPlaceholder,
  } = useNavigatorImageThumbnails();
  const userSettingsStore = useUserSettingsStore();

  const isImageFile = computed(() => {
    const entry = toValue(selectedEntry);

    if (!entry) {
      return false;
    }

    return checkIsImage(entry);
  });

  /**
   * Retired from the settings UI now that the preview always ends up at full resolution.
   * The key is still honoured for anyone who had switched it on: it skips the intermediate
   * thumbnail so the original is the only thing ever fetched.
   */
  const showFullSizeImagePreview = computed(
    () => userSettingsStore.userSettings.navigator.infoPanel.showFullSizeImagePreview,
  );

  const usesThumbnailImagePreview = computed(() => {
    const entry = toValue(selectedEntry);

    if (!entry || !isImageFile.value) {
      return false;
    }

    return shouldUseImageThumbnail(entry, showFullSizeImagePreview.value);
  });

  const previewRef = ref<HTMLElement | null>(null);

  /**
   * Measured for every image, not just thumbnailed ones, because the pane size is what makes
   * the intermediate thumbnail worth showing at all: asking for the grid's 384px and
   * stretching it across a 4K pane is what made this preview look soft.
   */
  const { previewSize } = useDevicePixelPreviewSize({
    previewRef,
    defaultSize: DEFAULT_INFO_PANEL_THUMBNAIL_SIZE,
    enabled: isImageFile,
  });

  const mediaSrc = computed(() => {
    const entry = toValue(selectedEntry);

    if (!entry?.path) {
      return '';
    }

    return convertFileSrc(entry.path);
  });

  // Video and audio need the loopback media server on Linux; images and PDFs stay on the
  // asset protocol, which serves them fine. See @/utils/media-src.
  const playableMediaSrc = computed(() => {
    const entry = toValue(selectedEntry);

    if (!entry?.path) {
      return '';
    }

    return convertMediaSrc(entry.path);
  });

  const imageThumbnailMaxDimension = computed(
    () => Math.max(previewSize.value.width, previewSize.value.height),
  );

  /** Full-resolution source. What the viewer settles on, and what zooming magnifies. */
  const imageOriginalSrc = computed(() => {
    if (!isImageFile.value) {
      return '';
    }

    return mediaSrc.value;
  });

  /**
   * Pane-sized stand-in shown until the original decodes. Empty when the original is the
   * only sensible source (SVG, or the full-size setting), in which case the viewer simply
   * starts from the original.
   */
  const imageThumbnailSrc = computed(() => {
    const entry = toValue(selectedEntry);

    if (!entry?.path || !usesThumbnailImagePreview.value) {
      return '';
    }

    return getImageThumbnail(entry, imageThumbnailMaxDimension.value) ?? '';
  });

  /**
   * The same 20px stand-in the grid cards use. Generating the real thumbnail takes long
   * enough to read as a stall here, because unlike the grid this pane had nothing to show
   * in the meantime — and its request can be queued behind the grid's own thumbnails.
   */
  const imagePreviewPlaceholderSrc = computed(() => {
    const entry = toValue(selectedEntry);

    if (!entry?.path || !isImageFile.value) {
      return '';
    }

    return getImageThumbnailPlaceholder(entry, imageThumbnailMaxDimension.value) ?? '';
  });

  return {
    previewRef,
    isImageFile,
    mediaSrc,
    playableMediaSrc,
    imageOriginalSrc,
    imageThumbnailSrc,
    imagePreviewPlaceholderSrc,
    usesThumbnailImagePreview,
  };
}
