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
  Repeat1Icon,
  RepeatOffIcon,
  Music2Icon,
  Loader2Icon,
} from '@lucide/vue';
import { Slider } from '@/components/ui/slider';

const props = withDefaults(defineProps<{
  src: string;
  kind?: 'video' | 'audio';
  autoplay?: boolean;
  /** Start silent. Also what lets an autoplaying preview begin without being blocked. */
  muted?: boolean;
  /** Artwork for audio, resolved by the caller. Falls back to a music icon when absent. */
  poster?: string;
}>(), {
  kind: 'video',
  autoplay: false,
  muted: false,
  poster: undefined,
});

const { t } = useI18n();

const SEEK_STEP_SECONDS = 5;
const VOLUME_STEP = 0.05;
const CONTROLS_IDLE_MS = 2500;
const LOOP_WRAP_LEAD_SECONDS = 0.2;
const LOOP_WATCH_INTERVAL_MS = 100;

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
const isMuted = ref(props.muted);
const isPointerActive = ref(false);
const isFocusWithin = ref(false);
/**
 * Repeat-one for now. When play-all and shuffle arrive this becomes the "repeat" leg of a
 * playback mode, so the button lives in its own lower-left group rather than beside the
 * transport controls.
 */
const isLooping = ref(false);

let idleTimer: ReturnType<typeof setTimeout> | undefined;
let loopWatchTimer: ReturnType<typeof setInterval> | undefined;
let lastProgressAt = -1;
/**
 * Autoplay is started from here rather than left to the `autoplay` attribute alone, because
 * the attribute only takes effect on the element's *first* load — the HTML spec clears the
 * can-autoplay flag once an element has played or been paused. Callers used to work around
 * that by keying the player on the file path so each file got a brand new element, but that
 * remounts the DOM node holding fullscreen, which dropped out of fullscreen on every
 * next-file. Playing explicitly on each new source lets the instance survive instead.
 */
let shouldAutoplayNextLoad = props.autoplay;

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

/**
 * Only keyboard focus holds the controls open.
 *
 * Focus is how someone navigating without a pointer keeps the controls reachable, but a plain
 * `focusin` also fires when a button is clicked, and focus then stays on that button. In
 * fullscreen that is fatal: you got there by clicking the fullscreen button, nothing else is
 * around to take focus away, so the bar would stay up for good over the picture.
 *
 * `:focus-visible` is exactly the distinction wanted — the engine sets it for keyboard focus
 * and withholds it for a click on a button.
 */
