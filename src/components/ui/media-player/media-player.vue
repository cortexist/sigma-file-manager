<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<!--
  Media player with app-drawn controls.

  The native `controls` attribute is deliberately not used. On Linux those controls are
  WebKitGTK's own, drawn in a closed shadow root the app cannot style or correct, and they
  render glyph artifacts at fractional device pixel ratios. Drawing them here also keeps
  the player looking the same on every platform.
-->

<script setup lang="ts">
import { ref, computed, onBeforeUnmount, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  PlayIcon,
  PauseIcon,
  Volume2Icon,
  Volume1Icon,
  VolumeXIcon,
  MaximizeIcon,
  MinimizeIcon,
  Loader2Icon,
} from '@lucide/vue';
import { Slider } from '@/components/ui/slider';

const props = withDefaults(defineProps<{
  src: string;
  kind?: 'video' | 'audio';
  autoplay?: boolean;
}>(), {
  kind: 'video',
  autoplay: false,
});

const { t } = useI18n();

const SEEK_STEP_SECONDS = 5;
const VOLUME_STEP = 0.05;
const CONTROLS_IDLE_MS = 2500;

const containerRef = ref<HTMLElement | null>(null);
const mediaRef = ref<HTMLMediaElement | null>(null);

const isPlaying = ref(false);
const isWaiting = ref(false);
const isFullscreen = ref(false);
const isScrubbing = ref(false);
const hasError = ref(false);
const currentTime = ref(0);
const duration = ref(0);
const bufferedTo = ref(0);
const volume = ref(1);
const isMuted = ref(false);
const isPointerActive = ref(false);
const isFocusWithin = ref(false);

let idleTimer: ReturnType<typeof setTimeout> | undefined;

const isVideo = computed(() => props.kind === 'video');

/**
 * Video controls are drawn only while the pointer is moving, so the picture keeps the whole
 * viewing area the rest of the time. That matters most for portrait video in a small window,
 * where the control bar would otherwise cover real picture rather than the letterbox area a
 * landscape video leaves behind.
 *
 * Visibility follows pointer *activity* rather than a `mouseleave`, because WebKitGTK under
 * Wayland never delivers the crossing event when the pointer leaves the toplevel window: the
 * hover flag would latch on and the bar would sit over the video for the rest of the session.
 * An idle timeout needs no such event, and it also covers a pointer left resting on the video.
 *
 * Scrubbing keeps them up so the bar does not vanish mid-drag, keyboard focus keeps them
 * reachable without a pointer, and a paused video keeps them up because nothing is being
 * watched. Audio has nothing to obscure, so its controls are always shown.
 */
const controlsVisible = computed(() => {
  if (!isVideo.value) return true;

  return !isPlaying.value
    || isPointerActive.value
    || isFocusWithin.value
    || isScrubbing.value
    || hasError.value;
});

const seekPosition = computed({
  get: () => [duration.value > 0 ? (currentTime.value / duration.value) * 100 : 0],
  set: ([next]) => {
    if (!mediaRef.value || duration.value <= 0) return;
    const target = (next / 100) * duration.value;
    currentTime.value = target;
    mediaRef.value.currentTime = target;
  },
});

const volumePosition = computed({
  get: () => [isMuted.value ? 0 : volume.value * 100],
  set: ([next]) => applyVolume(next / 100),
});

const bufferedPercent = computed(
  () => (duration.value > 0 ? Math.min(100, (bufferedTo.value / duration.value) * 100) : 0),
);

const volumeIcon = computed(() => {
  if (isMuted.value || volume.value === 0) return VolumeXIcon;
  return volume.value < 0.5 ? Volume1Icon : Volume2Icon;
});

function clearIdleTimer() {
  if (idleTimer === undefined) return;
  clearTimeout(idleTimer);
  idleTimer = undefined;
}

function markPointerActive() {
  isPointerActive.value = true;
  clearIdleTimer();
  idleTimer = setTimeout(() => {
    isPointerActive.value = false;
  }, CONTROLS_IDLE_MS);
}

