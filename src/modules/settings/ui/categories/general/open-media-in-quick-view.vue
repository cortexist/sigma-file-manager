<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { Switch } from '@/components/ui/switch';
import { SettingsItem } from '@/modules/settings';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import { PlaySquareIcon } from '@lucide/vue';

const userSettingsStore = useUserSettingsStore();
const { t } = useI18n();

const openMediaInQuickView = computed(
  () => userSettingsStore.userSettings.navigator.openMediaInQuickView,
);

function setOpenMediaInQuickView(checked: boolean) {
  userSettingsStore.set('navigator.openMediaInQuickView', checked);
}
</script>

<template>
  <SettingsItem
    :title="t('settings.general.openMediaInQuickView.title')"
    :description="t('settings.general.openMediaInQuickView.description')"
    :icon="PlaySquareIcon"
  >
    <Switch
      :model-value="openMediaInQuickView"
      @update:model-value="setOpenMediaInQuickView"
    />
  </SettingsItem>
</template>
