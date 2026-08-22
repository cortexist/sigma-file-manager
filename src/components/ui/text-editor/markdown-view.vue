<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { renderMarkdownToSafeHtml } from '@/utils/safe-html';
import { rewriteMarkdownAssetUrls } from '@/utils/readme-relative-urls';
import {
  applyFindHighlights,
  clearFindHighlights,
  collectDomTextMatches,
  revealInScrollParent,
} from './find-in-dom';
import type { FindRequest } from './types';

const props = withDefaults(defineProps<{
  /** Markdown to render. */
  source: string;
  /**
   * The file the markdown came from, so relative image links resolve next to it. Leave unset
   * for text that has no file behind it, such as a document fetched from a URL.
   */
  sourcePath?: string | null;
  /** Tighter type for a narrow column, such as the info panel's preview. */
  dense?: boolean;
  /** What to find in the rendered text, or null while nothing is being searched. */
  find?: FindRequest | null;
}>(), {
  sourcePath: null,
  dense: false,
  find: null,
});

const emit = defineEmits<{
  /** How many matches the current find request has in the rendered text. */
  'find-matches': [count: number];
}>();

const rootRef = ref<HTMLElement | null>(null);
const html = ref('');
let renderRequestId = 0;
let matchRanges: Range[] = [];

watch(
  [() => props.source, () => props.sourcePath],
  async ([source, sourcePath]) => {
    const requestId = ++renderRequestId;
    const baseHtml = renderMarkdownToSafeHtml(source);

    if (!sourcePath) {
      html.value = baseHtml;
      return;
    }

    // Shown only once the links are rewritten: the unrewritten page would flash its images
    // as broken first, and every relative image would be fetched twice.
    try {
      const rewritten = await rewriteMarkdownAssetUrls(baseHtml, {
        kind: 'localMarkdownFile',
        markdownFilePath: sourcePath,
      });

      if (requestId === renderRequestId) {
        html.value = rewritten;
      }
    }
    catch {
      if (requestId === renderRequestId) {
        html.value = baseHtml;
      }
    }
  },
  { immediate: true },
);

function paintActiveMatch() {
  const activeIndex = props.find?.activeIndex ?? -1;

  applyFindHighlights(matchRanges, activeIndex);

  const active = matchRanges[activeIndex];

  if (active) {
    revealInScrollParent(active);
  }
}

function refreshMatches() {
  const root = rootRef.value;
  const find = props.find;

  if (!root || !find) {
    matchRanges = [];
    clearFindHighlights();
    return;
  }

  matchRanges = find.query
    ? collectDomTextMatches(root, find.query, { matchCase: find.matchCase })
    : [];
  emit('find-matches', matchRanges.length);
  paintActiveMatch();
}

// Runs after the DOM has the new markup, since the ranges point into its text nodes. Markup
// that was ready before mounting is handled by the mount hook, as no change announces it.
watch(
  [html, () => props.find?.query, () => props.find?.matchCase, () => props.find === null],
  refreshMatches,
  { flush: 'post' },
);

watch(() => props.find?.activeIndex, paintActiveMatch);

onMounted(refreshMatches);
onBeforeUnmount(clearFindHighlights);
</script>

<template>
  <div
    ref="rootRef"
    class="markdown-content text-editor-markdown"
    :class="{ 'text-editor-markdown--dense': dense }"
    v-html="html"
  />
</template>

<style scoped>
.text-editor-markdown {
  box-sizing: border-box;
  padding: 12px 16px;
  margin: 0;
  color: hsl(var(--foreground));
  font-size: 0.875rem;
  line-height: 1.6;
  overflow-wrap: anywhere;
}

.text-editor-markdown--dense {
  padding: 8px 10px;
  font-size: 12px;
  line-height: 1.5;
}

/* The shared markdown styles size headings in rem, which a narrow column cannot afford. */
.text-editor-markdown--dense :deep(h1) {
  font-size: 1.35em;
}

.text-editor-markdown--dense :deep(h2) {
  font-size: 1.2em;
}

.text-editor-markdown--dense :deep(h3) {
  font-size: 1.1em;
}

.text-editor-markdown--dense :deep(h4),
.text-editor-markdown--dense :deep(h5),
.text-editor-markdown--dense :deep(h6) {
  font-size: 1em;
}
</style>

<style>
@import '@/styles/markdown-content.css';

::highlight(text-editor-find) {
  background-color: hsl(var(--primary) / 28%);
}

::highlight(text-editor-find-active) {
  background-color: hsl(var(--primary) / 60%);
  color: hsl(var(--primary-foreground));
}
</style>
