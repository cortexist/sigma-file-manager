<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
  type Component,
} from 'vue';
import { useI18n } from 'vue-i18n';
import {
  BoldIcon,
  CodeIcon,
  Columns2Icon,
  HeadingIcon,
  ItalicIcon,
  LinkIcon,
  ListIcon,
  Loader2Icon,
  ReplaceIcon,
  Rows2Icon,
  SaveIcon,
  SearchIcon,
  SquarePenIcon,
  Undo2Icon,
} from '@lucide/vue';
import { ScrollArea } from '@/components/ui/scroll-area';
import { isEditableElement } from '@/utils/dom-interaction-state';
import MarkdownView from './markdown-view.vue';
import TextEditorTool from './text-editor-tool.vue';
import TextFindBar from './text-find-bar.vue';
import { formatMarkdown, type MarkdownFormat } from './markdown-format';
import {
  findTextMatches,
  firstMatchIndexFrom,
  replaceAllMatches,
  replaceMatch,
  segmentTextByMatches,
  stepMatchIndex,
} from './find-in-text';
import { MAX_HIGHLIGHTED_MATCHES, revealInScrollParent } from './find-in-dom';
import type { FindRequest, TextEditorMarkdownMode } from './types';

const props = withDefaults(defineProps<{
  modelValue: string;
  /** Nothing here may be changed: a truncated file, an unsafe encoding, or a viewer handed to another application. */
  readonly?: boolean;
  markdown?: boolean;
  sourcePath?: string | null;
  /**
   * Read, edit, or — markdown only — both side by side. A plain text file reads `split` as
   * `edit`, since it has no preview to split against.
   */
  mode?: TextEditorMarkdownMode;
  /** Whether there are changes worth saving — and, equally, worth reverting. */
  canSave?: boolean;
  saving?: boolean;
  loading?: boolean;
  error?: string | null;
}>(), {
  readonly: false,
  markdown: false,
  sourcePath: null,
  mode: 'split',
  canSave: false,
  saving: false,
  loading: false,
  error: null,
});

const emit = defineEmits<{
  'update:modelValue': [value: string];
  'update:mode': [mode: TextEditorMarkdownMode];
  'save': [];
  'revert': [];
}>();

const { t } = useI18n();

const rootRef = ref<HTMLElement | null>(null);
const textareaRef = ref<HTMLTextAreaElement | null>(null);
const backdropRef = ref<HTMLElement | null>(null);
const sourcePaneRef = ref<HTMLElement | null>(null);
const previewPaneRef = ref<HTMLElement | null>(null);
const findBarRef = ref<InstanceType<typeof TextFindBar> | null>(null);

/**
 * Editing is the source pane with the lock off. A read-only markdown file is shown rendered
 * and nothing else: its source is of no use to someone who may not change it.
 */
const isEditing = computed(() => !props.readonly && props.mode !== 'read');
const showSource = computed(() => !props.markdown || isEditing.value);
const showPreview = computed(() => props.markdown && (props.readonly || props.mode !== 'edit'));
const isSplit = computed(() => showSource.value && showPreview.value);
const canReplace = computed(() => isEditing.value);

/** The toggle: reading to editing, and from editing — split included — back to reading. */
function toggleEditing() {
  emit('update:mode', isEditing.value ? 'read' : 'edit');
}

/** The split is on or off; which way it goes is decided by the room there is, below. */
function toggleSplit() {
  emit('update:mode', isSplit.value ? 'edit' : 'split');
}

/**
 * Below this width two columns are too thin to read, so a split stacks its panes instead.
 * Measured on the editor itself rather than the window, and the measurement drives both the
 * layout and the toolbar glyph, so the button can never show an arrangement other than the
 * one on screen.
 */
const SIDE_BY_SIDE_MIN_WIDTH = 720;
const panesSideBySide = ref(true);
let resizeObserver: ResizeObserver | null = null;

