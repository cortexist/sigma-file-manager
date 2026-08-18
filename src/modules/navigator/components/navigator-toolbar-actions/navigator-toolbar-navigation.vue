<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { Button } from '@/components/ui/button';
import {
  Tooltip,
  TooltipTrigger,
  TooltipContent,
} from '@/components/ui/tooltip';
import { ContextMenuShortcut } from '@/components/ui/context-menu';
import { useShortcutsStore } from '@/stores/runtime/shortcuts';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  ArrowLeftIcon,
  ArrowRightIcon,
  ArrowUpIcon,
  EllipsisIcon,
  FilePlusIcon,
  FolderPlusIcon,
  HomeIcon,
  RefreshCwIcon,
} from '@lucide/vue';
import { useTextDirection } from '@/composables/use-text-direction';

defineProps<{
  canGoBack: boolean;
  canGoForward: boolean;
  canGoUp: boolean;
  isLoading: boolean;
}>();

const emit = defineEmits<{
  (event: 'goBack'): void;
  (event: 'goForward'): void;
  (event: 'goUp'): void;
  (event: 'goHome'): void;
  (event: 'refresh'): void;
  (event: 'createNewDirectory'): void;
  (event: 'createNewFile'): void;
}>();

const { t } = useI18n();
const shortcutsStore = useShortcutsStore();
const { isRtl } = useTextDirection();
const backHistoryIcon = computed(() => isRtl.value ? ArrowRightIcon : ArrowLeftIcon);
const forwardHistoryIcon = computed(() => isRtl.value ? ArrowLeftIcon : ArrowRightIcon);
</script>

<template>
  <div class="navigator-toolbar-navigation navigator-toolbar-navigation--expanded animate-fade-in">
    <Tooltip>
      <TooltipTrigger as-child>
        <Button
          variant="ghost"
          size="icon"
          class="navigator-toolbar-navigation__button"
          :disabled="!canGoBack"
          @click="emit('goBack')"
        >
          <component
            :is="backHistoryIcon"
            class="navigator-toolbar-navigation__icon"
          />
        </Button>
      </TooltipTrigger>
      <TooltipContent>
        <div class="navigator-toolbar-navigation__tooltip-row">
          {{ t('fileBrowser.goBack') }}
          <ContextMenuShortcut>{{ shortcutsStore.getShortcutLabel('navigateHistoryBack') }}</ContextMenuShortcut>
        </div>
      </TooltipContent>
    </Tooltip>
    <Tooltip>
      <TooltipTrigger as-child>
        <Button
          variant="ghost"
          size="icon"
          class="navigator-toolbar-navigation__button"
          :disabled="!canGoForward"
          @click="emit('goForward')"
        >
          <component
            :is="forwardHistoryIcon"
            class="navigator-toolbar-navigation__icon"
          />
        </Button>
      </TooltipTrigger>
      <TooltipContent>
        <div class="navigator-toolbar-navigation__tooltip-row">
          {{ t('fileBrowser.goForward') }}
          <ContextMenuShortcut>{{ shortcutsStore.getShortcutLabel('navigateHistoryForward') }}</ContextMenuShortcut>
        </div>
      </TooltipContent>
    </Tooltip>
    <Tooltip>
      <TooltipTrigger as-child>
        <Button
          variant="ghost"
          size="icon"
          class="navigator-toolbar-navigation__button"
          :disabled="!canGoUp"
          @click="emit('goUp')"
        >
          <ArrowUpIcon class="navigator-toolbar-navigation__icon" />
        </Button>
      </TooltipTrigger>
      <TooltipContent>
        <div class="navigator-toolbar-navigation__tooltip-row">
          {{ t('fileBrowser.goUp') }}
          <ContextMenuShortcut>{{ shortcutsStore.getShortcutLabel('goUpDirectory') }}</ContextMenuShortcut>
        </div>
      </TooltipContent>
    </Tooltip>
    <Tooltip>
      <TooltipTrigger as-child>
        <Button
          variant="ghost"
          size="icon"
          class="navigator-toolbar-navigation__button"
          @click="emit('goHome')"
        >
          <HomeIcon class="navigator-toolbar-navigation__icon" />
        </Button>
      </TooltipTrigger>
      <TooltipContent>{{ t('fileBrowser.goHome') }}</TooltipContent>
    </Tooltip>
    <Tooltip>
      <TooltipTrigger as-child>
        <Button
          variant="ghost"
          size="icon"
          class="navigator-toolbar-navigation__button"
          :disabled="isLoading"
          @click="emit('refresh')"
        >
          <RefreshCwIcon
            class="navigator-toolbar-navigation__icon"
            :class="{ 'navigator-toolbar-navigation__icon--spin': isLoading }"
          />
        </Button>
      </TooltipTrigger>
      <TooltipContent>
        <div class="navigator-toolbar-navigation__tooltip-row">
          {{ t('fileBrowser.refresh') }}
          <ContextMenuShortcut>{{ shortcutsStore.getShortcutLabel('reloadCurrentDirectory') }}</ContextMenuShortcut>
        </div>
      </TooltipContent>
    </Tooltip>
  </div>

  <div class="navigator-toolbar-navigation navigator-toolbar-navigation--collapsed animate-fade-in">
    <DropdownMenu>
      <DropdownMenuTrigger as-child>
        <Button
          variant="ghost"
          size="icon"
          class="navigator-toolbar-navigation__button"
          :title="t('settingsCategories.navigation')"
        >
          <EllipsisIcon class="navigator-toolbar-navigation__icon" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        side="bottom"
        class="navigator-toolbar-navigation__dropdown"
      >
        <DropdownMenuItem
          :disabled="!canGoBack"
          @click="emit('goBack')"
        >
          <component
            :is="backHistoryIcon"
            :size="14"
          />
          {{ t('fileBrowser.goBack') }}
        </DropdownMenuItem>
        <DropdownMenuItem
          :disabled="!canGoForward"
          @click="emit('goForward')"
        >
          <component
            :is="forwardHistoryIcon"
            :size="14"
          />
          {{ t('fileBrowser.goForward') }}
        </DropdownMenuItem>
        <DropdownMenuItem
          :disabled="!canGoUp"
          @click="emit('goUp')"
        >
          <ArrowUpIcon :size="14" />
          {{ t('fileBrowser.goUp') }}
        </DropdownMenuItem>
        <DropdownMenuItem @click="emit('goHome')">
          <HomeIcon :size="14" />
          {{ t('fileBrowser.goHome') }}
        </DropdownMenuItem>
        <DropdownMenuItem
          :disabled="isLoading"
          @click="emit('refresh')"
        >
          <RefreshCwIcon :size="14" />
          {{ t('fileBrowser.refresh') }}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          class="navigator-toolbar-navigation__menu-item-with-shortcut"
          @click="emit('createNewDirectory')"
        >
          <FolderPlusIcon :size="14" />
          <span>{{ t('navigator.newDirectory') }}</span>
          <ContextMenuShortcut>
            {{ shortcutsStore.getShortcutLabel('createNewDirectory') }}
          </ContextMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuItem
          class="navigator-toolbar-navigation__menu-item-with-shortcut"
          @click="emit('createNewFile')"
        >
          <FilePlusIcon :size="14" />
          <span>{{ t('navigator.newFile') }}</span>
          <ContextMenuShortcut>
            {{ shortcutsStore.getShortcutLabel('createNewFile') }}
          </ContextMenuShortcut>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  </div>