// Used where the pointer is known to be gone, so the bar does not wait out the idle timeout.
function markPointerIdle() {
  clearIdleTimer();
  isPointerActive.value = false;
}

function formatTime(totalSeconds: number): string {
  if (!Number.isFinite(totalSeconds) || totalSeconds < 0) return '0:00';

  const seconds = Math.floor(totalSeconds % 60);
  const minutes = Math.floor((totalSeconds / 60) % 60);
  const hours = Math.floor(totalSeconds / 3600);
  const paddedSeconds = String(seconds).padStart(2, '0');

  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, '0')}:${paddedSeconds}`;
  }

  return `${minutes}:${paddedSeconds}`;
}

function applyVolume(next: number) {
  const clamped = Math.min(1, Math.max(0, next));
  volume.value = clamped;

  if (mediaRef.value) {
    mediaRef.value.volume = clamped;
    // Dragging the slider above zero is an implicit unmute.
    mediaRef.value.muted = clamped === 0;
  }

  isMuted.value = clamped === 0;
}

async function togglePlay() {
  const media = mediaRef.value;
  if (!media) return;

  if (media.paused) {
    try {
      await media.play();
    }
    catch {
      // Autoplay rejection is not an error worth surfacing; the user can press play.
    }
  }
  else {
    media.pause();
  }
}

function seekBy(deltaSeconds: number) {
  const media = mediaRef.value;
  if (!media || duration.value <= 0) return;

  const target = Math.min(duration.value, Math.max(0, media.currentTime + deltaSeconds));
  media.currentTime = target;
  currentTime.value = target;
}

function toggleMute() {
  const media = mediaRef.value;
  if (!media) return;

  const nextMuted = !media.muted;
  media.muted = nextMuted;
  isMuted.value = nextMuted;

  // Unmuting from a zero volume would stay silent, so give it something audible.
  if (!nextMuted && volume.value === 0) {
    applyVolume(0.5);
  }
}

async function toggleFullscreen() {
  if (!isVideo.value) return;

  try {
    if (document.fullscreenElement) {
      await document.exitFullscreen();
    }
    else if (containerRef.value) {
      await containerRef.value.requestFullscreen();
    }
  }
  catch {
    // Fullscreen can be refused by the window manager; leave the player as it is.
  }
}

function syncFullscreenState() {
  isFullscreen.value = Boolean(
    containerRef.value && document.fullscreenElement === containerRef.value,
  );
}

function onLoadedMetadata() {
  const media = mediaRef.value;
  if (!media) return;

  hasError.value = false;
  duration.value = Number.isFinite(media.duration) ? media.duration : 0;
  media.volume = volume.value;
  media.muted = isMuted.value;
}

function onTimeUpdate() {
  const media = mediaRef.value;
  // Ignore element updates mid-drag so the thumb does not fight the pointer.
  if (!media || isScrubbing.value) return;
  currentTime.value = media.currentTime;
}

function onProgress() {
  const media = mediaRef.value;
  if (!media || media.buffered.length === 0) return;
  bufferedTo.value = media.buffered.end(media.buffered.length - 1);
}

function onKeydown(event: KeyboardEvent) {
  // Let the seek and volume sliders keep their own arrow key handling.
  if (event.target instanceof HTMLElement && event.target.closest('.sigma-ui-slider')) {
    if (event.key !== ' ' && event.key.toLowerCase() !== 'k') return;
  }

  switch (event.key.toLowerCase()) {
    case ' ':
    case 'k':
      event.preventDefault();
      void togglePlay();
      break;
    case 'arrowleft':
      event.preventDefault();
      seekBy(-SEEK_STEP_SECONDS);
      break;
    case 'arrowright':
      event.preventDefault();
      seekBy(SEEK_STEP_SECONDS);
      break;
    case 'arrowup':
      event.preventDefault();
      applyVolume(volume.value + VOLUME_STEP);
      break;
    case 'arrowdown':
      event.preventDefault();
      applyVolume(volume.value - VOLUME_STEP);
      break;
    case 'm':
      event.preventDefault();
      toggleMute();
      break;
    case 'f':
      if (isVideo.value) {
        event.preventDefault();
        void toggleFullscreen();
      }

      break;
    default:
      break;
  }
}

// A new source is a different file: reset everything derived from the old one.
watch(() => props.src, () => {
  hasError.value = false;
  isPlaying.value = false;
  isWaiting.value = false;
  currentTime.value = 0;
  duration.value = 0;
  bufferedTo.value = 0;
});

if (typeof document !== 'undefined') {
  document.addEventListener('fullscreenchange', syncFullscreenState);
  // Switching away from the app puts the pointer somewhere else by definition.
  window.addEventListener('blur', markPointerIdle);
}

onBeforeUnmount(() => {
  clearIdleTimer();

  if (typeof document !== 'undefined') {
    document.removeEventListener('fullscreenchange', syncFullscreenState);
    window.removeEventListener('blur', markPointerIdle);
  }
});
</script>

<template>
  <div
    ref="containerRef"
    class="media-player"
    :class="{
      'media-player--video': isVideo,
      'media-player--audio': !isVideo,
      'media-player--fullscreen': isFullscreen,
      'media-player--controls-hidden': !controlsVisible,
    }"
    tabindex="0"
    @keydown="onKeydown"
    @pointermove="markPointerActive"
    @pointerdown="markPointerActive"
    @pointerleave="markPointerIdle"
    @focusin="isFocusWithin = true"
    @focusout="isFocusWithin = false"
  >
    <video
      v-if="isVideo"
      ref="mediaRef"
      :src="src"
      :autoplay="autoplay"
      class="media-player__surface"
      playsinline
      @click="togglePlay"
      @loadedmetadata="onLoadedMetadata"
      @timeupdate="onTimeUpdate"
      @progress="onProgress"
      @playing="isPlaying = true; isWaiting = false"
      @play="isPlaying = true"
      @pause="isPlaying = false"
      @waiting="isWaiting = true"
      @ended="isPlaying = false"
      @error="hasError = true"
    />
    <audio
      v-else
      ref="mediaRef"
      :src="src"
      :autoplay="autoplay"
      class="media-player__audio-element"
      @loadedmetadata="onLoadedMetadata"
      @timeupdate="onTimeUpdate"
      @progress="onProgress"
      @playing="isPlaying = true; isWaiting = false"
      @play="isPlaying = true"
      @pause="isPlaying = false"
      @waiting="isWaiting = true"
      @ended="isPlaying = false"
      @error="hasError = true"
    />

    <div
      v-if="isWaiting && !hasError"
      class="media-player__spinner"
      aria-hidden="true"
    >
      <Loader2Icon :size="32" />
    </div>

    <div
      class="media-player__controls"
    >
      <div class="media-player__row">
        <button
          type="button"
          class="media-player__button"
          :aria-label="isPlaying ? t('mediaPlayer.pause') : t('mediaPlayer.play')"
          :title="isPlaying ? t('mediaPlayer.pause') : t('mediaPlayer.play')"
          @click="togglePlay"
        >
          <component
            :is="isPlaying ? PauseIcon : PlayIcon"
            :size="18"
          />
        </button>
        <div class="media-player__seek">
          <div
            class="media-player__buffered"
            :style="{ width: `${bufferedPercent}%` }"
            aria-hidden="true"
          />
          <Slider
            v-model="seekPosition"
            :max="100"
            :step="0.1"
            :aria-label="t('mediaPlayer.seek')"
            class="media-player__seek-slider"
            @pointerdown="isScrubbing = true"
            @pointerup="isScrubbing = false"
            @value-commit="isScrubbing = false"
          />
        </div>
        <span class="media-player__time">
          {{ formatTime(currentTime) }} / {{ formatTime(duration) }}
        </span>
      </div>
      <div class="media-player__row">
        <button
          v-if="isVideo"
          type="button"
          class="media-player__button"
          :aria-label="isFullscreen ? t('mediaPlayer.exitFullscreen') : t('mediaPlayer.fullscreen')"
          :title="isFullscreen ? t('mediaPlayer.exitFullscreen') : t('mediaPlayer.fullscreen')"
          @click="toggleFullscreen"
        >
          <component
            :is="isFullscreen ? MinimizeIcon : MaximizeIcon"
            :size="18"
          />
        </button>
        <div class="media-player__volume media-player__button--end">
          <button
            type="button"
            class="media-player__button"
            :aria-label="isMuted ? t('mediaPlayer.unmute') : t('mediaPlayer.mute')"
            :title="isMuted ? t('mediaPlayer.unmute') : t('mediaPlayer.mute')"
            @click="toggleMute"
          >
            <component
              :is="volumeIcon"
              :size="18"
            />
          </button>
          <Slider
            v-model="volumePosition"
            :max="100"
            :step="1"
            :aria-label="t('mediaPlayer.volume')"
            class="media-player__volume-slider"
          />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.media-player {
  position: relative;
  display: flex;
  overflow: hidden;
  align-items: center;
  justify-content: center;
  outline: none;
}

.media-player--video {
  width: 100%;
  height: 100%;
  background: black;
}

.media-player--audio {
  width: 80%;
  max-width: 500px;
  border-radius: var(--radius-sm);
  background: hsl(var(--background-2));
}

.media-player--fullscreen {
  width: 100vw;
  height: 100vh;
}

/* Sized to the pane rather than left at its intrinsic size: `max-width`/`max-height` alone
   only ever shrink a video, so anything smaller than the pane would sit at its native size
   with black on all four sides. `contain` then letterboxes on the one axis that needs it. */

.media-player__surface {
  width: 100%;
  height: 100%;
  object-fit:contain;
}

.media-player__audio-element {
  display: none;
}

.media-player__spinner {
  position: absolute;
  color: white;
  pointer-events: none;
}

.media-player__spinner svg {
  animation: media-player-spin 1s linear infinite;
}

@keyframes media-player-spin {
  to {
    transform: rotate(360deg);
  }
}

.media-player__controls {
  position: absolute;
  right: 0;
  bottom: 0;
  left: 0;
  display: flex;
  flex-direction: column;
  padding: 8px 12px 10px;
  gap: 4px;
  opacity: 0.8;
  transition: opacity 150ms ease;
}

.media-player--video .media-player__controls {
  background: linear-gradient(to top, rgb(0 0 0 / 78%), transparent);
}

.media-player--audio .media-player__controls {
  position: relative;
  background: none;
}

.media-player--controls-hidden .media-player__controls {
  opacity: 0;
  pointer-events: none;
}

.media-player__seek {
  position: relative;
  display: flex;
  flex: 1;
  align-items: center;
}

.media-player__buffered {
  position: absolute;
  height: 4px;
  border-radius: 2px;
  background: hsl(var(--foreground) / 25%);
  pointer-events: none;
}

.media-player__seek-slider {
  position: relative;
  z-index: 1;
}

.media-player__row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.media-player__button {
  display: flex;
  padding: 4px;
  border: none;
  border-radius: var(--radius-sm);
  background: hsl(var(--background-2));
  opacity: 0.8;
  color: var(--foreground);
  cursor: pointer;
}

.media-player--video .media-player__button {
  color: white;
}

.media-player__button:hover {
  background: hsl(var(--foreground) / 15%);
}

.media-player__button:focus-visible {
  outline: 2px solid hsl(var(--primary));
  outline-offset: 1px;
}

.media-player__button--end {
  margin-left: auto;
}

.media-player__volume {
  display: flex;
  width: 108px;
  align-items: center;
  gap: 8px;
}

.media-player__volume-slider {
  min-width: 60px;
}

.media-player__time {
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.media-player--video .media-player__time {
  color: white;
}

.media-player--audio .media-player__time {
  margin-left: auto;
  color: hsl(var(--foreground));
}
</style>
