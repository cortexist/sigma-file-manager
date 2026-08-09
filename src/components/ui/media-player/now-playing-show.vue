<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<!--
  The fullscreen audio show: artist photography under metadata that cycles a card at a time,
  the way the Zune software filled the screen once you stopped touching it.

  This is decorative — every fact on it is also in the player controls underneath — so it is
  hidden from assistive technology rather than given its own labels.

  The element stays mounted whenever audio is loaded and only toggles a class, because the
  player's fullscreen lives on an ancestor and WebKitGTK drops fullscreen when the subtree
  holding it is remounted. See the note on `shouldAutoplayNextLoad` in media-player.vue.
-->

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import type { NowPlayingCard } from '@/utils/artist-info';

const props = withDefaults(defineProps<{
  cards: NowPlayingCard[];
  photos: string[];
  active: boolean;
  /** Used when the folder holds no photography of its own, typically the album art. */
  fallbackPhoto?: string;
  /** Cover art, held to the left of the cycling text the way the original paired the two. */
  albumArt?: string;
  /** How far through the track, 0 to 1. */
  progress?: number;
  /** Elapsed and total, already formatted by the player, e.g. `3:11 / 8:22`. */
  timeLabel?: string;
}>(), {
  fallbackPhoto: undefined,
  albumArt: undefined,
  progress: 0,
  timeLabel: '',
});

/** Zune-era accents. One value drives the wash, the kicker and the rule together. */
const ACCENTS = ['#f2681b', '#e3008c', '#00a99d', '#a2c516', '#f0a30a', '#7a3b9e'];

const HOLD_MS = 4400;
const SLIDE_OUT_MS = 620;
const STAGGER_MS = 110;
const PHOTO_MS = 19000;
const ACCENT_MS = 5200;

/**
 * The cover and the transport are a beat of their own rather than permanent furniture: they
 * join the opening card, stay for a long look, and leave before anything else moves. Roughly a
 * third of the loop, which is often enough to answer "how far in am I" and rare enough that the
 * rest of the time the screen is just photography and type.
 */
const TRANSPORT_CARD_INDEX = 0;
const TRANSPORT_SETTLE_MS = 1300;
const TRANSPORT_DWELL_MS = 16000;
const TRANSPORT_FADE_MS = 600;

/**
 * A text piece crosses the whole frame in one pass rather than sliding in and parking: fast in,
 * a brief crawl through the middle, fast out the far side. The beat lasts exactly as long as the
 * last piece needs, so nothing is cut off mid-flight when the card changes.
 */
const TEXT_DRIFT_MS = 7500;
const PIECE_STAGGER_MS = 650;

const cardIndex = ref(0);
const photoIndex = ref(0);
const accentIndex = ref(0);
const isCardIn = ref(false);
const isCardOut = ref(false);
const isTransportVisible = ref(false);

let holdTimer: ReturnType<typeof setTimeout> | undefined;
let outTimer: ReturnType<typeof setTimeout> | undefined;
let transportTimer: ReturnType<typeof setTimeout> | undefined;
let exitTimer: ReturnType<typeof setTimeout> | undefined;
let photoTimer: ReturnType<typeof setInterval> | undefined;
let accentTimer: ReturnType<typeof setInterval> | undefined;

const backdrops = computed(() => (
  props.photos.length > 0
    ? props.photos
    : (props.fallbackPhoto ? [props.fallbackPhoto] : [])
));

const currentCard = computed((): NowPlayingCard | null => props.cards[cardIndex.value] ?? null);

/**
 * Suppressed when the cover is already serving as the backdrop, since a folder with no
 * photography of its own would otherwise show the same picture twice, once vast and once tiny.
 */
const coverArt = computed(() => (props.photos.length > 0 ? props.albumArt : undefined));

interface ShowLine {
  kind: 'kicker' | 'headline' | 'sub' | 'body';
  text: string;
}

/**
 * The card flattened into the order it is set in, so the entry stagger is just the index of
 * each element rather than arithmetic spread across the template. The cover and the progress
 * bar are deliberately absent: they frame the cycling text rather than cycling with it.
 */
