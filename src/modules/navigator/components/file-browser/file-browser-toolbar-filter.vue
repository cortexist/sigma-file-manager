<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import type { ComponentPublicInstance } from 'vue';
import { useI18n } from 'vue-i18n';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Popover, PopoverTrigger, PopoverContent } from '@/components/ui/popover';
import {
  Tooltip,
  TooltipTrigger,
  TooltipContent,
} from '@/components/ui/tooltip';
import { ContextMenuShortcut } from '@/components/ui/context-menu';
import {
  TextSearchIcon, XIcon, ChevronDownIcon, RegexIcon, FolderSearchIcon, LoaderCircleIcon,
} from '@lucide/vue';
import { useShortcutsStore } from '@/stores/runtime/shortcuts';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import { compileFileBrowserQuickSearchPattern } from '@/modules/navigator/components/file-browser/utils/file-browser-entry-quick-search';
import {
  QUICK_SEARCH_PROPERTY_KEYS,
  parseQuickSearchQuery,
  toggleQuickSearchPropertyInQuery,
  type QuickSearchProperty,
} from '@/modules/navigator/components/file-browser/utils/file-browser-quick-search-query';
import { useIsSmallScreen } from '@/composables/use-responsive-query';

const props = withDefaults(defineProps<{
  filterQuery: string;
  isFilterOpen: boolean;
  focusInput: boolean;
  isRecursiveSearchRunning?: boolean;
  isRecursiveSearchTruncated?: boolean;
  recursiveSearchError?: string | null;
}>(), {
  isRecursiveSearchRunning: false,
  isRecursiveSearchTruncated: false,
  recursiveSearchError: null,
});

const emit = defineEmits<{
  (event: 'update:filterQuery', value: string): void;
  (event: 'update:isFilterOpen', value: boolean): void;
  (event: 'filterInputFocused'): void;
}>();

const { t } = useI18n();
const shortcutsStore = useShortcutsStore();
const userSettingsStore = useUserSettingsStore();

const filterInputRef = ref<InstanceType<typeof Input> | null>(null);
const filterTriggerRef = ref<HTMLElement | ComponentPublicInstance | null>(null);
const isPropertyPanelOpen = ref(false);
const shouldFocusOnButtonOpen = ref(false);
const isSmallScreen = useIsSmallScreen();

const activeProperty = computed(() => parseQuickSearchQuery(props.filterQuery.trim()).property);
const quickSearchSettings = computed(() => userSettingsStore.userSettings.navigator.quickSearch);
const isRegexEnabled = computed(() => quickSearchSettings.value.regex);
const isRecursiveEnabled = computed(() => quickSearchSettings.value.recursive);

const patternError = computed(() => {
  if (!isRegexEnabled.value || !props.filterQuery.trim()) {
    return null;
  }

  return compileFileBrowserQuickSearchPattern(props.filterQuery).error;
});

const statusMessage = computed(() => {
  if (patternError.value) {
    return t('fileBrowser.quickSearchInvalidPattern');
  }

  if (props.recursiveSearchError) {
    return props.recursiveSearchError;
  }

  if (props.isRecursiveSearchTruncated) {
    return t('fileBrowser.quickSearchTooManyResults');
  }

  return null;
});

function toggleRegex() {
  void userSettingsStore.set('navigator.quickSearch.regex', !isRegexEnabled.value);
}

function toggleRecursive() {
  void userSettingsStore.set('navigator.quickSearch.recursive', !isRecursiveEnabled.value);
}

const quickSearchPopoverSide = computed<'bottom' | 'left'>(() => isSmallScreen.value ? 'bottom' : 'left');

watch(() => [props.isFilterOpen, props.focusInput] as const, async ([open, focusInput]) => {
  if (!open) {
    isPropertyPanelOpen.value = false;
    shouldFocusOnButtonOpen.value = false;
    return;
  }

  if (focusInput || shouldFocusOnButtonOpen.value) {
    await nextTick();
    filterInputRef.value?.$el?.focus();
    shouldFocusOnButtonOpen.value = false;
    emit('filterInputFocused');
  }
});

const propertyLabelKeys: Record<QuickSearchProperty, string> = {
  name: 'fileBrowser.name',
  size: 'fileBrowser.size',
  items: 'fileBrowser.items',
  modified: 'fileBrowser.modified',
  created: 'created',
  accessed: 'accessed',
  path: 'path',
  mime: 'type',
};

function labelForProperty(property: QuickSearchProperty): string {
  return t(propertyLabelKeys[property]);
}

function tooltipForProperty(property: QuickSearchProperty): string {
  return t(`fileBrowser.quickSearchPropertyTooltip.${property}`);
}

function handleFilterAutoFocus(event: Event) {
  event.preventDefault();
}

function clearFilter() {
  emit('update:filterQuery', '');
  isPropertyPanelOpen.value = false;
}

function handleFilterQueryUpdate(value: string | number | undefined) {
  emit('update:filterQuery', String(value ?? ''));
}

