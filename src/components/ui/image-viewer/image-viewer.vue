<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<!--
  Image viewer with app-drawn controls, the still-image counterpart to `media-player.vue`.

  Both preview surfaces (the info panel and Quick View) render images through this, so a
  picture behaves the same wherever it is shown: wheel to zoom, drag to pan, and the same
  fullscreen affordance video and audio already have.

  The percentage readout is the picture's real magnification — screen pixels per image
  pixel — not a multiplier of the fitted view. Fit-to-pane is the *starting* view, but two
  panes of different sizes fit the same file at different sizes, so a fit-relative "100%"
  would claim two contradictory things at once about one image.

  Sharpness comes from loading in three stages rather than one. A pane-sized thumbnail is
  cheap and usually already cached, so it lands almost immediately; the original decodes
  behind it and swaps in once ready. Zooming therefore never magnifies a thumbnail — by the
  time anyone reaches for the wheel the original is virtually always the layer on screen.
-->

<script setup lang="ts">
import {
  computed,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from 'vue';
import { useI18n } from 'vue-i18n';
import {
  MaximizeIcon,
  MinimizeIcon,
  ZoomInIcon,
  ZoomOutIcon,
  ImageIcon,
} from '@lucide/vue';

const props = withDefaults(defineProps<{
  /** Full-resolution image. What zooming actually magnifies. */
  src: string;
  /** Pane-sized thumbnail shown until `src` decodes. */
  previewSrc?: string;
  /** The same 20px stand-in the grid cards use, blurred behind everything else. */
  placeholderSrc?: string;
  alt?: string;
}>(), {
  previewSrc: '',
  placeholderSrc: '',
  alt: '',
});

const { t } = useI18n();

const MIN_SCALE = 1;
const MAX_SCALE = 8;
const WHEEL_ZOOM_SENSITIVITY = 0.0015;
const BUTTON_ZOOM_STEP = 1.4;
const DOUBLE_CLICK_SCALE = 2;
const KEYBOARD_PAN_STEP = 48;

const containerRef = ref<HTMLElement | null>(null);
const scale = ref(1);
const offsetX = ref(0);
const offsetY = ref(0);
const isFullscreen = ref(false);
const isPanning = ref(false);
const isOriginalLoaded = ref(false);
const hasError = ref(false);
const naturalWidth = ref(0);
const naturalHeight = ref(0);
// The original's pixel size, distinct from the visible layer's: the thumbnail shares its
// aspect ratio (enough for layout and pan clamping) but not its resolution, and only the
// resolution can anchor a truthful zoom percentage.
const originalWidth = ref(0);
// Mirrors the container's box so the zoom readout follows pane resizes; the DOM reads in
// `getContainerBox` are the fallback for the moment before the observer's first delivery.
const containerWidth = ref(0);
const containerHeight = ref(0);

let resizeObserver: ResizeObserver | null = null;

let panPointerId: number | null = null;
let panStartX = 0;
let panStartY = 0;
let panOriginX = 0;
let panOriginY = 0;

const isZoomed = computed(() => scale.value > MIN_SCALE + 0.001);

/** Only the layer actually on screen; the original replaces the thumbnail once decoded. */
const activeSrc = computed(() => {
  if (isOriginalLoaded.value || !props.previewSrc) {
    return props.src;
  }

  return props.previewSrc;
});

const transform = computed(
  () => `translate(${offsetX.value}px, ${offsetY.value}px) scale(${scale.value})`,
);

/**
 * The image is laid out with `object-fit: contain`, so it rarely fills the container: the
 * letterboxed axis has slack that is not picture. Panning is clamped against the *rendered*
 * box rather than the container, otherwise a wide image in a tall pane could be dragged
 * until only background is visible.
 */
function getContainerBox(): {
  width: number;
  height: number;
} | null {
  const container = containerRef.value;

  if (!container) {
    return null;
  }

  const width = containerWidth.value || container.clientWidth;
  const height = containerHeight.value || container.clientHeight;

  if (width <= 0 || height <= 0) {
    return null;
  }

  return {
    width,
    height,
  };
}

function getRenderedImageSize(): {
  width: number;
  height: number;
} | null {
  const box = getContainerBox();

  if (!box || naturalWidth.value <= 0 || naturalHeight.value <= 0) {
    return null;
  }

  const aspectRatio = naturalWidth.value / naturalHeight.value;
  const containerAspectRatio = box.width / box.height;

  if (aspectRatio > containerAspectRatio) {
    return {
      width: box.width,
      height: box.width / aspectRatio,
    };
  }

  return {
    width: box.height * aspectRatio,
    height: box.height,
  };
}

function clampOffsets(): void {
  const box = getContainerBox();
  const renderedSize = getRenderedImageSize();

  if (!box || !renderedSize) {
    return;
  }

  const scaledWidth = renderedSize.width * scale.value;
  const scaledHeight = renderedSize.height * scale.value;
  const maxOffsetX = Math.max(0, (scaledWidth - box.width) / 2);
  const maxOffsetY = Math.max(0, (scaledHeight - box.height) / 2);

  offsetX.value = Math.min(maxOffsetX, Math.max(-maxOffsetX, offsetX.value));
  offsetY.value = Math.min(maxOffsetY, Math.max(-maxOffsetY, offsetY.value));
}

/**
 * The `scale` that would show the original at exactly one image pixel per screen pixel.
 * Physical pixels, via `devicePixelRatio`: on a scaled display, "100%" that ignored the
 * scale factor would still be interpolating — sharpness is the whole reason to ask for it.
 */
const scaleForActualSize = computed<number | null>(() => {
  if (originalWidth.value <= 0) {
    return null;
  }

  const renderedSize = getRenderedImageSize();

  if (!renderedSize) {
    return null;
  }

  const devicePixelRatio = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;

  return originalWidth.value / (renderedSize.width * devicePixelRatio);
});

/** True magnification is unknowable until the original's pixel size is — show that. */
const zoomLabel = computed(() => {
  const actualSizeScale = scaleForActualSize.value;

  if (actualSizeScale === null) {
    return '—';
  }

  return `${Math.round((scale.value / actualSizeScale) * 100)}%`;
});

// Raised past the fixed cap when needed: for a picture much larger than its pane, eight
// times the fitted size can still be a minified view, and 1:1 must always be reachable.
const effectiveMaxScale = computed(
  () => Math.max(MAX_SCALE, scaleForActualSize.value ?? 1),
);

/**
 * Zoom about a fixed point so the pixel under the cursor stays under the cursor. Anchoring
 * to the center instead would slide the subject away exactly when someone is trying to look
 * at it. `anchor` is in container coordinates, measured from its center.
 */
function applyScale(nextScale: number, anchor?: {
  x: number;
  y: number;
}): void {
  const clampedScale = Math.min(effectiveMaxScale.value, Math.max(MIN_SCALE, nextScale));

  if (clampedScale === scale.value) {
    return;
  }

  const anchorX = anchor?.x ?? 0;
  const anchorY = anchor?.y ?? 0;
  const imageX = (anchorX - offsetX.value) / scale.value;
  const imageY = (anchorY - offsetY.value) / scale.value;

  scale.value = clampedScale;
  offsetX.value = anchorX - imageX * clampedScale;
  offsetY.value = anchorY - imageY * clampedScale;

  if (clampedScale === MIN_SCALE) {
    offsetX.value = 0;
    offsetY.value = 0;
    return;
  }

  clampOffsets();
}

function getAnchorFromEvent(event: {
  clientX: number;
  clientY: number;
}) {
  const container = containerRef.value;

  if (!container) {
    return undefined;
  }

  const rect = container.getBoundingClientRect();

  return {
    x: event.clientX - rect.left - rect.width / 2,
    y: event.clientY - rect.top - rect.height / 2,
  };
}

function resetView(): void {
  scale.value = MIN_SCALE;
  offsetX.value = 0;
  offsetY.value = 0;
}

/**
 * A wheel event over the image zooms, except when it would only push an unzoomed image
 * further out. Letting that one case through unhandled keeps the surrounding panel
 * scrollable — otherwise the viewer would swallow every scroll that merely passes over it.
 */
function onWheel(event: WheelEvent): void {
  if (event.deltaY > 0 && !isZoomed.value) {
    return;
  }

  event.preventDefault();
  applyScale(scale.value * Math.exp(-event.deltaY * WHEEL_ZOOM_SENSITIVITY), getAnchorFromEvent(event));
}

function onPointerDown(event: PointerEvent): void {
  if (!isZoomed.value || event.button !== 0) {
    return;
  }

  event.preventDefault();
  panPointerId = event.pointerId;
  isPanning.value = true;
  panStartX = event.clientX;
  panStartY = event.clientY;
  panOriginX = offsetX.value;
  panOriginY = offsetY.value;
  (event.currentTarget as HTMLElement | null)?.setPointerCapture?.(event.pointerId);
}

function onPointerMove(event: PointerEvent): void {
  if (!isPanning.value || event.pointerId !== panPointerId) {
    return;
  }

  offsetX.value = panOriginX + (event.clientX - panStartX);
  offsetY.value = panOriginY + (event.clientY - panStartY);
  clampOffsets();
}

function endPan(event: PointerEvent): void {
  if (event.pointerId !== panPointerId) {
    return;
  }

  isPanning.value = false;
  panPointerId = null;
  (event.currentTarget as HTMLElement | null)?.releasePointerCapture?.(event.pointerId);
}

/**
 * Fit and actual size are the two views worth a gesture, so a double click jumps between
 * them. Only when 1:1 is at (or below) the fitted view already — a picture smaller than
 * its pane — does the jump fall back to a plain magnification step.
 */
function onDoubleClick(event: MouseEvent): void {
  if (isZoomed.value) {
    resetView();
    return;
  }

  const actualSizeScale = scaleForActualSize.value;
  const targetScale = actualSizeScale !== null && actualSizeScale > 1.05
    ? actualSizeScale
    : DOUBLE_CLICK_SCALE;

  applyScale(targetScale, getAnchorFromEvent(event));
}

function zoomInFromControls(): void {
  applyScale(scale.value * BUTTON_ZOOM_STEP);
}

function zoomOutFromControls(): void {
  applyScale(scale.value / BUTTON_ZOOM_STEP);
}

async function toggleFullscreen(): Promise<void> {
  if (!containerRef.value) {
    return;
  }

  try {
    if (document.fullscreenElement) {
      await document.exitFullscreen();
      return;
    }

    await containerRef.value.requestFullscreen();
  }
  catch {
    // Fullscreen can be refused by the window manager; leave the viewer as it is.
  }
}

function syncFullscreenState(): void {
  isFullscreen.value = Boolean(
    containerRef.value && document.fullscreenElement === containerRef.value,
  );
  // The pane changes size on the way in and out, which can leave a pan out of bounds.
  clampOffsets();
}

function panBy(deltaX: number, deltaY: number): void {
  if (!isZoomed.value) {
    return;
  }

  offsetX.value += deltaX;
  offsetY.value += deltaY;
  clampOffsets();
}

function onKeydown(event: KeyboardEvent): void {
  switch (event.key.toLowerCase()) {
    case '+':
    case '=':
      event.preventDefault();
      zoomInFromControls();
      break;
    case '-':
      event.preventDefault();
      zoomOutFromControls();
      break;
    case '0':
      event.preventDefault();
      resetView();
      break;
    case 'f':
      event.preventDefault();
      void toggleFullscreen();
      break;
    case 'escape':
      if (isZoomed.value && !isFullscreen.value) {
        event.preventDefault();
        resetView();
      }

      break;
    case 'arrowleft':
      if (isZoomed.value) {
        event.preventDefault();
        panBy(KEYBOARD_PAN_STEP, 0);
      }

      break;
    case 'arrowright':
      if (isZoomed.value) {
        event.preventDefault();
        panBy(-KEYBOARD_PAN_STEP, 0);
      }

      break;
    case 'arrowup':
      if (isZoomed.value) {
        event.preventDefault();
        panBy(0, KEYBOARD_PAN_STEP);
      }

      break;
    case 'arrowdown':
      if (isZoomed.value) {
        event.preventDefault();
        panBy(0, -KEYBOARD_PAN_STEP);
      }

      break;
    default:
      break;
  }
}

function readNaturalSize(event: Event): void {
  const image = event.target as HTMLImageElement;
  naturalWidth.value = image.naturalWidth;
  naturalHeight.value = image.naturalHeight;
}

/**
 * Dimensions come from whichever layer is showing, so panning is clamped correctly during
 * the window before the original arrives. A thumbnail keeps the source aspect ratio, so the
 * rendered box it implies is the same one the original will occupy.
 */
function onVisibleImageLoad(event: Event): void {
  readNaturalSize(event);

  // With no thumbnail to show first, the visible layer *is* the original.
  if (activeSrc.value === props.src) {
    isOriginalLoaded.value = true;
    originalWidth.value = (event.target as HTMLImageElement).naturalWidth;
  }

  clampOffsets();
}

function onOriginalDecoded(event: Event): void {
  readNaturalSize(event);
  isOriginalLoaded.value = true;
  originalWidth.value = (event.target as HTMLImageElement).naturalWidth;
  clampOffsets();
}

/**
 * Only the layer on screen can fail visibly. If the off-screen original cannot be decoded
 * the thumbnail stays up, which is a better outcome than replacing a usable picture with an
 * error glyph — so this deliberately leaves `hasError` alone.
 */
function onOriginalDecodeError(): void {
  isOriginalLoaded.value = false;
}

// A different file is a different picture: nothing about the old view still applies.
watch(() => props.src, () => {
  resetView();
  isOriginalLoaded.value = false;
  hasError.value = false;
  naturalWidth.value = 0;
  naturalHeight.value = 0;
  originalWidth.value = 0;
});

if (typeof document !== 'undefined') {
  document.addEventListener('fullscreenchange', syncFullscreenState);
}

onMounted(() => {
  if (typeof ResizeObserver === 'undefined' || !containerRef.value) {
    return;
  }

  resizeObserver = new ResizeObserver(() => {
    const container = containerRef.value;

    if (!container) {
      return;
    }

    containerWidth.value = container.clientWidth;
    containerHeight.value = container.clientHeight;
    // A shrunken pane can leave a pan past the new edge.
    clampOffsets();
  });
  resizeObserver.observe(containerRef.value);
});

onBeforeUnmount(() => {
  if (typeof document !== 'undefined') {
    document.removeEventListener('fullscreenchange', syncFullscreenState);
  }

  resizeObserver?.disconnect();
  resizeObserver = null;
});
</script>

<template>
  <div
    ref="containerRef"
    class="image-viewer"
    :class="{
      'image-viewer--fullscreen': isFullscreen,
      'image-viewer--zoomed': isZoomed,
      'image-viewer--panning': isPanning,
    }"
    tabindex="0"
    @wheel="onWheel"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="endPan"
    @pointercancel="endPan"
    @dblclick="onDoubleClick"
    @keydown="onKeydown"
  >
    <img
      v-if="placeholderSrc && !isOriginalLoaded && !hasError"
      :src="placeholderSrc"
      :alt="alt"
      class="image-viewer__placeholder"
      aria-hidden="true"
    >

    <img
      v-if="activeSrc && !hasError"
      :src="activeSrc"
      :alt="alt"
      class="image-viewer__image"
      :style="{ transform }"
      draggable="false"
      @load="onVisibleImageLoad"
      @error="hasError = true"
    >

    <!-- Kept mounted and invisible so the original decodes while the thumbnail is on screen;
         `activeSrc` swaps to it on load rather than starting the fetch only then. -->
    <img
      v-if="previewSrc && !isOriginalLoaded && src && !hasError"
      :src="src"
      alt=""
      class="image-viewer__decoder"
      aria-hidden="true"
      @load="onOriginalDecoded"
      @error="onOriginalDecodeError"
    >

    <div
      v-if="hasError"
      class="image-viewer__error"
    >
      <ImageIcon :size="48" />
    </div>

    <!-- The control clusters swallow pointerdown and dblclick before the container's
         gesture handlers see them. Otherwise, once zoomed, pressing a button starts a pan
         whose pointer capture steals the ensuing click — the buttons appear dead — and a
         quick second press lands as a double click that flips the zoom under the pointer. -->
    <div
      class="image-viewer__top-controls"
      @pointerdown.stop
      @dblclick.stop
    >
      <button
        type="button"
        class="image-viewer__button"
        :aria-label="isFullscreen ? t('imageViewer.exitFullscreen') : t('imageViewer.fullscreen')"
        :title="isFullscreen ? t('imageViewer.exitFullscreen') : t('imageViewer.fullscreen')"
        @click="toggleFullscreen"
      >
        <component
          :is="isFullscreen ? MinimizeIcon : MaximizeIcon"
          :size="18"
        />
      </button>
    </div>

    <div
      class="image-viewer__controls"
      @pointerdown.stop
      @dblclick.stop
    >
      <div class="image-viewer__row">
        <button
          type="button"
          class="image-viewer__button"
          :disabled="scale <= MIN_SCALE"
          :aria-label="t('imageViewer.zoomOut')"
          :title="t('imageViewer.zoomOut')"
          @click="zoomOutFromControls"
        >
          <ZoomOutIcon :size="18" />
        </button>
        <button
          type="button"
          class="image-viewer__zoom-level"
          :aria-label="t('imageViewer.resetZoom')"
          :title="t('imageViewer.resetZoom')"
          @click="resetView"
        >
          {{ zoomLabel }}
        </button>
        <button
          type="button"
          class="image-viewer__button"
          :disabled="scale >= effectiveMaxScale"
          :aria-label="t('imageViewer.zoomIn')"
          :title="t('imageViewer.zoomIn')"
          @click="zoomInFromControls"
        >
          <ZoomInIcon :size="18" />
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.image-viewer {
  position: relative;
  display: flex;
  overflow: hidden;
  width: 100%;
  height: 100%;
  align-items: center;
  justify-content: center;
  outline: none;
}

