<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<script setup lang="ts">
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

/**
 * One toolbar button: an icon, a tooltip that is also its accessible name, and one colour
 * rule — the accent says "this is on" for a toggle, or "this will do something" for an
 * action that is currently possible. Everything else is quiet.
 */
defineProps<{
  label: string;
  /** A toggle's state; shown as the accent and announced as pressed. */
  active?: boolean;
  /** An action worth taking right now (unsaved changes, say); accent without toggle semantics. */
  highlighted?: boolean;
  disabled?: boolean;
  loading?: boolean;
  shortcut?: string;
}>();

const emit = defineEmits<{
  click: [];
}>();
</script>

<template>
  <Tooltip>
    <TooltipTrigger as-child>
      <Button
        type="button"
        variant="ghost"
        size="icon"
        class="text-editor-tool"
        :class="{ 'text-editor-tool--on': active || highlighted }"
        :aria-pressed="active"
        :aria-label="label"
        :aria-keyshortcuts="shortcut"
        :disabled="disabled"
        :is-loading="loading"
        @click="emit('click')"
      >
        <slot />
      </Button>
    </TooltipTrigger>
    <TooltipContent>
      {{ label }}
    </TooltipContent>
  </Tooltip>
</template>

<style scoped>
.text-editor-tool {
  width: 28px;
  height: 28px;
  color: hsl(var(--muted-foreground));
}

.text-editor-tool:hover:not(:disabled) {
  color: hsl(var(--foreground));
}

.text-editor-tool--on,
.text-editor-tool--on:hover:not(:disabled) {
  color: hsl(var(--primary));
}
</style>
