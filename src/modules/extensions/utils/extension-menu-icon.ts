// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

/**
 * An extension names a menu icon either by Lucide name or by a file it ships.
 *
 * Extensions arrive with their own artwork, but a contributed menu item could only ever
 * show a stock glyph, so an extension's identity stopped at its marketplace listing. An
 * icon ending in an image extension is read as an asset relative to the extension's own
 * directory; everything else stays a Lucide name, so existing manifests are unaffected.
 */

const IMAGE_FILE_PATTERN = /\.(svg|png|webp|jpe?g|gif)$/i;

export function isExtensionAssetIcon(icon: string | undefined): boolean {
  return typeof icon === 'string' && IMAGE_FILE_PATTERN.test(icon.trim());
}

/** The asset path to load, or undefined when the icon is a Lucide name. */
export function getExtensionAssetIconPath(icon: string | undefined): string | undefined {
  return isExtensionAssetIcon(icon) ? (icon as string).trim() : undefined;
}