const cardLines = computed((): ShowLine[] => {
  const card = currentCard.value;

  if (!card) {
    return [];
  }

  const lines: ShowLine[] = [];

  lines.push({
    kind: 'kicker',
    text: card.kicker,
  });

  for (const headline of card.headline) {
    lines.push({
      kind: 'headline',
      text: headline,
    });
  }

  if (card.body) {
    lines.push({
      kind: 'body',
      text: card.body,
    });
  }
  else if (card.sub) {
    lines.push({
      kind: 'sub',
      text: card.sub,
    });
  }

  return lines;
});

const accent = computed(() => ACCENTS[accentIndex.value]);

const progressWidth = computed(
  () => `${Math.min(100, Math.max(0, props.progress * 100))}%`,
);

const isTransportCard = computed(() => cardIndex.value === TRANSPORT_CARD_INDEX);

/*
 * The text beats are set differently from the transport beat: capitals at three sizes, thrown
 * across the frame from an edge, cropped by it on the way past, and blended into the photograph
 * rather than laid on top of it. Some run vertically. Diagonals are deliberately absent — the
 * original used them and they are the one thing that tips the effect into being too much.
 */

type PieceRole = 'primary' | 'secondary' | 'label' | 'prose';
type PieceEntry = 'left' | 'right' | 'top' | 'bottom';

interface PieceSlot {
  position: Record<string, string>;
  from: PieceEntry;
  vertical?: boolean;
}

interface TextPiece {
  text: string;
  role: PieceRole;
  from: PieceEntry;
  vertical: boolean;
  style: Record<string, string>;
}

const SLOTS: Record<PieceRole, PieceSlot[]> = {
  primary: [
    {
      position: {
        left: '-4%',
        top: '13%',
      },
      from: 'left',
    },
    {
      position: {
        right: '-7%',
        top: '21%',
      },
      from: 'right',
    },
    {
      position: {
        left: '5%',
        top: '30%',
      },
      from: 'left',
    },
    {
      position: {
        left: '-3%',
        top: '9%',
      },
      from: 'top',
    },
    {
      position: {
        left: '4%',
        top: '5%',
        height: '72%',
      },
      from: 'top',
      vertical: true,
    },
    {
      position: {
        right: '6%',
        top: '6%',
        height: '72%',
      },
      from: 'bottom',
      vertical: true,
    },
  ],
  secondary: [
    {
      position: {
        left: '8%',
        bottom: '20%',
      },
      from: 'left',
    },
    {
      position: {
        right: '-5%',
        bottom: '26%',
      },
      from: 'right',
    },
    {
      position: {
        left: '-2%',
        bottom: '28%',
      },
      from: 'bottom',
    },
    {
      position: {
        right: '8%',
        bottom: '22%',
      },
      from: 'bottom',
    },
    /*
     * The vertical pair. Reached only through the orientation constraint in `textPieces` —
     * never by the free draw — and sitting lower than the primary columns so the two read
     * as staggered, not aligned. Both stop short of the label's band at the frame's foot.
     */
    {
      position: {
        left: '9%',
        bottom: '16%',
        height: '52%',
      },
      from: 'bottom',
      vertical: true,
    },
    {
      position: {
        right: '9%',
        bottom: '18%',
        height: '52%',
      },
      from: 'top',
      vertical: true,
    },
  ],
  label: [
    {
      position: {
        left: '7%',
        top: '9%',
      },
      from: 'top',
    },
    {
      position: {
        right: '7%',
        top: '11%',
      },
      from: 'right',
    },
    {
      position: {
        left: '7%',
        bottom: '11%',
      },
      from: 'left',
    },
  ],
  prose: [
    {
      position: {
        left: '7%',
        bottom: '18%',
      },
      from: 'left',
    },
    {
      position: {
        right: '9%',
        bottom: '20%',
      },
      from: 'right',
    },
  ],
};

/**
 * A stable pseudo-random value for a card. Stable matters: drawing a fresh random number on
 * every render would rearrange the frame underneath the animation that is already running.
 */
function noise(index: number, salt: number): number {
  const value = Math.sin(index * 127.1 + salt * 311.7) * 43758.5453;
  return value - Math.floor(value);
}

/** Which edge a placement hangs off, read from its position rather than declared twice. */
function placementSide(position: Record<string, string>): 'left' | 'right' {
  return 'left' in position ? 'left' : 'right';
}

interface PieceConstraint {
  vertical: boolean;
  side?: 'left' | 'right';
}

