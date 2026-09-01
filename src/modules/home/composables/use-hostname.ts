// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { ref } from 'vue';
import { hostname } from '@tauri-apps/plugin-os';

/**
 * The machine's hostname, resolved once per app run.
 *
 * Shared at module scope rather than per-caller: the home page and the home
 * banner both render the heading, and the name cannot change while the app is
 * open, so a second IPC round trip would buy nothing.
 *
 * Starts `null` and stays `null` if the lookup fails or returns nothing —
 * callers fall back to their previous static heading rather than rendering an
 * empty title.
 */
const resolvedHostname = ref<string | null>(null);
let lookup: Promise<void> | null = null;

export function useHostname() {
  lookup ??= hostname()
    .then((name) => {
      const trimmed = name?.trim();

      if (trimmed) {
        resolvedHostname.value = trimmed;
      }
    })
    .catch(() => {
      // Left null on purpose. A missing hostname is not worth an error surface
      // on the home page — the caller's fallback heading covers it.
    });

  return { hostname: resolvedHostname };
}