function handleFilterButtonClick() {
  if (!props.isFilterOpen) {
    shouldFocusOnButtonOpen.value = true;
  }
}

function getFilterTriggerElement(): HTMLElement | null {
  const refValue = filterTriggerRef.value;
  if (!refValue) return null;
  if (refValue instanceof HTMLElement) return refValue;
  return (refValue as ComponentPublicInstance).$el as HTMLElement;
}

function handleFilterInteractOutside(event: Event) {
  const customEvent = event as CustomEvent<{ originalEvent: PointerEvent | FocusEvent }>;
  const target = customEvent.detail?.originalEvent?.target as Node | undefined;
  const triggerEl = getFilterTriggerElement();
  const isTriggerClick = triggerEl && target && triggerEl.contains(target);

  if (isTriggerClick) {
    event.preventDefault();
    return;
  }

  if (!props.filterQuery) {
    emit('update:isFilterOpen', false);
  }
  else {
    event.preventDefault();
  }
}

function togglePropertyPanel() {
  isPropertyPanelOpen.value = !isPropertyPanelOpen.value;
}

function selectProperty(property: QuickSearchProperty) {
  emit('update:filterQuery', toggleQuickSearchPropertyInQuery(props.filterQuery, property));
}
</script>

<template>
  <Tooltip>
    <Popover
      :open="isFilterOpen"
      :modal="false"
      @update:open="emit('update:isFilterOpen', $event)"
    >
      <TooltipTrigger as-child>
        <PopoverTrigger as-child>
          <Button
            ref="filterTriggerRef"
            variant="ghost"
            size="icon"
            class="file-browser-toolbar-filter__button"
            :class="{ 'file-browser-toolbar-filter__button--active': filterQuery }"
            @click="handleFilterButtonClick"
          >
            <TextSearchIcon class="file-browser-toolbar-filter__icon" />
          </Button>
        </PopoverTrigger>
      </TooltipTrigger>
      <TooltipContent>
        <div class="file-browser-toolbar-filter__tooltip-row">
          {{ t('fileBrowser.quickSearch') }}
          <ContextMenuShortcut>{{ shortcutsStore.getShortcutLabel('toggleFilter') }}</ContextMenuShortcut>
        </div>
      </TooltipContent>
      <PopoverContent
        data-file-browser-toolbar-filter-popover
        :side="quickSearchPopoverSide"
        :align="'center'"
        class="file-browser-toolbar-filter__popover"
        @open-auto-focus="handleFilterAutoFocus"
        @close-auto-focus.prevent
        @interact-outside="handleFilterInteractOutside"
      >
        <div class="file-browser-toolbar-filter__field-row">
          <Input
            ref="filterInputRef"
            :model-value="filterQuery"
            :placeholder="isRecursiveEnabled ? t('fileBrowser.searchThisDirectoryRecursively') : t('fileBrowser.searchThisDirectory')"
            class="file-browser-toolbar-filter__input"
            :class="{ 'file-browser-toolbar-filter__input--invalid': patternError }"
            @update:model-value="handleFilterQueryUpdate"
          />
          <LoaderCircleIcon
            v-if="isRecursiveSearchRunning"
            :size="14"
            class="file-browser-toolbar-filter__spinner"
          />
          <Tooltip>
            <TooltipTrigger as-child>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                class="file-browser-toolbar-filter__suffix-btn"
                :class="{ 'file-browser-toolbar-filter__suffix-btn--active': isRegexEnabled }"
                :aria-pressed="isRegexEnabled"
                :aria-label="t('fileBrowser.quickSearchRegex')"
                @click="toggleRegex"
              >
                <RegexIcon :size="15" />
              </Button>
            </TooltipTrigger>
            <TooltipContent class="file-browser-toolbar-filter__property-tooltip">
              {{ t('fileBrowser.quickSearchRegexTooltip') }}
            </TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger as-child>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                class="file-browser-toolbar-filter__suffix-btn"
                :class="{ 'file-browser-toolbar-filter__suffix-btn--active': isRecursiveEnabled }"
                :aria-pressed="isRecursiveEnabled"
                :aria-label="t('fileBrowser.quickSearchRecursive')"
                @click="toggleRecursive"
              >
                <FolderSearchIcon :size="15" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              {{ t('fileBrowser.quickSearchRecursive') }}
            </TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger as-child>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                class="file-browser-toolbar-filter__suffix-btn"
                :class="{
                  'file-browser-toolbar-filter__suffix-btn--active': isPropertyPanelOpen,
                }"
                :aria-expanded="isPropertyPanelOpen"
                :aria-label="t('fileBrowser.quickSearchFilterByColumn')"
                @click="togglePropertyPanel"
              >
                <ChevronDownIcon
                  class="file-browser-toolbar-filter__chevron"
                  :class="{ 'file-browser-toolbar-filter__chevron--open': isPropertyPanelOpen }"
                  :size="16"
                />
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              {{ t('fileBrowser.quickSearchFilterByColumn') }}
            </TooltipContent>
          </Tooltip>
          <Button
            v-if="filterQuery"
            variant="ghost"
            size="icon"
            class="file-browser-toolbar-filter__suffix-btn"
            @click="clearFilter"
          >
            <XIcon :size="14" />
          </Button>
        </div>
        <div
          v-if="statusMessage"
          class="file-browser-toolbar-filter__status"
          :class="{ 'file-browser-toolbar-filter__status--error': patternError || recursiveSearchError }"
        >
          {{ statusMessage }}
        </div>
        <div
          v-show="isPropertyPanelOpen"
          class="file-browser-toolbar-filter__property-panel"
          role="group"
          :aria-label="t('fileBrowser.quickSearchFilterByColumn')"
        >
          <div class="file-browser-toolbar-filter__property-panel-title">
            {{ t('fileBrowser.quickSearchFilterByColumn') }}
          </div>
          <div class="file-browser-toolbar-filter__property-toggles">
            <Tooltip
              v-for="property in QUICK_SEARCH_PROPERTY_KEYS"
              :key="property"
            >
              <TooltipTrigger as-child>
                <Button
                  type="button"
                  size="xs"
                  variant="outline"
                  :class="[
                    'file-browser-toolbar-filter__property-toggle',
                    { 'file-browser-toolbar-filter__property-toggle--active': activeProperty === property },
                  ]"
                  @click="selectProperty(property)"
                >
                  {{ labelForProperty(property) }}
                </Button>
              </TooltipTrigger>
              <TooltipContent
                side="top"
                class="file-browser-toolbar-filter__property-tooltip"
              >
                {{ tooltipForProperty(property) }}
              </TooltipContent>
            </Tooltip>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  </Tooltip>