.image-viewer--fullscreen {
  width: 100vw;
  height: 100vh;
  background: black;
}

.image-viewer__image {
  width: 100%;
  height: 100%;
  object-fit: contain;

  /* Panning is a live drag, so animating it would lag the pointer. Only the wheel and the
     buttons get a transition, and both go through discrete steps. */
  transition: transform 120ms ease-out;
  will-change: transform;
}

.image-viewer--panning .image-viewer__image {
  transition: none;
}

.image-viewer--zoomed {
  cursor: grab;
}

.image-viewer--panning {
  cursor: grabbing;
}

/* A 20px image stretched over the pane: it reads as the picture's colors rather than as a
   broken preview, and covers the gap before the thumbnail arrives. */

.image-viewer__placeholder {
  position: absolute;
  width: 100%;
  height: 100%;
  filter: blur(12px);
  inset: 0;
  object-fit: contain;
  opacity: 0.5;
}

.image-viewer__decoder {
  position: absolute;
  width: 1px;
  height: 1px;
  opacity: 0;
  pointer-events: none;
}

.image-viewer__error {
  display: flex;
  align-items: center;
  justify-content: center;
  color: hsl(var(--muted-foreground) / 35%);
}

.image-viewer__top-controls {
  position: absolute;
  z-index: 1;
  top: 0;
  right: 0;
  left: 0;
  display: flex;
  justify-content: flex-end;
  padding: 8px 12px;
  opacity: 0.9;
  transition: opacity 150ms ease;
}

