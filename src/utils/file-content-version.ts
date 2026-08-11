// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

/**
 * Which bytes are at a path right now.
 *
 * A file's modification time and size together are what this app already treats as its
 * content identity: the thumbnail, video-thumbnail and embedded-cover caches are all keyed on
 * `path|modified_time|size`, on both the JavaScript and the Rust side. This states the same
 * rule once, for the other half of the problem — the URLs that feed a viewer.
 *
 * Those URLs are derived from the path alone, so re-saving a file leaves the `src` string
 * byte-identical. Nothing downstream can tell that anything happened: Vue's watchers on `src`
 * never fire, the element is never asked to load again, and neither the asset protocol nor
 * the loopback media server sends validators a revalidation could act on. Appending the
 * version is what turns "the file changed" into "the URL changed", which every layer below
 * already knows how to handle.
 */

interface FileContentIdentity {
  modified_time: number;
  size: number;
}

/**
 * `null` when there is nothing to version, so a caller can leave the URL untouched rather
 * than pin it to a token that says "unknown" and never changes.
 */
export function fileContentVersion(
  entry: FileContentIdentity | null | undefined,
): string | null {
  if (!entry) {
    return null;
  }

  const modifiedTime = Number(entry.modified_time) || 0;
  const size = Number(entry.size) || 0;

  return `${modifiedTime}-${size}`;
}

/**
 * Appends the version to an asset or media URL.
 *
 * Safe for both URL shapes in use. Tauri's asset protocol reads only the path component of
 * the request URI, and the media server deserializes its query into a struct that ignores
 * unknown parameters, so neither is affected by the extra pair — it exists purely to make the
 * string differ.
 */
export function withContentVersion(url: string, version: string | null | undefined): string {
  if (!url || !version) {
    return url;
  }

  return `${url}${url.includes('?') ? '&' : '?'}v=${encodeURIComponent(version)}`;
}
