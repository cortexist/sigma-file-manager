// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { ref } from 'vue';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import {
  ensurePlatformInfo,
  getPlatformInfo,
  isPlatformInfoInitialized,
} from '@/utils/platform-info';

/**
 * Video and audio cannot be played from the `asset://` protocol on Linux: WebKitGTK
 * delegates media loading to GStreamer, which has no source element for custom URI
 * schemes, so the media element fails with a format error while images on the same
 * protocol load fine (WebKit bug 146351, tauri-apps/tauri#3725). The Rust side exposes a
 * loopback HTTP server for these files instead, which also gives us real range requests
 * so seeking does not re-read the file from the beginning.
 *
 * Windows and macOS use WebView2/WKWebView, where `asset://` media works, so they keep
 * the existing path and never start the server.
 */
const mediaServerOrigin = ref<string | null>(null);
let mediaServerPromise: Promise<string | null> | null = null;

function requiresMediaServer(): boolean {
  return getPlatformInfo().isLinux;
}

export async function initMediaServer(): Promise<string | null> {
  if (mediaServerOrigin.value) {
    return mediaServerOrigin.value;
  }

  // Resolve the platform first: reading it synchronously before init completes reports
  // the Windows default, which would silently skip the server on Linux.
  await ensurePlatformInfo();

  if (!requiresMediaServer()) {
    return null;
  }

  if (!mediaServerPromise) {
    mediaServerPromise = (async () => {
      try {
        const origin = await invoke<string>('get_media_server_origin');
        mediaServerOrigin.value = origin;
        return origin;
      }
      catch (error) {
        console.error('Failed to start media server, falling back to asset protocol:', error);
        return null;
      }
      finally {
        mediaServerPromise = null;
      }
    })();
  }

  return mediaServerPromise;
}

/**
 * Resolves a local file path to a URL a `<video>` or `<audio>` element can play.
 *
 * Reads a ref, so Vue computeds calling this re-evaluate once the server origin arrives.
 * Falls back to the asset protocol when the server is unavailable or not needed.
 */
export function convertMediaSrc(path: string): string {
  if (!path) return '';

  // Read the ref unconditionally so Vue tracks it even when the platform is not known
  // yet, otherwise a computed resolved during startup would never re-run.
  const origin = mediaServerOrigin.value;

  if (origin) {
    return `${origin}?path=${encodeURIComponent(path)}`;
  }

  if (!isPlatformInfoInitialized() || requiresMediaServer()) {
    // Kick off startup so the next render picks up the real URL.
    void initMediaServer();
  }

  return convertFileSrc(path);
}

export function isMediaServerUrl(url: string): boolean {
  const origin = mediaServerOrigin.value;
  return Boolean(origin) && url.startsWith(`${origin}?`);
}
