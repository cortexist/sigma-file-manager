// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import type { QuickViewFileType } from '@/stores/runtime/quick-view';
import type { QuickViewPlaybackOnDismiss } from '@/types/user-settings';

/**
 * How the window is being put away. The close button is a decision to be finished with the
 * file; Space, Escape and the main window's toggle are a decision to stop looking at it,
 * which is not the same thing while something is playing.
 */
export type QuickViewDismissal = 'dismiss' | 'close';

/**
 * Whether playback should outlive the window being put away this particular way.
 *
 * Nothing playing means nothing to preserve, so this is false for a still image or a paused
 * video whatever the setting says — background playback is about sound and motion carrying
 * on, not about keeping windows alive on principle. It is also what keeps the app from being
 * held open by a Quick View that has nothing to play.
 */
export function shouldKeepPlayingAfterDismissal(options: {
  behavior: QuickViewPlaybackOnDismiss;
  dismissal: QuickViewDismissal;
  isPlaying: boolean;
}): boolean {
  if (!options.isPlaying) {
    return false;
  }

  if (options.behavior === 'stop') {
    return false;
  }

  // The close button keeps its meaning unless the user has asked for the opposite: it is the
  // one gesture that says "done with this file" rather than "hide it".
  if (options.dismissal === 'close' && options.behavior !== 'keepPlayingAlways') {
    return false;
  }

  return true;
}

/** The kinds of file the player is mounted for; everything else is looked at, not listened to. */
export function isPlaybackFileType(type: QuickViewFileType): boolean {
  return type === 'audio' || type === 'video';
}

/**
 * Which file the player keeps mounted *behind* the view once `incoming` takes it.
 *
 * Quick View shows one thing, but it need not silence another to do so: a text file opened
 * over a playing song is the same gesture as putting the window away while it plays — the
 * user has stopped looking, not stopped listening — so the song goes behind the view under
 * the same setting and carries on. A playback file, on the other hand, always takes the
 * player for itself: one player, so whatever was behind the view is over, and the very file
 * that was behind it is simply shown where it is.
 *
 * Returns the path the player should stay mounted for out of sight, or null for none.
 */
export function backgroundPlayerAfterViewChange(options: {
  /** The file in view now, if any, and whether it is the player's kind of file. */
  displayed: {
    path: string;
    isPlayback: boolean;
  } | null;
  /** The file already playing behind the view, if any. */
  background: string | null;
  incoming: {
    path: string;
    isPlayback: boolean;
  };
  behavior: QuickViewPlaybackOnDismiss;
  /** Whether the player — in view or behind it — is playing right now. */
  isPlaying: boolean;
}): string | null {
  if (options.incoming.isPlayback) {
    return null;
  }

  const { displayed } = options;

  if (!displayed?.isPlayback) {
    return options.background;
  }

  if (displayed.path === options.incoming.path) {
    return null;
  }

  const keepPlaying = shouldKeepPlayingAfterDismissal({
    behavior: options.behavior,
    dismissal: 'dismiss',
    isPlaying: options.isPlaying,
  });

  return keepPlaying ? displayed.path : null;
}