const splitLabel = computed(() => {
  if (!isSplit.value) {
    return t('textEditor.split');
  }

  return panesSideBySide.value ? t('textEditor.splitSideBySide') : t('textEditor.splitStacked');
});

const markdownFormats = computed((): Array<{
  kind: MarkdownFormat;
  label: string;
  icon: Component;
}> => [
  {
    kind: 'bold',
    label: t('textEditor.formatBold'),
    icon: BoldIcon,
  },
  {
    kind: 'italic',
    label: t('textEditor.formatItalic'),
    icon: ItalicIcon,
  },
  {
    kind: 'heading',
    label: t('textEditor.formatHeading'),
    icon: HeadingIcon,
  },
  {
    kind: 'link',
    label: t('textEditor.formatLink'),
    icon: LinkIcon,
  },
  {
    kind: 'code',
    label: t('textEditor.formatCode'),
    icon: CodeIcon,
  },
  {
    kind: 'list',
    label: t('textEditor.formatList'),
    icon: ListIcon,
  },
]);

/**
 * Applies a format around the textarea's selection. Goes through the browser's own
 * insertion where it can, so the edit lands on the undo stack like typing would; a value
 * assignment would wipe that history.
 */
function applyFormat(kind: MarkdownFormat) {
  const textarea = textareaRef.value;

  if (!textarea || !isEditing.value) {
    return;
  }

  const edit = formatMarkdown(props.modelValue, textarea.selectionStart, textarea.selectionEnd, kind);

  textarea.focus({ preventScroll: true });
  textarea.setSelectionRange(edit.start, edit.end);

  const inserted = typeof document.execCommand === 'function'
    && document.execCommand('insertText', false, edit.replacement);

  if (!inserted) {
    textarea.setRangeText(edit.replacement, edit.start, edit.end, 'end');
    textarea.dispatchEvent(new Event('input', { bubbles: true }));
  }

  textarea.setSelectionRange(edit.selectionStart, edit.selectionEnd);
}

// --- Find -------------------------------------------------------------------------------------

const isFindOpen = ref(false);
const isReplaceOpen = ref(false);
const findQuery = ref('');
const replacement = ref('');
const matchCase = ref(false);
const activeMatchIndex = ref(-1);
/** Reported by the rendered view, which searches the text it shows rather than the source. */
const previewMatchCount = ref(0);
/** Where the next search starts looking; set by a replacement so it lands past the new text. */
let nextSearchPosition: number | null = null;
let lastSearchKey = '';

/**
 * The source is what gets searched whenever it is on screen, the rendered page only when it
 * is all there is. In the split view the two hold the same text in different shapes, and the
 * editor is the one the hits are about to be acted on in.
 */
const sourceMatches = computed(() => {
  if (!isFindOpen.value || !showSource.value) {
    return [];
  }

  return findTextMatches(props.modelValue, findQuery.value, { matchCase: matchCase.value });
});

const matchCount = computed(() =>
  showSource.value ? sourceMatches.value.length : previewMatchCount.value,
);

const backdropSegments = computed(() => {
  if (sourceMatches.value.length === 0) {
    return [];
  }

  return segmentTextByMatches(props.modelValue, sourceMatches.value, MAX_HIGHLIGHTED_MATCHES);
});

const previewFind = computed((): FindRequest | null => {
  if (!isFindOpen.value || showSource.value) {
    return null;
  }

  return {
    query: findQuery.value,
    matchCase: matchCase.value,
    activeIndex: activeMatchIndex.value,
  };
});

function caretPosition(): number {
  return textareaRef.value?.selectionStart ?? 0;
}

/**
 * Picks which match to show whenever the list changes. A new query starts from the caret, so
 * typing finds the next occurrence from where the reader is; an edit to the text keeps the
 * place, since the matches around it are still the ones being looked at; a replacement
 * continues just past what it put in, so a replacement containing the query cannot be found
 * again and replaced forever.
 */
