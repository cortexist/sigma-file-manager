// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

/**
 * What the window manager shows for Quick View. It names the viewer, not the file manager:
 * Quick View is a shared window — other applications open files in it — and it matches the
 * desktop entry those applications see (`Name=Sigma Quick View`). Deliberately not localized,
 * like the application name itself.
 */
export const QUICK_VIEW_WINDOW_TITLE = 'Sigma Quick View';

export function quickViewWindowTitle(fileName?: string | null): string {
  return fileName ? `${QUICK_VIEW_WINDOW_TITLE} - ${fileName}` : QUICK_VIEW_WINDOW_TITLE;
}
