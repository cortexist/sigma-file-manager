// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

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