function buildPiece(
  role: PieceRole,
  text: string,
  index: number,
  salt: number,
  constraint?: PieceConstraint,
): TextPiece {
  const all = SLOTS[role];
  const eligible = constraint
    ? all.filter(slot => Boolean(slot.vertical) === constraint.vertical
      && (!constraint.side || placementSide(slot.position) === constraint.side))
    : all;
  // A constraint no slot can satisfy falls back to the free draw rather than to nothing.
  const slots = eligible.length > 0 ? eligible : all;
  const slot = slots[Math.floor(noise(index, salt) * slots.length) % slots.length];

  return {
    text,
    role,
    from: slot.from,
    vertical: Boolean(slot.vertical),
    style: {
      ...slot.position,
      animationDuration: `${TEXT_DRIFT_MS}ms`,
    },
  };
}

const textPieces = computed((): TextPiece[] => {
  const card = currentCard.value;

  if (!card || isTransportCard.value) {
    return [];
  }

  const pieces: TextPiece[] = [];
  const headline = card.headline.join(' ');
  let primary: TextPiece | null = null;

  if (headline) {
    primary = buildPiece('primary', headline, cardIndex.value, 1);
    pieces.push(primary);
  }

  // Prose keeps its capitalization and its line breaks; a biography set in cropped capitals
  // would be decoration rather than something anyone could read.
  if (card.body) {
    pieces.push(buildPiece('prose', card.body, cardIndex.value, 2));
  }
  else if (card.sub) {
    /**
     * The sub follows the headline's orientation — a rotated headline over a horizontal sub
     * reads as a mistake rather than a composition. When both run vertically they take
     * opposite edges, two staggered columns bracketing the photograph instead of stacking
     * on one; horizontal headlines keep the free draw the sub always had.
     */
    const constraint: PieceConstraint | undefined = primary
      ? {
          vertical: primary.vertical,
          side: primary.vertical
            ? placementSide(primary.style) === 'left' ? 'right' : 'left'
            : undefined,
        }
      : undefined;

    pieces.push(buildPiece('secondary', card.sub, cardIndex.value, 2, constraint));
  }

  pieces.push(buildPiece('label', card.kicker, cardIndex.value, 3));

  return pieces;
});

/** Wider apart than the transport beat's lines, so the pieces cross one after another. */
function pieceDelay(index: number): string {
  return `${index * PIECE_STAGGER_MS}ms`;
}

/** Long enough for the last, most-delayed piece to finish its crossing. */
const textBeatMs = computed(
  () => TEXT_DRIFT_MS + Math.max(0, textPieces.value.length - 1) * PIECE_STAGGER_MS,
);

/**
 * Lines enter top-down and leave bottom-up, which reads as typesetting rather than as a block
 * of text sliding about.
 */
function lineDelay(index: number): string {
  const delay = isCardOut.value
    ? (cardLines.value.length - 1 - index) * (STAGGER_MS - 40)
    : index * STAGGER_MS;

  return `${delay}ms`;
}

function clearTimers() {
  clearTimeout(holdTimer);
  clearInterval(photoTimer);
  clearInterval(accentTimer);
  clearTimeout(outTimer);
  clearTimeout(transportTimer);
  clearTimeout(exitTimer);
  holdTimer = undefined;
  outTimer = undefined;
  transportTimer = undefined;
  exitTimer = undefined;
  photoTimer = undefined;
  accentTimer = undefined;
}

function exitCard() {
  isCardOut.value = true;

  outTimer = setTimeout(() => {
    cardIndex.value = (cardIndex.value + 1) % props.cards.length;
    runCard();
  }, SLIDE_OUT_MS + cardLines.value.length * STAGGER_MS);
}

function runCard() {
  if (props.cards.length === 0) return;

  isCardOut.value = false;
  isCardIn.value = false;

  // Two frames: the first commits the pre-entry position, the second animates away from it.
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      isCardIn.value = true;
    });
  });

  const carriesTransport = cardIndex.value === TRANSPORT_CARD_INDEX;

  if (carriesTransport) {
    // Only once the staggered entry has settled, so the bar never arrives onto moving text.
    transportTimer = setTimeout(() => {
      isTransportVisible.value = true;
    }, TRANSPORT_SETTLE_MS);
  }

  if (!carriesTransport) {
    // Text pieces carry themselves off the frame, so the beat simply ends when the last one has
    // finished crossing — there is nothing left on screen to animate out.
    holdTimer = setTimeout(() => {
      cardIndex.value = (cardIndex.value + 1) % props.cards.length;
      runCard();
    }, textBeatMs.value);
    return;
  }

  holdTimer = setTimeout(() => {
    /*
     * The transport leaves first and alone. Cards differ in how many headline lines they carry,
     * so text changing underneath a visible bar would shift it up and down; waiting out the
     * fade means the bar is gone before anything else moves.
     */
    isTransportVisible.value = false;
    exitTimer = setTimeout(exitCard, TRANSPORT_FADE_MS);
  }, carriesTransport ? TRANSPORT_SETTLE_MS + TRANSPORT_DWELL_MS : HOLD_MS);
}

