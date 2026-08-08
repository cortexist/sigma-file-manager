<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { CheckIcon } from '@lucide/vue';
import { Popover, PopoverTrigger, PopoverContent } from '@/components/ui/popover';
import { DEFAULT_ACCENT_COLOR, useUserSettingsStore } from '@/stores/storage/user-settings';

/** Bare HSL channels, assigned straight to `--primary`. Laid out as a 3x3 grid in order. */
const ACCENT_COLORS: {
  value: string;
  nameKey: string;
}[] = [
  {
    value: '37 73% 52%',
    nameKey: 'mango',
  },
  {
    value: '56 73% 52%',
    nameKey: 'lemon',
  },
  {
    value: '87 73% 52%',
    nameKey: 'matcha',
  },
  {
    value: '130 73% 52%',
    nameKey: 'lime',
  },
  {
    value: DEFAULT_ACCENT_COLOR,
    nameKey: 'default',
  },
  {
    value: '224 73% 62%',
    nameKey: 'strongBlue',
  },
  {
    value: '264 73% 62%',
    nameKey: 'violet',
  },
  {
    value: '330 100% 50%',
    nameKey: 'zune',
  },
  {
    value: '350 78% 52%',
    nameKey: 'red',
  },
];

const userSettingsStore = useUserSettingsStore();
const { t } = useI18n();

const isOpen = ref(false);

const selectedAccentColor = computed(
  () => userSettingsStore.userSettings.accentColor ?? DEFAULT_ACCENT_COLOR,
);

function selectAccentColor(value: string) {
  userSettingsStore.set('accentColor', value);
  isOpen.value = false;
}
</script>

<template>
  <Popover v-model:open="isOpen">
    <PopoverTrigger as-child>
      <button
        type="button"
        class="accent-color__swatch accent-color__swatch--trigger"
        :style="{ background: `hsl(${selectedAccentColor})` }"
        :aria-label="t('settings.accentColor.title')"
        :title="t('settings.accentColor.title')"
      />
    </PopoverTrigger>
    <!-- PopoverContent is teleported through PopoverPortal, so it never carries this
         component's scope id and a scoped rule cannot reach it. An inline style lands on the
         root either way, and is what overrides the 18rem-wide, 1rem-padded default. -->
    <PopoverContent :style="{ width: 'fit-content', padding: '8px' }">
      <div class="accent-color__grid">
        <button
          v-for="color in ACCENT_COLORS"
          :key="color.value"
          type="button"
          class="accent-color__swatch"
          :style="{ background: `hsl(${color.value})` }"
          :aria-label="t(`settings.accentColor.colors.${color.nameKey}`)"
          :title="t(`settings.accentColor.colors.${color.nameKey}`)"
          :aria-pressed="color.value === selectedAccentColor"
          @click="selectAccentColor(color.value)"
        >
          <CheckIcon
            v-if="color.value === selectedAccentColor"
            :size="16"
            class="accent-color__check"
          />
        </button>
      </div>
    </PopoverContent>
  </Popover>
</template>

<style scoped>
.accent-color__swatch {
  display: flex;
  width: 24px;
  height: 24px;
  align-items: center;
  justify-content: center;
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
  cursor: pointer;
}

.accent-color__swatch:focus-visible {
  outline: 2px solid hsl(var(--ring));
  outline-offset: 2px;
}

/* Columns are the swatch width rather than `1fr`, so nothing stretches to fill the panel. */
.accent-color__grid {
  display: grid;
  gap: 8px;
  grid-template-columns: repeat(3, 24px);
}

/* Dark tick on pale swatches, light on saturated ones, is not worth a luminance calculation
   here: a mixed-blend tick stays legible on every color in the set. */

.accent-color__check {
  color: white;
  mix-blend-mode: normal;
}
</style>
