// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { describe, expect, it } from 'vitest';
import {
  backgroundPlayerAfterViewChange,
  isPlaybackFileType,
  shouldKeepPlayingAfterDismissal,
} from '../background-playback';

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

describe('backgroundPlayerAfterViewChange', () => {
  const SONG = '/home/user/Music/song.flac';
  const OTHER_SONG = '/home/user/Music/other.flac';
  const NOTES = '/home/user/Documents/notes.md';
  const README = '/home/user/Documents/readme.md';

  function playback(path: string) {
    return {
      path,
      isPlayback: true,
    };
  }

  function document(path: string) {
    return {
      path,
      isPlayback: false,
    };
  }

  /**
   * The case the slot exists for: a text file opened over a playing song is a decision to
   * stop looking, not to stop listening, so the song stays mounted behind the view.
   */
  it('moves a playing file behind the view when a document takes it', () => {
    expect(backgroundPlayerAfterViewChange({
      displayed: playback(SONG),
      background: null,
      incoming: document(NOTES),
      behavior: 'keepPlaying',
      isPlaying: true,
    })).toBe(SONG);
  });

  /** Moving between documents leaves whatever is playing behind them alone. */
  it('keeps the background player across documents', () => {
    expect(backgroundPlayerAfterViewChange({
      displayed: document(NOTES),
      background: SONG,
      incoming: document(README),
      behavior: 'keepPlaying',
      isPlaying: true,
    })).toBe(SONG);
  });

  /**
   * One player: a playback file taking the view ends whatever was behind it — and when it is
   * the very file that was behind the view, it is simply shown where it is.
   */
  it('clears the slot whenever a playback file takes the view', () => {
    expect(backgroundPlayerAfterViewChange({
      displayed: document(NOTES),
      background: SONG,
      incoming: playback(OTHER_SONG),
      behavior: 'keepPlaying',
      isPlaying: true,
    })).toBeNull();

    expect(backgroundPlayerAfterViewChange({
      displayed: document(NOTES),
      background: SONG,
      incoming: playback(SONG),
      behavior: 'keepPlaying',
      isPlaying: true,
    })).toBeNull();
  });

  /** The same rule as dismissing the window: nothing playing, or told to stop, means nothing kept. */
  it('drops a file that is not playing, or that the setting says to stop', () => {
    expect(backgroundPlayerAfterViewChange({
      displayed: playback(SONG),
      background: null,
      incoming: document(NOTES),
      behavior: 'keepPlaying',
      isPlaying: false,
    })).toBeNull();

    expect(backgroundPlayerAfterViewChange({
      displayed: playback(SONG),
      background: null,
      incoming: document(NOTES),
      behavior: 'stop',
      isPlaying: true,
    })).toBeNull();
  });

  it('knows which kinds of file the player is for', () => {
    expect(isPlaybackFileType('audio')).toBe(true);
    expect(isPlaybackFileType('video')).toBe(true);
    expect(isPlaybackFileType('text')).toBe(false);
    expect(isPlaybackFileType('image')).toBe(false);
  });
});
