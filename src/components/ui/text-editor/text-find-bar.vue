<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  CaseSensitiveIcon,
  ChevronDownIcon,
  ChevronUpIcon,
  ReplaceIcon,
  SearchIcon,
  XIcon,
} from '@lucide/vue';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

const props = defineProps<{
  matchCount: number;
  /** Index of the match being shown, or -1 when there is none. */
  activeIndex: number;
  /** Whether the text can be changed at all; without it the replace controls are not offered. */
  canReplace: boolean;
}>();

const query = defineModel<string>('query', { required: true });
const replacement = defineModel<string>('replacement', { required: true });
const matchCase = defineModel<boolean>('matchCase', { required: true });
const showReplace = defineModel<boolean>('showReplace', { required: true });

const emit = defineEmits<{
  next: [];
  previous: [];
  replace: [];
  replaceAll: [];
  close: [];
}>();

const { t } = useI18n();

const queryInputRef = ref<HTMLInputElement | null>(null);

const countLabel = computed(() => {
  if (!query.value) {
    return '';
  }

  if (props.matchCount === 0) {
    return t('textEditor.noMatches');
  }

  return t('textEditor.matchPosition', {
    current: props.activeIndex + 1,
    total: props.matchCount,
  });
});

function onQueryKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter') {
    event.preventDefault();

    if (event.shiftKey) {
      emit('previous');
    }
    else {
      emit('next');
    }

    return;
  }

  if (event.key === 'Escape') {
    event.preventDefault();
    emit('close');
  }
}

function onReplacementKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter') {
    event.preventDefault();
    emit('replace');
    return;
  }

  if (event.key === 'Escape') {
    event.preventDefault();
    emit('close');
  }
}

/** Puts the caret in the query with its text selected, so typing starts a new search. */
function focus() {
  const input = queryInputRef.value;

  if (!input) {
    return;
  }

  input.focus();
  input.select();
}

defineExpose({ focus });
</script>

<template>
  <div
    class="text-find-bar"
    role="search"
    :aria-label="t('textEditor.find')"
  >
    <div class="text-find-bar__row">
      <label class="text-find-bar__field">
        <SearchIcon
          :size="14"
          class="text-find-bar__field-icon"
          aria-hidden="true"
        />
        <input
          ref="queryInputRef"
          v-model="query"
          type="text"
          class="text-find-bar__input"
          :placeholder="t('textEditor.findPlaceholder')"
          :aria-label="t('textEditor.find')"
          spellcheck="false"
          autocomplete="off"
          @keydown="onQueryKeydown"
        >
      </label>
      <span
        class="text-find-bar__count"
        :class="{ 'text-find-bar__count--empty': query && matchCount === 0 }"
        aria-live="polite"
      >
        {{ countLabel }}
      </span>
      <Tooltip>
        <TooltipTrigger as-child>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            class="text-find-bar__tool"
            :class="{ 'text-find-bar__tool--active': matchCase }"
            :aria-pressed="matchCase"
            :aria-label="t('textEditor.matchCase')"
            @click="matchCase = !matchCase"
          >
            <CaseSensitiveIcon :size="16" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {{ t('textEditor.matchCase') }}
        </TooltipContent>
      </Tooltip>
      <Tooltip>
        <TooltipTrigger as-child>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            class="text-find-bar__tool"
            :disabled="matchCount === 0"
            :aria-label="t('textEditor.previousMatch')"
            @click="emit('previous')"
          >
            <ChevronUpIcon :size="16" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {{ t('textEditor.previousMatch') }}
        </TooltipContent>
      </Tooltip>
      <Tooltip>
        <TooltipTrigger as-child>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            class="text-find-bar__tool"
            :disabled="matchCount === 0"
            :aria-label="t('textEditor.nextMatch')"
            @click="emit('next')"
          >
            <ChevronDownIcon :size="16" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {{ t('textEditor.nextMatch') }}
        </TooltipContent>
      </Tooltip>
      <Tooltip>
        <TooltipTrigger as-child>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            class="text-find-bar__tool"
            :aria-label="t('textEditor.closeFind')"
            @click="emit('close')"
          >
            <XIcon :size="16" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {{ t('textEditor.closeFind') }}
        </TooltipContent>
      </Tooltip>
    </div>
    <div
      v-if="showReplace && canReplace"
      class="text-find-bar__row"
    >
      <label class="text-find-bar__field">
        <ReplaceIcon
          :size="14"
          class="text-find-bar__field-icon"
          aria-hidden="true"
        />
        <input
          v-model="replacement"
          type="text"
          class="text-find-bar__input"
          :placeholder="t('textEditor.replacePlaceholder')"
          :aria-label="t('textEditor.replacePlaceholder')"
          spellcheck="false"
          autocomplete="off"
          @keydown="onReplacementKeydown"
        >
      </label>
      <Button
        type="button"
        size="xs"
        variant="outline"
        :disabled="matchCount === 0"
        @click="emit('replace')"
      >
        {{ t('textEditor.replace') }}
      </Button>
      <Button
        type="button"
        size="xs"
        variant="outline"
        :disabled="matchCount === 0"
        @click="emit('replaceAll')"
      >
        {{ t('textEditor.replaceAll') }}
      </Button>
    </div>
  </div>
</template>

<style scoped>
.text-find-bar {
  display: flex;
  flex: 0 0 auto;
  flex-direction: column;
  padding: 6px 10px;
  border-bottom: 1px solid hsl(var(--border));
  background: hsl(var(--background));
  gap: 6px;
}

.text-find-bar__row {
  display: flex;
  align-items: center;
  gap: 4px;
}

.text-find-bar__field {
  display: flex;
  width: min(320px, 100%);
  height: 28px;
  box-sizing: border-box;
  flex: 0 1 auto;
  align-items: center;
  padding: 0 8px;
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
  background: hsl(var(--input));
  color: hsl(var(--foreground));
  gap: 6px;
}

.text-find-bar__field:focus-within {
  border-color: hsl(var(--ring));
}

.text-find-bar__field-icon {
  flex-shrink: 0;
  color: hsl(var(--muted-foreground));
}

.text-find-bar__input {
  min-width: 0;
  flex: 1 1 auto;
  padding: 0;
  border: none;
  background: transparent;
  color: inherit;
  font-size: 13px;
  outline: none;
}

.text-find-bar__input::placeholder {
  color: hsl(var(--muted-foreground));
}

.text-find-bar__count {
  min-width: 6ch;
  padding: 0 6px;
  color: hsl(var(--muted-foreground));
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.text-find-bar__count--empty {
  color: hsl(var(--destructive));
}

.text-find-bar__tool {
  width: 28px;
  height: 28px;
  flex-shrink: 0;
  color: hsl(var(--muted-foreground));
}

.text-find-bar__tool:hover:not(:disabled),
.text-find-bar__tool--active {
  color: hsl(var(--foreground));
}

.text-find-bar__tool--active {
  background: hsl(var(--secondary));
}

.text-find-bar__tool:disabled {
  opacity: 0.4;
}
</style>