watch(sourceMatches, (matches, previousMatches) => {
  if (!showSource.value) {
    return;
  }

  const searchKey = `${matchCase.value ? 'A' : 'a'}:${findQuery.value}`;
  let from: number;

  if (nextSearchPosition !== null) {
    from = nextSearchPosition;
    nextSearchPosition = null;
  }
  else if (searchKey !== lastSearchKey) {
    from = caretPosition();
  }
  else {
    from = previousMatches?.[activeMatchIndex.value]?.start ?? caretPosition();
  }

  lastSearchKey = searchKey;
  activeMatchIndex.value = firstMatchIndexFrom(matches, from);
});

/**
 * Shows the active match: selected in the editor, so closing the bar leaves it ready to copy
 * or type over, and scrolled into view by way of its mark in the backdrop. The selection is
 * left alone while the editor itself has focus — moving the caret under someone typing would
 * be worse than any highlight.
 */
watch([activeMatchIndex, sourceMatches], async () => {
  const textarea = textareaRef.value;
  const match = sourceMatches.value[activeMatchIndex.value];

  if (!textarea || !match || document.activeElement === textarea) {
    return;
  }

  textarea.setSelectionRange(match.start, match.end);
  await nextTick();

  const activeMark = backdropRef.value?.querySelector<HTMLElement>('.text-editor__match--active');

  if (activeMark) {
    revealInScrollParent(activeMark);
  }
}, { flush: 'post' });

function onPreviewMatches(count: number) {
  previewMatchCount.value = count;
  activeMatchIndex.value = count > 0 ? 0 : -1;
}

function stepMatch(direction: 1 | -1) {
  activeMatchIndex.value = stepMatchIndex(activeMatchIndex.value, matchCount.value, direction);
}

function replaceActiveMatch() {
  const match = sourceMatches.value[activeMatchIndex.value];

  if (!canReplace.value || !match) {
    return;
  }

  nextSearchPosition = match.start + replacement.value.length;
  emit('update:modelValue', replaceMatch(props.modelValue, match, replacement.value));
}

function replaceEveryMatch() {
  if (!canReplace.value || sourceMatches.value.length === 0) {
    return;
  }

  emit('update:modelValue', replaceAllMatches(props.modelValue, sourceMatches.value, replacement.value));
}

/**
 * Opens the bar, seeded with the editor's selection when that is a single line — the usual
 * way to look for the other occurrences of something is to select one of them first.
 */
function openFind(withReplace = false) {
  const textarea = textareaRef.value;

  if (textarea && textarea.selectionStart !== textarea.selectionEnd) {
    const selected = props.modelValue.slice(textarea.selectionStart, textarea.selectionEnd);

    if (!selected.includes('\n')) {
      findQuery.value = selected;
    }
  }

  isFindOpen.value = true;

  if (withReplace && canReplace.value) {
    isReplaceOpen.value = true;
  }

  void nextTick(() => findBarRef.value?.focus());
}

/**
 * Closes the bar and hands focus back to the editor with the match still selected. Returns
 * whether there was a bar to close, so an owner that gives Escape another meaning can let the
 * bar take it first.
 */
function closeFind(): boolean {
  if (!isFindOpen.value) {
    return false;
  }

  isFindOpen.value = false;
  textareaRef.value?.focus({ preventScroll: true });

  return true;
}

function toggleFind() {
  if (isFindOpen.value) {
    closeFind();
  }
  else {
    openFind();
  }
}

/** Replace rides on find: turning it on opens both, turning it off leaves find up. */
function toggleReplace() {
  if (isFindOpen.value && isReplaceOpen.value) {
    isReplaceOpen.value = false;
    return;
  }

  openFind(true);
}

