// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { invoke } from '@tauri-apps/api/core';
import { ensurePlatformInfo } from '@/utils/platform-info';

/**
 * Reads the frame a `<video>` is currently showing back out of the element.
 *
 * The frame is taken at the file's own pixel dimensions rather than the size the element
 * happens to be drawn at, so a still captured from a small window is still the full picture.
 *
 * Not usable on Linux: WebKitGTK decodes video straight into a GPU buffer that JavaScript
 * cannot sample, so this returns a black rectangle there rather than failing outright. See
 * `copyCurrentVideoFrameToClipboard`, and `src-tauri/src/video_thumbnails.rs` for the same
 * problem in its original form.
 */
export async function encodeCurrentVideoFrameToPng(video: HTMLVideoElement): Promise<Uint8Array> {
  const width = video.videoWidth;
  const height = video.videoHeight;

  // Zero until the first frame is decoded, which is also the only state where there is
  // genuinely nothing to capture.
  if (width === 0 || height === 0) {
    throw new Error('The video has no decoded frame to capture yet');
  }

  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;

  const context = canvas.getContext('2d');

  if (!context) {
    throw new Error('Failed to obtain a 2D canvas context for the frame capture');
  }

  context.drawImage(video, 0, 0, width, height);

  const blob = await new Promise<Blob | null>((resolve) => {
    canvas.toBlob(resolve, 'image/png');
  });

  if (!blob) {
    throw new Error('Failed to encode the captured frame as a PNG');
  }

  return new Uint8Array(await blob.arrayBuffer());
}

/**
 * Puts the frame a `<video>` is currently showing on the system clipboard as an image, so it
 * can be pasted straight into another application.
 *
 * Linux decodes the frame natively from `sourcePath` rather than reading it out of the
 * element, for the reason `encodeCurrentVideoFrameToPng` describes: the pixels the webview
 * has are not reachable from JavaScript. The playback position is the only thing the element
 * is asked for there, so the native decoder lands on the frame that was on screen. This is
 * the same split video thumbnails already make.
 */
export async function copyCurrentVideoFrameToClipboard(
  video: HTMLVideoElement,
  sourcePath: string,
): Promise<void> {
  if ((await ensurePlatformInfo()).isLinux) {
    await invoke('copy_video_frame_to_system_clipboard', {
      path: sourcePath,
      positionSeconds: video.currentTime,
    });

    return;
  }

  const pngBytes = await encodeCurrentVideoFrameToPng(video);

  await invoke('set_system_clipboard_image_from_png_bytes', {
    pngBytes: Array.from(pngBytes),
  });
}
