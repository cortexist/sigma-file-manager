// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { describe, expect, it } from 'vitest';
import { shouldKeepPlayingAfterDismissal } from '../background-playback';

describe('shouldKeepPlayingAfterDismissal', () => {
  /**
   * The gesture the whole feature exists for: Space put the window away while a file was
   * playing, and the file carries on where it can still be reached.
   */
  it('keeps a playing file alive when the window is dismissed', () => {
    expect(shouldKeepPlayingAfterDismissal({
      behavior: 'keepPlaying',
      dismissal: 'dismiss',
      isPlaying: true,
    })).toBe(true);
  });

  /**
   * Nothing to preserve means nothing to keep alive. This is also what stops a Quick View
   * showing a photo from holding the app open after the last window is gone.
   */
  it('lets the window close normally when nothing is playing', () => {
    expect(shouldKeepPlayingAfterDismissal({
      behavior: 'keepPlaying',
      dismissal: 'dismiss',
      isPlaying: false,
    })).toBe(false);

    expect(shouldKeepPlayingAfterDismissal({
      behavior: 'keepPlayingAlways',
      dismissal: 'close',
      isPlaying: false,
    })).toBe(false);
  });

  /** The default leaves one gesture that means "stop", so playback is never inescapable. */
  it('stops on the close button by default', () => {
    expect(shouldKeepPlayingAfterDismissal({
      behavior: 'keepPlaying',
      dismissal: 'close',
      isPlaying: true,
    })).toBe(false);
  });

  it('keeps playing even on the close button when asked to', () => {
    expect(shouldKeepPlayingAfterDismissal({
      behavior: 'keepPlayingAlways',
      dismissal: 'close',
      isPlaying: true,
    })).toBe(true);
  });

  it('never keeps playing once the behavior is turned off', () => {
    for (const dismissal of ['dismiss', 'close'] as const) {
      expect(shouldKeepPlayingAfterDismissal({
        behavior: 'stop',
        dismissal,
        isPlaying: true,
      })).toBe(false);
    }
  });
});