function onWindowKeydown(event: KeyboardEvent) {
  const target = event.target;
  const insideEditor = target instanceof Node && rootRef.value?.contains(target) === true;

  // Another field on the page owns the keys typed into it.
  if (!insideEditor && isEditableElement(target)) {
    return;
  }

  const primary = (event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey;

  if (primary && event.code === 'KeyF') {
    event.preventDefault();
    openFind();
    return;
  }

  if (primary && event.code === 'KeyH') {
    if (canReplace.value) {
      event.preventDefault();
      openFind(true);
    }

    return;
  }

  if (event.code === 'F3') {
    if (!isFindOpen.value) {
      if (!findQuery.value) {
        return;
      }

      isFindOpen.value = true;
    }

    event.preventDefault();
    stepMatch(event.shiftKey ? -1 : 1);
  }
}

// --- Editing surface --------------------------------------------------------------------------

function getScrollViewport(pane: HTMLElement | null): HTMLElement | null {
  return pane?.querySelector<HTMLElement>('.sigma-ui-scroll-area__viewport') ?? null;
}

/**
 * The textarea grows to its content and the pane around it does the scrolling, which keeps
 * the whole text reachable by the pane's scrollbar and lets the find backdrop sit exactly
 * behind it without a second scroll position to keep in step.
 */
function syncTextareaHeight() {
  const textarea = textareaRef.value;

  if (!textarea) {
    return;
  }

  textarea.style.height = 'auto';
  textarea.style.height = `${textarea.scrollHeight}px`;
}

function clampScrollTop(viewport: HTMLElement, scrollTop: number): number {
  const maxScroll = Math.max(0, viewport.scrollHeight - viewport.clientHeight);

  return Math.min(Math.max(0, scrollTop), maxScroll);
}

function onTextareaInput(event: Event) {
  const textarea = event.target as HTMLTextAreaElement;
  const viewport = getScrollViewport(sourcePaneRef.value);
  // Measuring the new height collapses the textarea for an instant, which the pane answers by
  // clamping its scroll position; put it back once layout has settled.
  const scrollTopBefore = viewport?.scrollTop ?? 0;

  emit('update:modelValue', textarea.value);
  syncTextareaHeight();

  if (!viewport) {
    return;
  }

  const scrollViewport: HTMLElement = viewport;

  function restoreScrollPosition() {
    scrollViewport.scrollTop = clampScrollTop(scrollViewport, scrollTopBefore);
  }

  void nextTick(() => {
    restoreScrollPosition();
    requestAnimationFrame(() => {
      restoreScrollPosition();
      requestAnimationFrame(restoreScrollPosition);
    });
  });
}

watch(() => props.modelValue, () => {
  void nextTick(syncTextareaHeight);
}, { flush: 'post' });

watch(showSource, (shown) => {
  if (shown) {
    void nextTick(syncTextareaHeight);
  }
});

// --- Split view scroll sync --------------------------------------------------------------------

let scrollSyncLock = false;
let teardownScrollSync: (() => void) | null = null;

/** Keeps the panes at the same fraction of their height; the only thing the two shapes share. */
function syncScrollRatio(from: HTMLElement, to: HTMLElement) {
  if (scrollSyncLock) {
    return;
  }

  const fromRange = from.scrollHeight - from.clientHeight;
  const toRange = to.scrollHeight - to.clientHeight;

  if (toRange <= 0) {
    return;
  }

  const nextTop = (fromRange > 0 ? from.scrollTop / fromRange : 0) * toRange;

  if (Math.abs(to.scrollTop - nextTop) < 0.5) {
    return;
  }

  scrollSyncLock = true;
  to.scrollTop = nextTop;
  queueMicrotask(() => {
    scrollSyncLock = false;
  });
}

function stopScrollSync() {
  teardownScrollSync?.();
  teardownScrollSync = null;
}

function startScrollSync() {
  stopScrollSync();

  const sourceViewport = getScrollViewport(sourcePaneRef.value);
  const previewViewport = getScrollViewport(previewPaneRef.value);

  if (!sourceViewport || !previewViewport) {
    return;
  }

  // Hoisted declarations do not see the guard above, so the narrowed pair is restated.
  const source: HTMLElement = sourceViewport;
  const preview: HTMLElement = previewViewport;

  function onSourceScroll() {
    syncScrollRatio(source, preview);
  }

  function onPreviewScroll() {
    syncScrollRatio(preview, source);
  }

  source.addEventListener('scroll', onSourceScroll, { passive: true });
  preview.addEventListener('scroll', onPreviewScroll, { passive: true });

  const resizeObserver = typeof ResizeObserver === 'function'
    ? new ResizeObserver(() => syncScrollRatio(source, preview))
    : null;

  resizeObserver?.observe(source);
  resizeObserver?.observe(preview);

  teardownScrollSync = () => {
    source.removeEventListener('scroll', onSourceScroll);
    preview.removeEventListener('scroll', onPreviewScroll);
    resizeObserver?.disconnect();
  };
}

watch([isSplit, () => props.loading, () => props.error], ([split, loading, error]) => {
  if (split && !loading && !error) {
    void nextTick(startScrollSync);
  }
  else {
    stopScrollSync();
  }
}, { flush: 'post' });

// --- Lifecycle --------------------------------------------------------------------------------

onMounted(() => {
  window.addEventListener('keydown', onWindowKeydown);

  if (typeof ResizeObserver !== 'undefined' && rootRef.value) {
    resizeObserver = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect.width ?? 0;
      panesSideBySide.value = width >= SIDE_BY_SIDE_MIN_WIDTH;
    });
    resizeObserver.observe(rootRef.value);
  }

  syncTextareaHeight();

  if (isSplit.value && !props.loading && !props.error) {
    startScrollSync();
  }
});

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onWindowKeydown);
  resizeObserver?.disconnect();
  stopScrollSync();
});