function onFocusIn(event: FocusEvent) {
  const target = event.target;

  if (!(target instanceof Element)) {
    isFocusWithin.value = false;
    return;
  }

  try {
    isFocusWithin.value = target.matches(':focus-visible');
  }
  catch {
    // An engine without `:focus-visible` keeps the old behaviour rather than stranding
    // keyboard users with controls they cannot hold open.
    isFocusWithin.value = true;
  }
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

function toggleLoop() {
  isLooping.value = !isLooping.value;
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
  // if (!isVideo.value) return;

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

  if (shouldAutoplayNextLoad) {
    shouldAutoplayNextLoad = false;
    void media.play().catch(() => {
      // Same as pressing play: a blocked autoplay is not worth surfacing.
    });
  }
}

function clearWaiting() {
  isWaiting.value = false;
}

/**
 * Playback progress is the only trustworthy "not stalled" signal on this stack. WebKitGTK
 * reports `readyState` 2 for the whole of a fully buffered file, so it can never be compared
 * against HAVE_FUTURE_DATA, and a seek fires `waiting` without a matching `playing` afterwards.
 * `timeupdate` only fires when the clock actually moves, so treating it as proof of life clears
 * the spinner no matter which event the pipeline decided to skip.
 */
function markProgress() {
  const media = mediaRef.value;
  if (!media || media.currentTime === lastProgressAt) return;

  lastProgressAt = media.currentTime;
  clearWaiting();
}

function onPlaybackActive() {
  isPlaying.value = true;
  clearWaiting();
}

function onPause() {
  isPlaying.value = false;
  clearWaiting();
}

function stopLoopWatch() {
  if (loopWatchTimer === undefined) return;
  clearInterval(loopWatchTimer);
  loopWatchTimer = undefined;
}

/**
 * While looping, wrap fractionally before the end rather than letting the file finish.
 *
 * Reaching the end and seeking back works windowed but not in fullscreen: WebKitGTK leaves
 * the element reporting `paused === false` on a frozen first frame — no buffering, no error,
 * just a clock that never advances, so the button showed pause and two clicks were needed to
 * get moving. Never entering the ended state avoids that path, and playback carries straight
 * through the seek. `timeupdate` is too coarse to catch the last fraction reliably, hence the
 * short interval.
 */
function startLoopWatch() {
  stopLoopWatch();

  loopWatchTimer = setInterval(() => {
    const media = mediaRef.value;

    if (!media || media.paused || !isLooping.value) return;

    const remaining = media.duration - media.currentTime;

    if (Number.isFinite(remaining) && remaining > 0 && remaining <= LOOP_WRAP_LEAD_SECONDS) {
      media.currentTime = 0;
    }
  }, LOOP_WATCH_INTERVAL_MS);
}

/**
 * Rewind and resume, in that order, waiting for the seek to actually land.
 *
 * Asking to play in the same tick as the rewind works windowed but not in fullscreen, where
 * WebKitGTK takes the seek and drops the play: the element then reports `paused === false`
 * while nothing advances, so the button claimed to be playing and the next click merely
 * paused it — two clicks to get moving again.
 *
 * `isPlaying` is reconciled from the element at the end rather than assumed, so the button
 * always describes what the pipeline is really doing.
 */
function restartForLoop(media: HTMLMediaElement) {
  let resumed = false;

  function resume() {
    if (resumed) return;
    resumed = true;

    clearTimeout(seekFallbackTimer);
    media.removeEventListener('seeked', resume);

    void media.play()
      .catch(() => {
        // Refused outright: leave it genuinely paused so a single click resumes.
        media.pause();
      })
      .finally(() => {
        isPlaying.value = !media.paused;
      });
  }

  media.addEventListener('seeked', resume);
  // Should the engine never report the seek, start anyway rather than stopping the loop dead.
  const seekFallbackTimer = setTimeout(resume, 400);

  media.currentTime = 0;
}

function onEnded() {
  const media = mediaRef.value;

  /**
   * Looping is driven here rather than by the element's `loop` attribute. WebKitGTK's own
   * loop re-reads the stream and then wraps every later round several seconds early, always
   * at the same point, while a straight play-through reaches the true end and fires `ended`.
   * Restarting by hand keeps every round a normal play-through.
   */
  if (isLooping.value && media) {
    restartForLoop(media);
    return;
  }

  isPlaying.value = false;
  clearWaiting();
}

function onError() {
  hasError.value = true;
  isWaiting.value = false;
}

function onTimeUpdate() {
  const media = mediaRef.value;
  markProgress();

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
      event.preventDefault();
      void toggleFullscreen();
      break;
    default:
      break;
  }
}

// The pre-end wrap only needs watching while a loop is actually running.
watch([isLooping, isPlaying], ([looping, playing]) => {
  if (looping && playing) {
    startLoopWatch();
    return;
  }

  stopLoopWatch();
});

// A new source is a different file: reset everything derived from the old one. The instance
// itself is kept across files so that fullscreen survives; see `shouldAutoplayNextLoad`.
watch(() => props.src, () => {
  hasError.value = false;
  isPlaying.value = false;
  isWaiting.value = false;
  currentTime.value = 0;
  duration.value = 0;
  bufferedTo.value = 0;
  lastProgressAt = -1;
  shouldAutoplayNextLoad = props.autoplay;
  // `muted` means "start silent", so each file starts from the setting again rather than
  // inheriting a manual unmute from the previous one. Volume is deliberately not reset —
  // carrying it across files is what a player should do.
  isMuted.value = props.muted;
});

/**
 * Switching the setting on is a request to start playing what is already open, which is why
 * this arms the same one-shot the next source would. Turning it off leaves playback alone.
 */
watch(() => props.autoplay, (autoplay) => {
  if (!autoplay) {
    shouldAutoplayNextLoad = false;
    return;
  }

  const media = mediaRef.value;

  if (media?.paused) {
    void media.play().catch(() => {});
  }
});

if (typeof document !== 'undefined') {
  document.addEventListener('fullscreenchange', syncFullscreenState);
  // Switching away from the app puts the pointer somewhere else by definition.
  window.addEventListener('blur', markPointerIdle);
}

