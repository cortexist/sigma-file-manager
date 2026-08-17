<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { AudioLinesIcon } from '@lucide/vue';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectItemText,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { SettingsItem } from '@/modules/settings';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import type { QuickViewPlaybackOnDismiss } from '@/types/user-settings';

const userSettingsStore = useUserSettingsStore();
const { t } = useI18n();

const behaviorOptions = computed(() => [
  {
    name: t('settings.general.quickViewPlaybackOnDismiss.keepPlaying'),
    value: 'keepPlaying' as QuickViewPlaybackOnDismiss,
  },
  {
    name: t('settings.general.quickViewPlaybackOnDismiss.keepPlayingAlways'),
    value: 'keepPlayingAlways' as QuickViewPlaybackOnDismiss,
  },
  {
    name: t('settings.general.quickViewPlaybackOnDismiss.stop'),
    value: 'stop' as QuickViewPlaybackOnDismiss,
  },
]);

const selectedBehavior = computed({
  get: () => behaviorOptions.value.find(
    option => option.value === userSettingsStore.userSettings.navigator.quickViewPlaybackOnDismiss,
  ),
  set: (option) => {
    if (option) {
      userSettingsStore.set('navigator.quickViewPlaybackOnDismiss', option.value);
    }
  },
});
</script>

<template>
  <SettingsItem
    :title="t('settings.general.quickViewPlaybackOnDismiss.title')"
    :description="t('settings.general.quickViewPlaybackOnDismiss.description')"
    :icon="AudioLinesIcon"
  >
    <Select
      v-model="selectedBehavior"
      by="value"
    >
      <SelectTrigger class="quick-view-playback-select-trigger">
        <SelectValue>
          {{ selectedBehavior?.name }}
        </SelectValue>
      </SelectTrigger>
      <SelectContent>
        <SelectItem
          v-for="option in behaviorOptions"
          :key="option.value"
          :value="option"
        >
          <SelectItemText>
            {{ option.name }}
          </SelectItemText>
        </SelectItem>
      </SelectContent>
    </Select>
  </SettingsItem>
</template>

<style scoped>
.quick-view-playback-select-trigger {
  width: min(100%, 18rem);
}
</style>