function start() {
  clearTimers();
  cardIndex.value = 0;
  runCard();

  photoTimer = setInterval(() => {
    if (backdrops.value.length > 1) {
      photoIndex.value = (photoIndex.value + 1) % backdrops.value.length;
    }
  }, PHOTO_MS);

  accentTimer = setInterval(() => {
    accentIndex.value = (accentIndex.value + 1) % ACCENTS.length;
  }, ACCENT_MS);
}

function stop() {
  clearTimers();
  isCardIn.value = false;
  isCardOut.value = false;
  isTransportVisible.value = false;
}

watch(() => props.active, (active) => {
  if (active) {
    start();
    return;
  }

  stop();
});

// A different track mid-show restarts the sequence rather than continuing into another file's
// cards from wherever the old one had got to.
watch(() => props.cards, () => {
  if (props.active) {
    start();
  }
});

onBeforeUnmount(clearTimers);
</script>

<template>
  <div
    class="now-playing-show"
    :class="{ 'now-playing-show--active': active }"
    :style="{ '--now-playing-accent': accent }"
    aria-hidden="true"
  >
    <img
      v-for="(photo, index) in backdrops"
      :key="photo"
      :src="photo"
      class="now-playing-show__photo"
      :class="{ 'now-playing-show__photo--live': index === photoIndex }"
      alt=""
    >

    <div class="now-playing-show__tint" />
    <div class="now-playing-show__burn" />
    <div class="now-playing-show__vignette" />

    <!-- The transport beat: cover and credits together, centred. -->
    <div
      v-if="isTransportCard"
      class="now-playing-show__panel"
    >
      <img
        v-if="coverArt"
        :src="coverArt"
        class="now-playing-show__art"
        :class="{ 'now-playing-show__art--visible': isTransportVisible }"
        alt=""
      >

      <div
        class="now-playing-show__card"
        :class="{
          'now-playing-show__card--in': isCardIn,
          'now-playing-show__card--out': isCardOut,
        }"
      >
        <div
          v-for="(line, index) in cardLines"
          :key="`${cardIndex}-${index}`"
          class="now-playing-show__line"
          :class="`now-playing-show__${line.kind}`"
          :style="{ transitionDelay: lineDelay(index) }"
        >
          {{ line.text }}
        </div>
      </div>
    </div>

    <!-- The text beats: capitals thrown across the frame and cropped by it. -->
    <div
      v-else
      class="now-playing-show__pieces"
    >
      <div
        v-for="(piece, index) in textPieces"
        :key="`${cardIndex}-${index}`"
        class="now-playing-show__piece"
        :class="[
          `now-playing-show__piece--${piece.role}`,
          `now-playing-show__piece--from-${piece.from}`,
          { 'now-playing-show__piece--vertical': piece.vertical },
        ]"
        :style="[piece.style, { animationDelay: pieceDelay(index) }]"
      >
        {{ piece.text }}
      </div>
    </div>

    <!-- Along the foot of the frame, where a transport belongs, rather than in the middle. -->
    <div
      class="now-playing-show__transport"
      :class="{ 'now-playing-show__transport--visible': isTransportVisible }"
    >
      <div class="now-playing-show__progress">
        <span :style="{ width: progressWidth }" />
      </div>
      <div
        v-if="timeLabel"
        class="now-playing-show__clock"
      >
        {{ timeLabel }}
      </div>
    </div>
  </div>
</template>

<style scoped>
.now-playing-show {
  --now-playing-text-opacity: 0.15;

  position: absolute;
  z-index: 2;
  overflow: hidden;
  background: black;
  color: white;
  inset: 0;
  opacity: 0;
  pointer-events: none;
  transition: opacity 900ms ease;
}