function focus() {
  textareaRef.value?.focus({ preventScroll: true });
}

defineExpose({
  openFind,
  closeFind,
  focus,
});
</script>

<template>
  <div
    ref="rootRef"
    class="text-editor"
  >
    <div class="text-editor__toolbar">
      <div class="text-editor__toolbar-group">
        <template v-if="!readonly">
          <TextEditorTool
            :label="t('textEditor.revert')"
            :highlighted="canSave"
            :disabled="!canSave || saving"
            @click="emit('revert')"
          >
            <Undo2Icon :size="16" />
          </TextEditorTool>
          <TextEditorTool
            :label="t('textEditor.save')"
            :highlighted="canSave"
            :disabled="!canSave"
            :loading="saving"
            shortcut="Control+S"
            @click="emit('save')"
          >
            <SaveIcon
              v-if="!saving"
              :size="16"
            />
          </TextEditorTool>
          <TextEditorTool
            :label="isEditing ? t('textEditor.read') : t('textEditor.edit')"
            :active="isEditing"
            @click="toggleEditing"
          >
            <SquarePenIcon :size="16" />
          </TextEditorTool>
          <!-- One toggle: the room available decides the direction, and the glyph shows the
               arrangement actually on screen — columns side by side, or rows stacked. -->
          <TextEditorTool
            v-if="markdown"
            :label="splitLabel"
            :active="isSplit"
            @click="toggleSplit"
          >
            <Columns2Icon
              v-if="panesSideBySide"
              :size="16"
            />
            <Rows2Icon
              v-else
              :size="16"
            />
          </TextEditorTool>
        </template>
        <TextEditorTool
          :label="t('textEditor.find')"
          :active="isFindOpen"
          shortcut="Control+F"
          @click="toggleFind"
        >
          <SearchIcon :size="16" />
        </TextEditorTool>
        <template v-if="isEditing">
          <span
            class="text-editor__toolbar-separator"
            aria-hidden="true"
          />
          <TextEditorTool
            :label="t('textEditor.toggleReplace')"
            :active="isFindOpen && isReplaceOpen"
            shortcut="Control+H"
            @click="toggleReplace"
          >
            <ReplaceIcon :size="16" />
          </TextEditorTool>
          <template v-if="markdown">
            <span
              class="text-editor__toolbar-separator"
              aria-hidden="true"
            />
            <TextEditorTool
              v-for="format in markdownFormats"
              :key="format.kind"
              :label="format.label"
              @click="applyFormat(format.kind)"
            >
              <component
                :is="format.icon"
                :size="16"
              />
            </TextEditorTool>
          </template>
        </template>
      </div>
      <div class="text-editor__toolbar-status">
        <slot name="status" />
      </div>
    </div>

    <TextFindBar
      v-if="isFindOpen"
      ref="findBarRef"
      v-model:query="findQuery"
      v-model:replacement="replacement"
      v-model:match-case="matchCase"
      v-model:show-replace="isReplaceOpen"
      :match-count="matchCount"
      :active-index="activeMatchIndex"
      :can-replace="canReplace"
      @next="stepMatch(1)"
      @previous="stepMatch(-1)"
      @replace="replaceActiveMatch"
      @replace-all="replaceEveryMatch"
      @close="closeFind"
    />

    <div
      v-if="loading"
      class="text-editor__loading"
    >
      <Loader2Icon
        :size="48"
        class="text-editor__loading-icon"
      />
    </div>
    <p
      v-else-if="error"
      class="text-editor__error"
    >
      {{ error }}
    </p>
    <div
      v-else
      class="text-editor__panes"
      :class="{
        'text-editor__panes--split': isSplit,
        'text-editor__panes--stacked': isSplit && !panesSideBySide,
      }"
      :role="isSplit ? 'group' : undefined"
      :aria-label="isSplit ? t('textEditor.markdownSplitGroup') : undefined"
    >
      <div
        v-if="showSource"
        ref="sourcePaneRef"
        class="text-editor__pane text-editor__pane--source"
        :aria-label="markdown ? t('textEditor.markdownSource') : undefined"
      >
        <ScrollArea class="text-editor__scroll">
          <div class="text-editor__surface">
            <!-- Drawn behind the textarea with its exact metrics, so each mark sits under the
                 text it matches. The textarea paints the characters; this paints only the marks. -->
            <div
              v-if="backdropSegments.length > 0"
              ref="backdropRef"
              class="text-editor__backdrop"
              aria-hidden="true"
            >
              <template
                v-for="(segment, index) in backdropSegments"
                :key="index"
              >
                <mark
                  v-if="segment.matchIndex !== null"
                  class="text-editor__match"
                  :class="{ 'text-editor__match--active': segment.matchIndex === activeMatchIndex }"
                >{{ segment.text }}</mark>
                <template v-else>
                  {{ segment.text }}
                </template>
              </template>
            </div>
            <textarea
              ref="textareaRef"
              class="text-editor__textarea"
              :value="modelValue"
              :readonly="!isEditing"
              spellcheck="false"
              :aria-label="markdown ? t('textEditor.markdownSource') : t('textEditor.textContent')"
              @input="onTextareaInput"
            />
          </div>
        </ScrollArea>
      </div>
      <div
        v-if="showPreview"
        ref="previewPaneRef"
        class="text-editor__pane text-editor__pane--preview"
        :aria-label="t('textEditor.markdownPreview')"
      >
        <ScrollArea class="text-editor__scroll">
          <MarkdownView
            :source="modelValue"
            :source-path="sourcePath"
            :find="previewFind"
            @find-matches="onPreviewMatches"
          />
        </ScrollArea>
      </div>
    </div>
  </div>