onBeforeUnmount(() => {
  clearIdleTimer();
  stopLoopWatch();

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
    @focusin="onFocusIn"
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
      @playing="onPlaybackActive"
      @play="onPlaybackActive"
      @pause="onPause"
      @waiting="isWaiting = true"
      @seeked="clearWaiting"
      @canplay="clearWaiting"
      @canplaythrough="clearWaiting"
      @loadeddata="clearWaiting"
      @ended="onEnded"
      @error="onError"
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
      @playing="onPlaybackActive"
      @play="onPlaybackActive"
      @pause="onPause"
      @waiting="isWaiting = true"
      @seeked="clearWaiting"
      @canplay="clearWaiting"
      @canplaythrough="clearWaiting"
      @loadeddata="clearWaiting"
      @ended="onEnded"
      @error="onError"
    />

    <!-- Audio borrows the video layout so the controls sit over a picture either way. The
         caller resolves the artwork, since only it knows the file path behind `src`. -->
    <div
      v-if="!isVideo"
      class="media-player__artwork"
    >
      <img
        v-if="poster"
        :src="poster"
        :alt="t('mediaPlayer.artwork')"
        class="media-player__artwork-image"
      >
      <Music2Icon
        v-else
        :size="64"
        class="media-player__artwork-fallback"
      />
    </div>

    <div
      v-if="isWaiting && !hasError"
      class="media-player__spinner"
      aria-hidden="true"
    >
      <Loader2Icon :size="32" />
    </div>

    <div
      class="media-player__top-controls"
    >
      <button
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
    </div>

    <div class="media-player__controls">
      <div class="media-player__row">
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
      </div>
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
        <span class="media-player__time">
          {{ formatTime(currentTime) }}/{{ formatTime(duration) }}
        </span>
        <div class="media-player__volume">
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
        <!-- Playback modes. Play-all and shuffle will join the loop toggle here. -->
        <div class="media-player__modes media-player__controls-end">
          <button
            type="button"
            class="media-player__button"
            :class="{ 'media-player__button--active': isLooping }"
            :aria-pressed="isLooping"
            :aria-label="isLooping ? t('mediaPlayer.loopOff') : t('mediaPlayer.loop')"
            :title="isLooping ? t('mediaPlayer.loopOff') : t('mediaPlayer.loop')"
            @click="toggleLoop"
          >
            <!-- Glyph carries the state, so the toggle does not read on colour alone. -->
            <component
              :is="isLooping ? Repeat1Icon : RepeatOffIcon"
              :size="18"
            />
          </button>
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
  width: 100%;
  height: 100%;
  background: black;
}

/* Sized to the pane and scaled evenly, matching how video is presented. */
.media-player__artwork {
  position: absolute;
  display: flex;
  align-items: center;
  justify-content: center;
  color: hsl(var(--foreground) / 40%);
  inset: 0;
}

.media-player__artwork-image {
  width: 100%;
  height: 100%;
  object-fit: contain;
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
  background: linear-gradient(to top, rgb(0 0 0 / 78%), transparent);
  gap: 4px;
  opacity: 0.9;
  transition: opacity 150ms ease;
}

/* Fullscreen sits over the picture rather than in the bottom bar, so it needs the same
   scrim the bottom bar gets to stay legible against a bright frame. */

.media-player__top-controls {
  position: absolute;
  z-index: 1;
  top: 0;
  right: 0;
  left: 0;
  display: flex;
  justify-content: flex-end;
  padding: 8px 12px;
  background: linear-gradient(to bottom, rgb(0 0 0 / 78%), transparent);
  opacity: 0.9;
  transition: opacity 150ms ease;
}

.media-player--controls-hidden .media-player__top-controls {
  opacity: 0;
  pointer-events: none;
}

.media-player--controls-hidden .media-player__controls {
  opacity: 0;
  pointer-events: none;
}

/* Fullscreen only: with the bar gone, a pointer left floating over the picture is the last
   thing on screen that is not the film. Any movement brings both straight back. */

.media-player--fullscreen.media-player--controls-hidden {
  cursor: none;
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

.media-player__row:has(button){
  padding: 4px 8px;
  border-radius: var(--radius-lg);
  background: hsl(var(--background) / 85%);
}

.media-player__button {
  display: flex;
  padding: 4px;
  border: none;
  border-radius: var(--radius-sm);
  color: var(--foreground);
  cursor: pointer;
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

.media-player--video .media-player__button--active,
.media-player__button--active {
  background: hsl(var(--secondary));
  color: hsl(var(--primary));
  opacity: 1;
}

.media-player__modes {
  display: flex;
  align-items: center;
  gap: 8px;
}

.media-player__controls-end {
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
  color: white;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
</style>