.now-playing-show--active {
  opacity: 1;
}

/*
 * The pan is carried by every photo rather than only the visible one, and is paused rather
 * than removed while the show is down. Both are for the same reason: taking an animation off
 * an element snaps its transform back to the start immediately. Binding it to the live photo
 * alone did exactly that to the outgoing picture, mid-crossfade, so every photo change was
 * preceded by a visible jolt. A paused animation holds its current transform instead, and a
 * photo on its way out keeps drifting for the whole dissolve.
 */

/*
 * If this animation ever looks jerky, check how the app was launched before touching it:
 * `tauri:dev:webkit-igpu` sets WEBKIT_DISABLE_COMPOSITING_MODE=1, under which every CSS
 * animation is software-rendered frame by frame and nothing here can be smooth.
 */

.now-playing-show__photo {
  position: absolute;
  width: 100%;
  height: 100%;
  animation: now-playing-pan 20s ease-in-out infinite alternate;
  animation-play-state: paused;

  /* Raised from 35%: with the tint no longer blending, the photo carries more of the color. */
  filter: saturate(60%) contrast(112%) brightness(62%);
  inset: 0;
  object-fit: cover;
  opacity: 0;
  transition: opacity 2600ms ease-in-out;
}

.now-playing-show__photo--live {
  opacity: 1;
}

.now-playing-show--active .now-playing-show__photo {
  animation-play-state: running;
}

/*
 * The zoom has to stay ahead of the drift or the picture's edge walks into frame: a translation
 * of t% needs a scale of at least 1 + 2t/100 to keep the photo covering the frame, and both
 * values interpolate, so the tightest point is the small-scale end. 1.08 against 3% has enough
 * margin; shrinking the starting scale without shrinking the drift would not.
 */

/*
 * The zoom has to stay ahead of the drift or the picture's edge walks into frame: a translation
 * of t% needs a scale of at least 1 + 2t/100 to keep the photo covering the frame, and both
 * values interpolate, so the tightest point is the small-scale end. 1.08 against 3% has enough
 * margin; shrinking the starting scale without shrinking the drift would not.
 */
@keyframes now-playing-pan {
  from { transform: scale(1.08) translate3d(-3%, 1.8%, 0); }

  to { transform: scale(1.28) translate3d(4.5%, -3.5%, 0); }
}

.now-playing-show__tint,
.now-playing-show__burn,
.now-playing-show__vignette {
  position: absolute;
  inset: 0;
}

/* One shared accent behind the whole frame is what reads as the screen changing hue, rather
   than a tinted picture sitting behind static furniture. */

/*
 * Plain alpha, not `mix-blend-mode`.
 *
 * The original `color` blend kept the photograph's luminance and took only hue and saturation
 * from the accent; a blended layer also needs a backdrop readback and re-blend on every frame
 * the panning photo changes, where an alpha wash composites for free. The look was retuned for
 * the cheaper form — wash lightened, photo saturation raised from 35% to 60% — and that color
 * treatment is the one that was signed off, so keep the two halves together if changing either.
 */

.now-playing-show__tint {
  background: var(--now-playing-accent);
  opacity: 0.22;
  transition: background-color 5200ms linear;
}

/* Same reasoning as the tint: a soft-light blend over moving photography is a per-frame
   readback. As plain alpha it is a gentle highlight rather than a light-preserving burn. */

.now-playing-show__burn {
  background: radial-gradient(
    120% 90% at 78% 12%,
    color-mix(in srgb, var(--now-playing-accent) 55%, transparent),
    transparent 35%
  );
  opacity: 0.25;
  transition: background 5200ms linear;
}

.now-playing-show__vignette {
  background:
    linear-gradient(to right, rgb(0 0 0 / 72%) 0%, rgb(0 0 0 / 28%) 38%, rgb(0 0 0 / 0%) 70%),
    linear-gradient(to top, rgb(0 0 0 / 66%) 0%, rgb(0 0 0 / 0%) 46%);
}

/* One composition rather than two states: the cover and the transport frame a column of type,
   and only that column cycles. Anchored on the left axis and sized in viewport units so it
   reads the same windowed and fullscreen. */

/* Sits above the transport rather than in the middle of the frame, leaving the same gap between
   the credits and the bar as the bar leaves beneath itself. */