</template>

<style scoped>
.text-editor {
  display: flex;
  width: 100%;
  height: 100%;
  min-height: 0;
  flex: 1 1 0;
  flex-direction: column;
  align-self: stretch;
  background: hsl(var(--background));
}

.text-editor__toolbar {
  display: flex;
  flex: 0 0 auto;
  flex-wrap: wrap;
  align-items: center;
  padding: 6px 10px;
  border-bottom: 1px solid hsl(var(--border));
  gap: 8px;
}

.text-editor__toolbar-group {
  display: flex;
  flex: 0 0 auto;
  flex-wrap: wrap;
  align-items: center;
  gap: 2px;
}

.text-editor__toolbar-status {
  min-width: 0;
  flex: 1 1 auto;
  color: hsl(var(--muted-foreground));
  font-size: 12px;
  text-align: end;
}

.text-editor__toolbar-separator {
  width: 1px;
  height: 18px;
  margin: 0 6px;
  background: hsl(var(--border));
}

.text-editor__loading {
  display: flex;
  min-height: 120px;
  flex: 1 1 auto;
  align-items: center;
  justify-content: center;
  padding: 24px;
}

.text-editor__loading-icon {
  animation: text-editor-spin 1s linear infinite;
  color: hsl(var(--muted-foreground));
}

