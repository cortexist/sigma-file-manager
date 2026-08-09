<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { PlaySquareIcon } from '@lucide/vue';
import { Switch } from '@/components/ui/switch';
import { toast } from '@/components/ui/toaster';
import { SettingsItem } from '@/modules/settings';
import { useI18n } from 'vue-i18n';
import { usePlatformStore } from '@/stores/runtime/platform';
import {
  isDefaultMediaViewer,
  isMediaViewerRegistrationAvailable,
  setDefaultMediaViewer,
} from '@/utils/media-viewer-registration';

const { t } = useI18n();
const platformStore = usePlatformStore();

const isAvailable = ref(false);
const isEnabled = ref(false);
const isLoading = ref(true);
const isApplying = ref(false);

async function refreshDefaultMediaViewerState(showError = true) {
  isLoading.value = true;

  try {
    isEnabled.value = await isDefaultMediaViewer();
  }
  catch (error) {
    console.error('Failed to read default media viewer state:', error);

    if (showError) {
      toast.error(error instanceof Error ? error.message : String(error));
    }

    isEnabled.value = false;
  }
  finally {
    isLoading.value = false;
  }
}

async function onDefaultMediaViewerChange(enabled: boolean) {
  isApplying.value = true;

  try {
    isEnabled.value = await setDefaultMediaViewer(enabled);
  }
  catch (error) {
    console.error('Failed to update default media viewer state:', error);
    toast.error(error instanceof Error ? error.message : String(error));
    await refreshDefaultMediaViewerState(false);
  }
  finally {
    isApplying.value = false;
  }
}

onMounted(async () => {
  isAvailable.value = await isMediaViewerRegistrationAvailable().catch(() => false);

  if (!isAvailable.value) {
    isLoading.value = false;
    return;
  }

  await refreshDefaultMediaViewerState();
});
</script>

<template>
  <!-- Only Linux has a backend for this; elsewhere the row would be a dead switch. -->
  <SettingsItem
    v-if="platformStore.isLinux"
    :title="t('settings.experimental.defaultMediaViewer.title')"
    :description="t('settings.experimental.defaultMediaViewer.description')"
    :icon="PlaySquareIcon"
  >
    <Switch
      id="default-media-viewer"
      :disabled="!isAvailable || isLoading || isApplying"
      :model-value="isEnabled"
      @update:model-value="onDefaultMediaViewerChange"
    />
  </SettingsItem>
</template>
