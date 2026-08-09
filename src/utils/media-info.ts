// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

/**
 * What a media file is made of, read off the file itself rather than guessed from its name.
 *
 * The backend returns raw numbers and leaves every unknown absent rather than defaulting it, so
 * the formatting — and the decision to omit a row entirely — lives here. Both the player's info
 * overlay and the info panel render from `summarizeMediaInfo`, which is what keeps a video
 * described the same way in each.
 */

import { invoke } from '@tauri-apps/api/core';

export interface MediaInfoVideoStream {
  codec: string | null;
  width: number;
  height: number;
  frameRate: number | null;
  bitrateBps: number | null;
  decoder: string | null;
}

export interface MediaInfoAudioStream {
  codec: string | null;
  channels: number;
  sampleRateHz: number;
  bitrateBps: number | null;
  decoder: string | null;
}

export interface MediaInfoImage {
  format: string | null;
  width: number;
  height: number;
  color: string | null;
  dpi: number | null;
}

export interface MediaInfo {
  container: string | null;
  durationMs: number | null;
  video: MediaInfoVideoStream[];
  audio: MediaInfoAudioStream[];
  image: MediaInfoImage | null;
}

/** A label and its value, ready to render. Translation keys, resolved by the caller. */
export interface MediaInfoRow {
  labelKey: string;
  value: string;
}

export async function readMediaInfo(path: string): Promise<MediaInfo> {
  return invoke<MediaInfo>('media_info', { path });
}

export function formatResolution(width: number, height: number): string | null {
  if (width <= 0 || height <= 0) return null;
  return `${width} × ${height}`;
}

/**
 * Bitrates are quoted in kbps below 10 Mbps and Mbps above, which is how the rest of the world
 * writes them — 192 kbps for a song, 18.3 Mbps for a camera file.
 */
export function formatBitrate(bitsPerSecond: number | null): string | null {
  if (bitsPerSecond === null || bitsPerSecond <= 0) return null;

  if (bitsPerSecond >= 10_000_000) {
    return `${(bitsPerSecond / 1_000_000).toFixed(1)} Mbps`;
  }

  return `${Math.round(bitsPerSecond / 1000)} kbps`;
}

/** 23.976 keeps its decimals; 30 does not gain any. */
export function formatFrameRate(frameRate: number | null): string | null {
  if (frameRate === null || frameRate <= 0) return null;

  const rounded = Math.round(frameRate * 1000) / 1000;
  return `${Number.isInteger(rounded) ? rounded : rounded.toFixed(3)} fps`;
}

export function formatSampleRate(sampleRateHz: number): string | null {
  if (sampleRateHz <= 0) return null;
  return `${(sampleRateHz / 1000).toFixed(1)} kHz`;
}

/** Named where a name is universally understood, and counted where it is not. */
export function formatChannels(channels: number): string | null {
  if (channels <= 0) return null;
  if (channels === 1) return 'Mono';
  if (channels === 2) return 'Stereo';
  return `${channels} channels`;
}

export function formatDpi(dpi: number | null): string | null {
  if (dpi === null || dpi <= 0) return null;
  return `${Math.round(dpi)} DPI`;
}

function pushRow(rows: MediaInfoRow[], labelKey: string, value: string | null): void {
  if (value === null) return;
  rows.push({
    labelKey,
    value,
  });
}

/**
 * Flattens whatever the file turned out to be into rows worth showing.
 *
 * Absent fields produce no row rather than a row reading "unknown": a panel of blanks tells the
 * reader less than a shorter panel does. Only the first stream of each kind is described, since
 * the second audio track of a film is a different question from "what is this file".
 */
export function summarizeMediaInfo(info: MediaInfo): MediaInfoRow[] {
  const rows: MediaInfoRow[] = [];

  if (info.image) {
    const { image } = info;
    pushRow(rows, 'mediaInfo.resolution', formatResolution(image.width, image.height));
    pushRow(rows, 'mediaInfo.encoding', image.format);
    pushRow(rows, 'mediaInfo.color', image.color);
    pushRow(rows, 'mediaInfo.density', formatDpi(image.dpi));
    return rows;
  }

  const [video] = info.video;
  const [audio] = info.audio;

  // The container only earns a row when there is no video codec already naming the format.
  if (!video) {
    pushRow(rows, 'mediaInfo.container', info.container);
  }

  if (video) {
    pushRow(rows, 'mediaInfo.resolution', formatResolution(video.width, video.height));
    pushRow(rows, 'mediaInfo.encoding', video.codec);
    pushRow(rows, 'mediaInfo.frameRate', formatFrameRate(video.frameRate));
    pushRow(rows, 'mediaInfo.bitrate', formatBitrate(video.bitrateBps));
  }

  if (audio) {
    pushRow(rows, video ? 'mediaInfo.audioEncoding' : 'mediaInfo.encoding', audio.codec);
    pushRow(rows, 'mediaInfo.channels', formatChannels(audio.channels));
    pushRow(rows, 'mediaInfo.sampleRate', formatSampleRate(audio.sampleRateHz));

    // A video's own bitrate is already listed; this one would be ambiguous beside it.
    if (!video) {
      pushRow(rows, 'mediaInfo.bitrate', formatBitrate(audio.bitrateBps));
    }
  }

  /**
   * Last, because it is the one value long enough to wrap — "VA-API H.264 Decoder in AMD Radeon
   * 780M Graphics" — and at the end it wraps without pushing anything else around.
   *
   * The picture is what people are asking about when they want to know whether a file lands on
   * a GPU, so a video's decoder wins; an audio-only file reports its own.
   */
  pushRow(rows, 'mediaInfo.decoder', video?.decoder ?? audio?.decoder ?? null);

  return rows;
}
