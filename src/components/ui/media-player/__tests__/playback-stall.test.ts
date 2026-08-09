// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { describe, expect, it } from 'vitest';
import {
  newStallWatch,
  observePlayback,
  type PlaybackSample,
  type StallWatch,
} from '../playback-stall';

const OPTIONS = {
  graceMs: 3000,
  maxRecoveries: 2,
};

function playingAt(currentTime: number): PlaybackSample {
  return {
    paused: false,
    ended: false,
    currentTime,
  };
}

/** Runs `ticks` readings of the same sample, returning the state and how many rebuilds fired. */
function run(watch: StallWatch, sample: PlaybackSample, ticks: number) {
  let recoveries = 0;

  for (let i = 0; i < ticks; i++) {
    const verdict = observePlayback(watch, sample, 500, OPTIONS);
    watch = verdict.watch;
    if (verdict.recover) recoveries++;
  }

  return {
    watch,
    recoveries,
  };
}

describe('observePlayback', () => {
  it('recovers once the clock has sat frozen for the grace period while playing', () => {
    // First reading sets the baseline; six more at 500ms reach the 3s grace.
    const { recoveries } = run(newStallWatch(), playingAt(0), 7);

    expect(recoveries).toBe(1);
  });

  it('never recovers while the clock is moving', () => {
    let watch = newStallWatch();
    let recovered = false;

    for (let tick = 0; tick < 100; tick++) {
      const verdict = observePlayback(watch, playingAt(tick * 0.5), 500, OPTIONS);
      watch = verdict.watch;
      recovered ||= verdict.recover;
    }

    expect(recovered).toBe(false);
  });

  /** A paused clock is what pause means; treating it as a stall would rebuild on every pause. */
  it('does not count paused or ended time as a stall', () => {
    const paused: PlaybackSample = {
      paused: true,
      ended: false,
      currentTime: 5,
    };
    const ended: PlaybackSample = {
      paused: false,
      ended: true,
      currentTime: 5,
    };

    expect(run(newStallWatch(), paused, 50).recoveries).toBe(0);
    expect(run(newStallWatch(), ended, 50).recoveries).toBe(0);
  });

  /** Pausing must also clear an accumulating stall rather than let it fire on resume. */
  it('starts the grace period over after a pause', () => {
    // Almost stalled long enough...
    let { watch } = run(newStallWatch(), playingAt(0), 6);

    // ...then paused, then playing again from the same frozen clock.
    ({ watch } = run(watch, {
      paused: true,
      ended: false,
      currentTime: 0,
    }, 1));
    const resumed = run(watch, playingAt(0), 6);

    // One tick to re-baseline, so six more have not yet reached the grace.
    expect(resumed.recoveries).toBe(0);
    expect(run(resumed.watch, playingAt(0), 1).recoveries).toBe(1);
  });

  /** A file nothing can play must not be rebuilt in a loop forever. */
  it('stops recovering at the cap', () => {
    const { recoveries } = run(newStallWatch(), playingAt(0), 1000);

    expect(recoveries).toBe(OPTIONS.maxRecoveries);
  });

  it('a moving clock re-arms the stall detection but keeps the recovery count', () => {
    // Two recoveries spend the cap...
    let { watch } = run(newStallWatch(), playingAt(0), 1000);

    // ...progress happens, then the clock freezes again: the cap still holds.
    const verdict = observePlayback(watch, playingAt(3), 500, OPTIONS);
    watch = verdict.watch;

    expect(run(watch, playingAt(3), 1000).recoveries).toBe(0);
  });
});
