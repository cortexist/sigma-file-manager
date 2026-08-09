// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

/**
 * Resolves once this window has produced two frames, or after the timeout if it never does.
 *
 * `show()` resolving on the caller's side only means the request reached the compositor; it
 * says nothing about whether this window has a surface and a GL context yet. Frames are the
 * one signal that cannot lie: `requestAnimationFrame` fires only when the window is actually
 * being composited, and the second frame guards against a single one dispatched while the
 * pipeline is still coming up.
 *
 * The timeout is the degraded path, not the expected one — a window that never becomes
 * visible would otherwise park the caller forever.
 */
export function waitForFirstPaint(timeoutMs: number): Promise<void> {
  return new Promise((resolve) => {
    let settled = false;

    function settle() {
      if (settled) return;
      settled = true;
      window.clearTimeout(fallback);
      resolve();
    }

    const fallback = window.setTimeout(settle, timeoutMs);

    requestAnimationFrame(() => {
      requestAnimationFrame(settle);
    });
  });
}