</template>

<style scoped>
.file-browser-toolbar-filter__button {
  width: 36px;
  height: 36px;
}

.file-browser-toolbar-filter__button--active {
  background-color: hsl(var(--secondary));
  color: hsl(var(--foreground));
}

.file-browser-toolbar-filter__icon {
  width: 18px;
  height: 18px;
}

.file-browser-toolbar-filter__field-row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.file-browser-toolbar-filter__input {
  min-width: 0;
  height: 28px;
  flex: 1 1 auto;
}

.file-browser-toolbar-filter__suffix-btn {
  width: 28px;
  height: 28px;
  flex-shrink: 0;
  color: hsl(var(--muted-foreground));
}

/* An accent foreground reads as "on" at a glance, which a background tint alone does not. */
.file-browser-toolbar-filter__suffix-btn--active {
  background-color: hsl(var(--primary) / 10%);
  color: hsl(var(--primary));
}

.file-browser-toolbar-filter__input--invalid {
  border-color: hsl(var(--destructive));
}

.file-browser-toolbar-filter__spinner {
  flex-shrink: 0;
  animation: file-browser-toolbar-filter-spin 1s linear infinite;
  color: hsl(var(--muted-foreground));
}

@keyframes file-browser-toolbar-filter-spin {
  to {
    transform: rotate(360deg);
  }
}

.file-browser-toolbar-filter__status {
  padding: 4px 6px 2px;
  color: hsl(var(--muted-foreground));
  font-size: 11px;
  line-height: 1.35;
}

.file-browser-toolbar-filter__status--error {
  color: hsl(var(--destructive));
}

.file-browser-toolbar-filter__chevron {
  transition: transform 0.15s ease;
}

.file-browser-toolbar-filter__chevron--open {
  transform: rotate(180deg);
}

.file-browser-toolbar-filter__property-panel {
  padding: 4px;
  border-top: 1px solid hsl(var(--border) / 50%);
  margin-top: 4px;
}

.file-browser-toolbar-filter__property-panel-title {
  margin-bottom: 8px;
  color: hsl(var(--muted-foreground));
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.02em;
  text-transform: uppercase;
}

.file-browser-toolbar-filter__property-toggles {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.file-browser-toolbar-filter__property-toggle {
  height: 1.375rem;
  min-height: 1.375rem;
  padding: 0 0.35rem;
  font-size: 11px;
  line-height: 1.15;
  text-transform: capitalize;
}

.file-browser-toolbar-filter__property-toggle--active {
  background-color: hsl(var(--secondary));
  color: hsl(var(--secondary-foreground));
}

.file-browser-toolbar-filter__property-toggle--active:hover {
  background-color: hsl(var(--secondary) / 80%);
  color: hsl(var(--secondary-foreground));
}

.file-browser-toolbar-filter__tooltip-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
</style>

<style>
.file-browser-toolbar-filter__popover.sigma-ui-popover-content {
  width: min(340px, calc(100vw - 32px));
  padding: 4px;
}

.file-browser-toolbar-filter__property-tooltip.sigma-ui-tooltip-content {
  max-width: 18rem;
  line-height: 1.35;
}
</style>
