<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { FolderSearchIcon } from '@lucide/vue';
import { Switch } from '@/components/ui/switch';
import { toast } from '@/components/ui/toaster';
import { SettingsItem } from '@/modules/settings';
import { useI18n } from 'vue-i18n';
import { usePlatformStore } from '@/stores/runtime/platform';

const { t } = useI18n();
const platformStore = usePlatformStore();

const isAvailable = ref(false);
const isEnabled = ref(false);
const isLoading = ref(true);
const isApplying = ref(false);

async function refreshState(showError = true) {
  isLoading.value = true;

  try {
    isEnabled.value = await invoke<boolean>('is_default_file_chooser');
  }
  catch (error) {
    console.error('Failed to read file chooser registration state:', error);

    if (showError) {
      toast.error(error instanceof Error ? error.message : String(error));
    }

    isEnabled.value = false;
  }
  finally {
    isLoading.value = false;
  }
}

async function onChange(enabled: boolean) {
  isApplying.value = true;

  try {
    isEnabled.value = await invoke<boolean>('set_default_file_chooser', { enabled });
  }
  catch (error) {
    console.error('Failed to update file chooser registration:', error);
    toast.error(error instanceof Error ? error.message : String(error));
    await refreshState(false);
  }
  finally {
    isApplying.value = false;
  }
}

onMounted(async () => {
  isAvailable.value = await invoke<boolean>('file_chooser_registration_available')
    .catch(() => false);

  if (!isAvailable.value) {
    isLoading.value = false;
    return;
  }

  await refreshState();
});
</script>

<template>
  <!-- The desktop portal is a Linux mechanism; elsewhere the row would be a dead switch. -->
  <SettingsItem
    v-if="platformStore.isLinux"
    :title="t('settings.experimental.defaultFileChooser.title')"
    :description="t('settings.experimental.defaultFileChooser.description')"
    :icon="FolderSearchIcon"
  >
    <Switch
      id="default-file-chooser"
      :disabled="!isAvailable || isLoading || isApplying"
      :model-value="isEnabled"
      @update:model-value="onChange"
    />
  </SettingsItem>
</template>
