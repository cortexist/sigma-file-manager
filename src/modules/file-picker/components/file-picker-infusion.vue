<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<!--
  The app's Infusion glass over the dialog, so a file dialog looks like the file manager that
  serves it: the background media blurred to an ambient wash, tinting the whole surface.

  Two departures from the main window's `InfusionWrapper`, both because this runs in a
  short-lived dialog process:

    - **Stills only.** A video wallpaper would need decoding and the loopback media server for
      the seconds a dialog is on screen. A video selection falls back to the bundled default
      image, which is also the honest reading of the setting: the user chose that *look*, and
      one blurred frame of it is what a 64px blur at 15% opacity amounts to anyway.
    - **Nothing is fetched.** `ensureMediaCached` is never called, so no dialog ever waits on
      (or triggers) a wallpaper download. Whatever is already local is used; a built-in that
      was never cached resolves to its bundled preview, which is plenty behind that much blur.
-->

<script setup lang="ts">
import { computed } from 'vue';
import { Infusion } from '@/components/ui/infusion';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import { useBackgroundMedia } from '@/modules/home/composables/use-background-media';
import { backgroundMedia, DEFAULT_INFUSION_BACKGROUND_FILE_NAME } from '@/data/background-media';

const userSettingsStore = useUserSettingsStore();
const { getMediaUrl, resolveMediaSelection } = useBackgroundMedia();

const infusionSettings = computed(() => userSettingsStore.userSettings.infusion);

/** The dialog is not one of the customizable pages, so it wears the shared settings. */
const pageSettings = computed(() => infusionSettings.value.pages['']);

const mediaSelectionOptions = {
  defaultMediaId: DEFAULT_INFUSION_BACKGROUND_FILE_NAME,
  resolveMediaIdFromIndex: (index: number) => backgroundMedia[index]?.fileName ?? null,
};

const infusionSrc = computed(() => {
  const selection = resolveMediaSelection(
    pageSettings.value.background,
    mediaSelectionOptions,
  );

  if (selection && selection.type === 'image') {
    return getMediaUrl(selection.item);
  }

  const fallback = resolveMediaSelection(
    { mediaId: DEFAULT_INFUSION_BACKGROUND_FILE_NAME },
    mediaSelectionOptions,
  );

  return fallback ? getMediaUrl(fallback.item) : '';
});
</script>

<template>
  <Infusion
    v-if="infusionSettings.enabled && infusionSrc"
    :src="infusionSrc"
    type="image"
    :opacity="pageSettings.opacity / 100"
    :opacity-dark="pageSettings.opacity / 100"
    :blur="pageSettings.blur"
    :media-contrast="pageSettings.mediaContrast ?? 100"
    :media-brightness="pageSettings.mediaBrightness ?? 100"
    :noise-opacity="pageSettings.noise / 100"
    :noise-scale="pageSettings.noiseScale"
    :blend-mode="pageSettings.mixBlendMode"
  />
</template>