.now-playing-show__panel {
  position: absolute;
  right: 7%;
  bottom: 12%;
  left: 7%;
  display: flex;
  align-items: flex-end;
  gap: clamp(16px, 4vh, 52px);
}

.now-playing-show__art {
  width: clamp(64px, 24vh, 240px);
  height: clamp(64px, 24vh, 240px);
  flex: none;
  box-shadow: 0 1.6vh 5vh rgb(0 0 0 / 55%);
  object-fit: cover;
  opacity: 0;
  transition: opacity 600ms ease;
}

.now-playing-show__art--visible {
  opacity: 1;
}

.now-playing-show__card {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 0.4vh;
}

.now-playing-show__line {
  opacity: 0;
  transform: translate3d(-2.6vw, 0, 0);
  transition:
    opacity 760ms cubic-bezier(0.16, 1, 0.3, 1),
    transform 900ms cubic-bezier(0.16, 1, 0.3, 1);
}

.now-playing-show__card--in .now-playing-show__line {
  opacity: 1;
  transform: none;
}

.now-playing-show__card--out .now-playing-show__line {
  opacity: 0;
  transform: translate3d(2.2vw, 0, 0);
  transition-duration: 620ms, 620ms;
}

/* Keeps its space in the column at all times; only its opacity changes, so the text above it
   never reflows when the beat begins or ends. */

/* Along the foot of the frame. Sitting under the credits in the middle of the screen made it
   read as part of the type rather than as a transport. */

.now-playing-show__transport {
  position: absolute;
  right: 7%;
  bottom: 6%;
  left: 7%;
  display: flex;
  align-items: center;
  gap: clamp(10px, 1.6vw, 24px);
  opacity: 0;
  transition: opacity 600ms ease;
}

.now-playing-show__transport--visible {
  opacity: 1;
}

/*
 * Text beats. Capitals at three weights of emphasis, entering from whichever edge their slot
 * came from and carrying on out the far side, cropped by the frame the whole way. `overlay`
 * blending is what mixes them into the photograph instead of stacking them on top of it — the
 * letters take the picture's light, which is why they read as gold over a bright area and white
 * over a dark one.
 */

.now-playing-show__pieces {
  position: absolute;
  inset: 0;
}

/*
 * Each piece makes one crossing of the frame: on fast from its edge, a short crawl through the
 * middle, then away out the far side without stopping. Travel is in viewport units rather than
 * percentages of the element, so a short word crosses the same distance a long one does.
 *
 * `--now-playing-text-opacity` is the single knob for how far the letters sit into the
 * photograph. It multiplies with the color alpha and the overlay blend, so small values here
 * go a long way.
 */

.now-playing-show__piece {
  position: absolute;
  animation-fill-mode: both;
  animation-play-state: paused;

  /*
   * One curve for the whole crossing rather than keyframed segments. A timing function set
   * inside a keyframe governs only the segment starting there, so the old interior keyframes
   * split the travel into stretches at different speeds, and the velocity jumped where two of
   * them met — that corner was the pause, not the slowness. This bezier is the inverse of an
   * ease-in-out: fast in, a long slack middle, fast out, with no discontinuity anywhere.
   * Opacity rides a second animation so that its keyframes cannot re-segment this one.
   */
  animation-timing-function: cubic-bezier(0, 0.72, 1, 0.28), linear;
  color: rgb(255 255 255 / 92%);
  font-weight: 700;
  letter-spacing: -0.01em;
  line-height: 0.92;
  mix-blend-mode: overlay;
  opacity: 0;
  text-transform: uppercase;
  white-space: nowrap;
}

.now-playing-show--active .now-playing-show__piece {
  animation-play-state: running;
}

.now-playing-show__piece--from-left {
  animation-name: now-playing-cross-right, now-playing-piece-fade;
}

.now-playing-show__piece--from-right {
  animation-name: now-playing-cross-left, now-playing-piece-fade;
}

.now-playing-show__piece--from-top {
  animation-name: now-playing-cross-down, now-playing-piece-fade;
}

.now-playing-show__piece--from-bottom {
  animation-name: now-playing-cross-up, now-playing-piece-fade;
}

@keyframes now-playing-cross-right {
  from { transform: translate3d(-108vw, 0, 0); }

  to { transform: translate3d(108vw, 0, 0); }
}

