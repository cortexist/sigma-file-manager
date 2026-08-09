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

export interface MediaExif {
  camera: string | null;
  lens: string | null;
  takenAt: string | null;
  exposureTime: string | null;
  fNumber: string | null;
  iso: string | null;
  focalLength: string | null;
  /** Signed decimal degrees, south and west negative. Usually stripped before sharing. */
  latitude: number | null;
  longitude: number | null;
  software: string | null;
}

export interface MediaInfoImage {
  format: string | null;
  width: number;
  height: number;
  color: string | null;
  dpi: number | null;
  exif: MediaExif | null;
}

export interface MediaTags {
  title: string | null;
  artist: string | null;
  album: string | null;
  albumArtist: string | null;
  composer: string | null;
  genre: string | null;
  trackNumber: number | null;
  year: number | null;
  encoder: string | null;
}

export interface MediaInfo {
  container: string | null;
  durationMs: number | null;
  video: MediaInfoVideoStream[];
  audio: MediaInfoAudioStream[];
  image: MediaInfoImage | null;
  tags: MediaTags | null;
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

/**
 * Decimal degrees, which is the form that can be pasted straight into a map. Six places is
 * about a tenth of a metre — past the point any camera's fix is meaningful, and short enough
 * to read. A latitude without its longitude locates nothing, so both must be present.
 */
export function formatCoordinates(
  latitude: number | null,
  longitude: number | null,
): string | null {
  if (latitude === null || longitude === null) return null;
  if (!Number.isFinite(latitude) || !Number.isFinite(longitude)) return null;

  return `${latitude.toFixed(6)}, ${longitude.toFixed(6)}`;
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
 * What the camera recorded, ahead of what the file is made of — for a photograph the body and
 * the exposure are the recognisable part, the way a title and artist are for a recording.
 *
 * The values arrive already in EXIF's own display forms ("1/1600 s", "f/3.6", "23 mm"), so
 * there is nothing to format here.
 */
function pushExifRows(rows: MediaInfoRow[], exif: MediaExif | null): void {
  if (!exif) return;

  pushRow(rows, 'mediaInfo.camera', exif.camera);
  pushRow(rows, 'mediaInfo.lens', exif.lens);
  pushRow(rows, 'mediaInfo.takenAt', exif.takenAt);
  // Where and when the shot was taken belong together, ahead of how it was exposed.
  pushRow(rows, 'mediaInfo.location', formatCoordinates(exif.latitude, exif.longitude));
  pushRow(rows, 'mediaInfo.exposure', exif.exposureTime);
  pushRow(rows, 'mediaInfo.aperture', exif.fNumber);
  pushRow(rows, 'mediaInfo.iso', exif.iso);
  pushRow(rows, 'mediaInfo.focalLength', exif.focalLength);
}

/** What the file says about itself, ahead of what it is made of — it is what people recognise. */
function pushTagRows(rows: MediaInfoRow[], tags: MediaTags | null): void {
  if (!tags) return;

  pushRow(rows, 'mediaInfo.title', tags.title);
  pushRow(rows, 'mediaInfo.artist', tags.artist);
  pushRow(rows, 'mediaInfo.album', tags.album);
  // Only worth a row of its own when it differs from the track artist.
  pushRow(
    rows,
    'mediaInfo.albumArtist',
    tags.albumArtist && tags.albumArtist !== tags.artist ? tags.albumArtist : null,
  );
  pushRow(rows, 'mediaInfo.composer', tags.composer);
  pushRow(rows, 'mediaInfo.trackNumber', tags.trackNumber === null ? null : String(tags.trackNumber));
  pushRow(rows, 'mediaInfo.year', tags.year === null ? null : String(tags.year));
  pushRow(rows, 'mediaInfo.genre', tags.genre);
}

/**
 * The decoder that would handle this file, which is a fact about this machine and its installed
 * plugins rather than about the file. It belongs beside playback controls, not in a list of the
 * file's own properties, so it is kept out of `summarizeMediaInfo` and added by the player.
 */
export function describeDecoderRow(info: MediaInfo): MediaInfoRow | null {
  const decoder = info.video[0]?.decoder ?? info.audio[0]?.decoder ?? null;

  if (!decoder) return null;

  return {
    labelKey: 'mediaInfo.decoder',
    value: decoder,
  };
}

/**
 * Flattens whatever the file turned out to be into rows worth showing — everything here is a
 * property the file itself carries.
 *
 * Absent fields produce no row rather than a row reading "unknown": a panel of blanks tells the
 * reader less than a shorter panel does. Only the first stream of each kind is described, since
 * the second audio track of a film is a different question from "what is this file".
 */
export function summarizeMediaInfo(info: MediaInfo): MediaInfoRow[] {
  const rows: MediaInfoRow[] = [];

  if (info.image) {
    const { image } = info;
    pushExifRows(rows, image.exif);
    pushRow(rows, 'mediaInfo.resolution', formatResolution(image.width, image.height));
    pushRow(rows, 'mediaInfo.encoding', image.format);
    pushRow(rows, 'mediaInfo.color', image.color);
    pushRow(rows, 'mediaInfo.density', formatDpi(image.dpi));
    pushRow(rows, 'mediaInfo.encodedWith', image.exif?.software ?? null);
    return rows;
  }

  pushTagRows(rows, info.tags);

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

  // The software that wrote the file, last because it is provenance rather than content.
  pushRow(rows, 'mediaInfo.encodedWith', info.tags?.encoder ?? null);

  return rows;
}