.text-editor__error {
  padding: 16px;
  margin: 0;
  color: hsl(var(--destructive));
  font-size: 14px;
}

.text-editor__panes {
  display: flex;
  width: 100%;
  min-height: 0;
  flex: 1 1 0;
  flex-direction: row;
}

/* Stacked or side by side is decided by the editor's own width (see SIDE_BY_SIDE_MIN_WIDTH),
   never by a viewport breakpoint: the same measurement drives the toolbar glyph. */

.text-editor__panes--stacked {
  flex-direction: column;
}

.text-editor__pane {
  display: flex;
  min-width: 0;
  min-height: 0;
  flex: 1 1 50%;
  flex-direction: column;
}

/* The divider carries the accent, softened: the plain border color disappeared between two
   full panes of text, and a split the eye cannot find reads as one jumbled document. */

.text-editor__panes--split .text-editor__pane--source {
  border-right: 1px solid hsl(var(--primary) / 65%);
}

.text-editor__panes--stacked .text-editor__pane--source {
  min-height: 120px;
  flex: 1 1 40%;
  border-right: none;
  border-bottom: 1px solid hsl(var(--primary) / 65%);
}

.text-editor__scroll {
  width: 100%;
  height: 100%;
  min-height: 0;
  flex: 1 1 0;
}

.text-editor__scroll :deep(.sigma-ui-scroll-area__viewport) {
  max-height: 100%;
  overflow-anchor: none;
}

/* Every metric that decides where a character lands is set once here and inherited by both
   layers, which is what keeps the marks under the characters they belong to. */

.text-editor__surface {
  position: relative;
  width: 100%;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace;
  font-size: 13px;
  letter-spacing: normal;
  line-height: 1.45;
  tab-size: 8;
  word-spacing: normal;
}

.text-editor__textarea,
.text-editor__backdrop {
  display: block;
  width: 100%;
  box-sizing: border-box;
  padding: 12px 16px;
  border: none;
  margin: 0;
  font: inherit;
  letter-spacing: inherit;
  overflow-wrap: break-word;
  tab-size: inherit;
  white-space: pre-wrap;
  word-break: normal;
  word-spacing: inherit;
}

.text-editor__textarea {
  position: relative;
  overflow: hidden;
  min-height: 3.6em;
  background: transparent;
  color: hsl(var(--foreground));
  outline: none;
  resize: none;
}

.text-editor__backdrop {
  position: absolute;
  color: transparent;
  inset: 0;
  pointer-events: none;
  user-select: none;
}

.text-editor__match {
  border-radius: 2px;
  background: hsl(var(--primary) / 28%);
  color: transparent;
}

.text-editor__match--active {
  background: hsl(var(--primary) / 60%);
}

@keyframes text-editor-spin {
  from {
    transform: rotate(0deg);
  }

  to {
    transform: rotate(360deg);
  }
}
</style>