</template>

<style scoped>
.navigator-toolbar-navigation {
  display: flex;
  flex-shrink: 0;
  gap: 4px;
}

.navigator-toolbar-navigation--expanded {
  display: flex;
}

.navigator-toolbar-navigation--collapsed {
  display: none;
}

.navigator-toolbar-navigation__button {
  width: 36px;
  height: 36px;
}

.navigator-toolbar-navigation__icon {
  width: 18px;
  height: 18px;
}

.navigator-toolbar-navigation__icon--spin {
  animation: navigator-toolbar-navigation-spin 1s linear infinite;
}

@keyframes navigator-toolbar-navigation-spin {
  from {
    transform: rotate(0deg);
  }

  to {
    transform: rotate(360deg);
  }
}

.navigator-toolbar-navigation__dropdown {
  min-width: 180px;
}

.navigator-toolbar-navigation__tooltip-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.navigator-toolbar-navigation__menu-item-with-shortcut {
  display: flex;
  align-items: center;
}

/* Collapses only when the toolbar itself runs out of room for five buttons. Narrowing the
   window does not squeeze this row the way it might look: at 800px the tabs move to a row
   of their own, which leaves the toolbar with more space, not less. The buttons used to
   collapse on the width of a single pane, which a split view or an info panel reached
   constantly. Keep in step with UI_CONSTANTS.WINDOW_TOOLBAR_NAV_COLLAPSE_WIDTH. */
@media (width < 600px) {
  .navigator-toolbar-navigation--expanded {
    display: none;
  }

  .navigator-toolbar-navigation--collapsed {
    display: flex;
  }
}
</style>
