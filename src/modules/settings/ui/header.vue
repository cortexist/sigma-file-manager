<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { useI18n } from 'vue-i18n';

const { t } = useI18n();
</script>

<template>
  <div class="settings-header">
    <svg
      data-tauri-drag-region
      viewBox="0 0 128 128"
      xmlns="http://www.w3.org/2000/svg"
    >
      <rect
        x="34"
        y="33.075"
        width="60"
        height="60"
        class="glyph"
      />
      <path d="M41.781,21l-41.681,0l-0,-7.016l41.681,0l0,7.016Zm-41.665,0l126.398,0.043c0.756,0 1.369,0.614 1.369,1.369l0,90.384c0,0.756 -0.614,1.369 -1.369,1.369l-125.029,0c-0.756,0 -1.369,-0.614 -1.369,-1.369l0,-91.796Zm80.884,21l-36,0l0,8l16.661,16.3l-16.625,17.671l0,9.029l37.963,0l0,-9l-25.989,0l16.857,-17.7l-16.16,-15.3l23.252,0l0.04,-9Z" fill="currentColor"/>
      <rect
        x="0.12"
        y="14.035"
        width="41.619"
        height="7.095"
        fill-opacity="0.15"
        fill="#00000088"
      />
    </svg>
    <div class="settings-header__title-section">
      <p class="settings-header__description">
        <strong>"{{ t('app.name') }}"</strong>
        {{ t('app.description') }}
        <br>{{ t('app.copyright') }}
      </p>
    </div>
  </div>
</template>

<style scoped>
.settings-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-bottom: 1rem;
  border-bottom: 1px solid hsl(var(--border));
  gap: 2rem;
}

.settings-header svg {
  color: hsl(var(--primary));
  width: 64px;
  height: 64px;
}

.settings-header .glyph {
  fill: contrast-color(hsl(var(--primary)));
}

.settings-header__title-section {
  flex: 1;
}

.settings-header__description {
  margin: 0;
  color: hsl(var(--muted-foreground));
  font-size: 0.875rem;
}

@media (width <= 768px) {
  .settings-header {
    font-size: 10px;
    gap: 1rem;
  }

  .settings-header svg {
    width: 36px;
    height: 36px;
  }

   .settings-header__description {
     font-size: 0.75rem;
   }
}
</style>