.image-viewer__controls {
  position: absolute;
  z-index: 1;
  right: 0;
  bottom: 0;
  left: 0;
  display: flex;
  justify-content: center;
  padding: 8px 12px 10px;
  opacity: 0.9;
  transition: opacity 150ms ease;
}

.image-viewer__row {
  display: flex;
  align-items: center;
  padding: 4px 8px;
  border-radius: var(--radius-lg);
  background: hsl(var(--background) / 85%);
  gap: 8px;
}

.image-viewer__button {
  display: flex;
  padding: 4px;
  border: none;
  border-radius: var(--radius-sm);
  background: hsl(var(--background) / 50%);
  color: var(--foreground);
  cursor: pointer;
}

.image-viewer__button:hover:not(:disabled) {
  background: hsl(var(--foreground) / 15%);
}

.image-viewer__button:disabled {
  cursor: default;
  opacity: 0.4;
}

.image-viewer__button:focus-visible,
.image-viewer__zoom-level:focus-visible {
  outline: 2px solid hsl(var(--primary));
  outline-offset: 1px;
}

.image-viewer__zoom-level {
  min-width: 48px;
  padding: 4px 6px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--foreground);
  cursor: pointer;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}

.image-viewer__zoom-level:hover {
  background: hsl(var(--foreground) / 15%);
}
</style>