@keyframes now-playing-cross-left {
  from { transform: translate3d(108vw, 0, 0); }

  to { transform: translate3d(-108vw, 0, 0); }
}

@keyframes now-playing-cross-down {
  from { transform: translate3d(0, -112vh, 0); }

  to { transform: translate3d(0, 112vh, 0); }
}

@keyframes now-playing-cross-up {
  from { transform: translate3d(0, 112vh, 0); }

  to { transform: translate3d(0, -112vh, 0); }
}

/* Well inside the travel at both ends, so a piece is already moving when it appears and still
   moving when it goes, rather than materialising at a standstill. */
@keyframes now-playing-piece-fade {
  0%, 100% { opacity: 0; }

  14%, 86% { opacity: var(--now-playing-text-opacity); }
}

.now-playing-show__piece--primary {
  font-size: clamp(56px, 21vh, 270px);
}

.now-playing-show__piece--secondary {
  color: rgb(255 255 255 / 80%);
  font-size: clamp(32px, 14vh, 180px);
}

/* The label keeps the accent and stays out of the blend, so there is always one element on the
   frame that is plainly legible rather than mixed into the picture. */

.now-playing-show__piece--label {
  color: var(--now-playing-accent);
  font-size: clamp(24px, 4vh, 56px);
  letter-spacing: 0.3em;
  mix-blend-mode: normal;
  transition:
    opacity 900ms cubic-bezier(0.16, 1, 0.3, 1),
    transform 1500ms cubic-bezier(0.16, 1, 0.3, 1),
    color 5200ms linear;
}

/* Prose stays at a reading weight; a whole biography set in bold would shout. */
.now-playing-show__piece--prose {
  max-width: 34ch;
  color: rgb(255 255 255 / 82%);
  font-size: clamp(14px, 3vh, 34px);
  font-weight: 400;
  line-height: 1.4;
  text-transform: none;
  white-space: normal;
}

.now-playing-show__piece--vertical {
  writing-mode: vertical-rl;
}

.now-playing-show__progress {
  height: 3px;
  flex: 1;
  background: rgb(255 255 255 / 18%);
}

.now-playing-show__progress span {
  display: block;
  height: 100%;
  background: var(--now-playing-accent);
  transition:
    width 300ms linear,
    background-color 5200ms linear;
}

.now-playing-show__clock {
  flex: none;
  color: rgb(255 255 255 / 55%);
  font-size: clamp(11px, 2vh, 22px);
  font-variant-numeric: tabular-nums;
}

.now-playing-show__kicker {
  margin-bottom: 1.1vh;
  color: var(--now-playing-accent);
  font-size: clamp(11px, 2vh, 22px);
  letter-spacing: 0.3em;
  text-transform: uppercase;
  transition:
    color 5200ms linear,
    opacity 760ms cubic-bezier(0.16, 1, 0.3, 1),
    transform 900ms cubic-bezier(0.16, 1, 0.3, 1);
}

/* Smaller than it was when the frame cropped it, because the text now has to live inside a
   fixed column beside the cover instead of running off the right edge. */

.now-playing-show__headline {
  font-size: clamp(28px, 8vh, 96px);
  font-weight: 200;
  letter-spacing: -0.035em;
  line-height: 0.98;
}

.now-playing-show__sub {
  margin-top: 1.4vh;
  color: rgb(255 255 255 / 66%);
  font-size: clamp(14px, 3.4vh, 40px);
}

/* Prose is the one thing allowed to wrap, since a biography has no natural break to set. */
.now-playing-show__body {
  max-width: 42ch;
  margin-top: 1.4vh;
  color: rgb(255 255 255 / 66%);
  font-size: clamp(13px, 2.6vh, 30px);
  line-height: 1.45;
  white-space: normal;
}

@media (prefers-reduced-motion: reduce) {
  .now-playing-show__photo,
  .now-playing-show--active .now-playing-show__photo {
    animation: none;
  }

  .now-playing-show__line,
  .now-playing-show__card--in .now-playing-show__line,
  .now-playing-show__card--out .now-playing-show__line {
    transform: none;
    transition-duration: 320ms;
  }

  /* Pieces stay where their slot puts them and only fade, rather than sweeping the frame. */
  .now-playing-show__piece {
    animation: none;
    opacity: var(--now-playing-text-opacity);
    transform: none;
  }
}
</style>
