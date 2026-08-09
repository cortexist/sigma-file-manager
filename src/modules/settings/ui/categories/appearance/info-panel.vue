<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
Copyright © 2026 Cortexist, LLC (modifications). All rights reserved.
-->

<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import { PanelRightIcon } from '@lucide/vue';
import { Switch } from '@/components/ui/switch';
import { SettingsItem } from '@/modules/settings';
import { useInfoPanelBooleanSetting } from '@/modules/settings/composables/use-info-panel-boolean-setting';
import { useInfoPanelLayout } from '@/modules/navigator/components/info-panel/composables/use-info-panel-layout';

const { t } = useI18n();
const {
  isDynamicSize: infoPanelDynamicSize,
  enableDynamicSize,
  disableDynamicSize,
} = useInfoPanelLayout();

const muteVideoPreviewByDefault = useInfoPanelBooleanSetting('muteVideoPreviewByDefault');
const autoplayVideoPreview = useInfoPanelBooleanSetting('autoplayVideoPreview');

function handleToggleInfoPanelDynamicSize(enabled: boolean) {
  if (enabled) {
    void enableDynamicSize();
    return;
  }

  void disableDynamicSize();
}
</script>

<template>
  <SettingsItem
    :title="t('settings.infoPanel.title')"
    :icon="PanelRightIcon"
  >
    <template #nested>
      <div class="info-panel-settings">
        <div class="info-panel-settings__row">
          <div class="info-panel-settings__copy">
            <span class="info-panel-settings__label">
              {{ t('settings.infoPanel.dynamicSize') }}
            </span>
            <p class="info-panel-settings__description">
              {{ t('settings.infoPanel.dynamicSizeTooltip') }}
            </p>
          </div>
          <Switch
            :model-value="infoPanelDynamicSize"
            @update:model-value="handleToggleInfoPanelDynamicSize"
          />
        </div>

        <!-- "Show full-size image in info panel preview" used to live here. The preview now
             always resolves to the original, so the switch no longer chooses between a sharp
             and a soft picture — only whether an intermediate thumbnail is shown on the way.
             The stored key is still honoured for anyone who had turned it on; see
             `use-info-panel-image-preview.ts`. -->

        <div class="info-panel-settings__row">
          <div class="info-panel-settings__copy">
            <span class="info-panel-settings__label">
              {{ t('settings.infoPanel.muteVideoPreviewByDefault') }}
            </span>
            <p class="info-panel-settings__description">
              {{ t('settings.infoPanel.muteVideoPreviewByDefaultDescription') }}
            </p>
          </div>
          <Switch
            :model-value="muteVideoPreviewByDefault"
            @update:model-value="muteVideoPreviewByDefault = $event"
          />
        </div>

        <div class="info-panel-settings__row">
          <div class="info-panel-settings__copy">
            <span class="info-panel-settings__label">
              {{ t('settings.infoPanel.autoplayVideoPreview') }}
            </span>
            <p class="info-panel-settings__description">
              {{ t('settings.infoPanel.autoplayVideoPreviewDescription') }}
            </p>
          </div>
          <Switch
            :model-value="autoplayVideoPreview"
            @update:model-value="autoplayVideoPreview = $event"
          />
        </div>
      </div>
    </template>
  </SettingsItem>
</template>

<style scoped>
.info-panel-settings {
  display: flex;
  width: 100%;
  flex-direction: column;
  gap: 1rem;
}

.info-panel-settings__row {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-start;
  justify-content: space-between;
  gap: 1rem 2rem;
}

.info-panel-settings__copy {
  display: flex;
  min-width: min(100%, 16rem);
  flex: 1 1 12rem;
  flex-direction: column;
}

.info-panel-settings__label {
  color: hsl(var(--foreground));
  font-size: 0.875rem;
}

.info-panel-settings__description {
  margin: 0;
  color: hsl(var(--muted-foreground));
  font-size: 0.875rem;
  line-height: 1.4;
}

/* The warning pill, its transition, and the shortcut `kbd` styling went with the removed
   full-size-image row — they had no other user in this file. */
</style>
