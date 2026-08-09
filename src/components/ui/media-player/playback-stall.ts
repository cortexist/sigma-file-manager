// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

/**
 * Decides when playback has wedged badly enough that the pipeline must be rebuilt.
 *
 * WebKitGTK can leave a media element reporting `paused === false` while the clock never
 * advances — no buffering event, no error, just a spinner that runs forever. Nothing addressed
 * to the element helps: pause/play toggles the glyph and moves nothing. The one lever that
 * reliably clears it is rebuilding the pipeline — `load()` on the same source — which is what
 * closing and reopening the viewer does by hand.
 *
 * The decision lives here as a pure function because the real stall cannot be triggered on
 * demand: the only way to trust the watchdog is to test every path of the decision itself.
 */

export interface PlaybackSample {
  paused: boolean;
  ended: boolean;
  currentTime: number;
}

export interface StallWatch {
  /** The clock reading progress was last seen at; `null` while playback is not running. */
  baselineTime: number | null;
  stalledMs: number;
  /** Rebuilds spent on the current source. The cap keeps a truly dead file from looping. */
  recoveries: number;
}

export interface StallWatchOptions {
  /** How long the clock must sit frozen, while claiming to play, before a rebuild. */
  graceMs: number;
  maxRecoveries: number;
}

export interface StallVerdict {
  watch: StallWatch;
  /** True exactly when the caller should rebuild the pipeline now. */
  recover: boolean;
}

export function newStallWatch(): StallWatch {
  return {
    baselineTime: null,
    stalledMs: 0,
    recoveries: 0,
  };
}

/**
 * Rebuild a pipeline that claims to play while its clock never moves.
 *
 * `load()` on the unchanged source is the programmatic form of the one thing proven to clear
 * these stalls — closing and reopening the viewer, which also just builds a fresh pipeline.
 * Nothing gentler works: pause/play reaches the element and moves nothing, and there is no
 * error or buffering event to react to.
 */
export function rebuildWedgedPipeline(media: HTMLMediaElement): void {
  const position = media.currentTime;

  function resume() {
    media.removeEventListener('loadedmetadata', resume);

    if (position > 0) {
      media.currentTime = position;
    }

    void media.play().catch(() => {
      // Refused: leave it honestly paused rather than claiming to play a dead pipeline.
      media.pause();
    });
  }

  media.addEventListener('loadedmetadata', resume);
  media.load();
}

/**
 * Feeds one periodic reading of the element into the watch.
 *
 * Progress is the only trustworthy signal on this stack — `readyState` sits at 2 for entire
 * files and events are skipped — so the sole question asked is whether the clock moved since
 * the last reading. Paused and ended are not stalls: a paused clock is what pause means, and
 * `ended` has its own handling.
 */
export function observePlayback(
  watch: StallWatch,
  sample: PlaybackSample,
  elapsedMs: number,
  options: StallWatchOptions,
): StallVerdict {
  if (sample.paused || sample.ended) {
    return {
      watch: {
        ...watch,
        baselineTime: null,
        stalledMs: 0,
      },
      recover: false,
    };
  }

  if (watch.baselineTime === null || sample.currentTime !== watch.baselineTime) {
    return {
      watch: {
        ...watch,
        baselineTime: sample.currentTime,
        stalledMs: 0,
      },
      recover: false,
    };
  }

  const stalledMs = watch.stalledMs + elapsedMs;

  if (stalledMs >= options.graceMs && watch.recoveries < options.maxRecoveries) {
    return {
      watch: {
        baselineTime: sample.currentTime,
        stalledMs: 0,
        recoveries: watch.recoveries + 1,
      },
      recover: true,
    };
  }

  return {
    watch: {
      ...watch,
      stalledMs,
    },
    recover: false,
  };
}
