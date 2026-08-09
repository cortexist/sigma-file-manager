// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { describe, expect, it } from 'vitest';
import {
  formatBitrate,
  formatChannels,
  formatDpi,
  formatFrameRate,
  formatResolution,
  formatSampleRate,
  summarizeMediaInfo,
  type MediaInfo,
} from '@/utils/media-info';

function mediaInfo(overrides: Partial<MediaInfo> = {}): MediaInfo {
  return {
    container: null,
    durationMs: null,
    video: [],
    audio: [],
    image: null,
    ...overrides,
  };
}

describe('media info formatting', () => {
  it('quotes bitrates the way the rest of the world writes them', () => {
    expect(formatBitrate(192_100)).toBe('192 kbps');
    expect(formatBitrate(18_266_440)).toBe('18.3 Mbps');
    // The backend sends absent rather than zero, but a zero must not become "0 kbps".
    expect(formatBitrate(0)).toBeNull();
    expect(formatBitrate(null)).toBeNull();
  });

  it('keeps the decimals an NTSC rate needs and none that it does not', () => {
    expect(formatFrameRate(23.976023976)).toBe('23.976 fps');
    expect(formatFrameRate(30)).toBe('30 fps');
    expect(formatFrameRate(null)).toBeNull();
  });

  it('formats the remaining scalars', () => {
    expect(formatResolution(1920, 1080)).toBe('1920 × 1080');
    expect(formatResolution(0, 1080)).toBeNull();
    expect(formatSampleRate(48_000)).toBe('48.0 kHz');
    expect(formatChannels(1)).toBe('Mono');
    expect(formatChannels(2)).toBe('Stereo');
    expect(formatChannels(6)).toBe('6 channels');
    expect(formatDpi(299.72)).toBe('300 DPI');
    expect(formatDpi(null)).toBeNull();
  });

  describe('summarizing a file', () => {
    it('describes a video by its picture first and its sound second', () => {
      const rows = summarizeMediaInfo(mediaInfo({
        container: 'Quicktime',
        durationMs: 47_563,
        video: [{
          codec: 'H.264 (Main Profile)',
          width: 1920,
          height: 1080,
          frameRate: 30,
          bitrateBps: 18_266_440,
          decoder: 'VA-API H.264 Decoder in AMD Radeon 780M Graphics',
        }],
        audio: [{
          codec: 'MPEG-4 AAC',
          channels: 2,
          sampleRateHz: 48_000,
          bitrateBps: 192_100,
          decoder: 'libav AAC (Advanced Audio Coding) decoder',
        }],
      }));

      expect(rows).toEqual([
        {
          labelKey: 'mediaInfo.resolution',
          value: '1920 × 1080',
        },
        {
          labelKey: 'mediaInfo.encoding',
          value: 'H.264 (Main Profile)',
        },
        {
          labelKey: 'mediaInfo.frameRate',
          value: '30 fps',
        },
        {
          labelKey: 'mediaInfo.bitrate',
          value: '18.3 Mbps',
        },
        {
          labelKey: 'mediaInfo.audioEncoding',
          value: 'MPEG-4 AAC',
        },
        {
          labelKey: 'mediaInfo.channels',
          value: 'Stereo',
        },
        {
          labelKey: 'mediaInfo.sampleRate',
          value: '48.0 kHz',
        },
        // The picture's decoder, not the sound's, and last because it is the one that wraps.
        {
          labelKey: 'mediaInfo.decoder',
          value: 'VA-API H.264 Decoder in AMD Radeon 780M Graphics',
        },
      ]);
    });

    /** With no picture to describe, the sound is the file and takes the plain labels. */
    it('describes audio as the subject when there is no video', () => {
      const rows = summarizeMediaInfo(mediaInfo({
        container: 'MPEG-4',
        audio: [{
          codec: 'MPEG-4 AAC',
          channels: 2,
          sampleRateHz: 44_100,
          bitrateBps: 256_000,
          decoder: 'libav AAC (Advanced Audio Coding) decoder',
        }],
      }));

      expect(rows).toEqual([
        {
          labelKey: 'mediaInfo.container',
          value: 'MPEG-4',
        },
        {
          labelKey: 'mediaInfo.encoding',
          value: 'MPEG-4 AAC',
        },
        {
          labelKey: 'mediaInfo.channels',
          value: 'Stereo',
        },
        {
          labelKey: 'mediaInfo.sampleRate',
          value: '44.1 kHz',
        },
        {
          labelKey: 'mediaInfo.bitrate',
          value: '256 kbps',
        },
        {
          labelKey: 'mediaInfo.decoder',
          value: 'libav AAC (Advanced Audio Coding) decoder',
        },
      ]);
    });

    it('describes a still by its size, encoding and density', () => {
      const rows = summarizeMediaInfo(mediaInfo({
        image: {
          format: 'JPEG',
          width: 2048,
          height: 1485,
          color: 'RGB 8-bit',
          dpi: 72,
        },
      }));

      expect(rows).toEqual([
        {
          labelKey: 'mediaInfo.resolution',
          value: '2048 × 1485',
        },
        {
          labelKey: 'mediaInfo.encoding',
          value: 'JPEG',
        },
        {
          labelKey: 'mediaInfo.color',
          value: 'RGB 8-bit',
        },
        {
          labelKey: 'mediaInfo.density',
          value: '72 DPI',
        },
      ]);
    });

    /**
     * A screenshot states no density, and most files leave something out. A row reading
     * "unknown" tells the reader less than no row at all.
     */
    it('omits what the file does not state instead of inventing it', () => {
      const rows = summarizeMediaInfo(mediaInfo({
        image: {
          format: 'PNG',
          width: 2762,
          height: 1578,
          color: 'RGBA 8-bit',
          dpi: null,
        },
      }));

      expect(rows.map(row => row.labelKey)).not.toContain('mediaInfo.density');
      expect(rows).toHaveLength(3);
    });

    /** No GStreamer, or a codec nothing installed can take, means no decoder to name. */
    it('says nothing about a decoder the backend could not name', () => {
      const rows = summarizeMediaInfo(mediaInfo({
        video: [{
          codec: 'H.264',
          width: 1920,
          height: 1080,
          frameRate: null,
          bitrateBps: null,
          decoder: null,
        }],
      }));

      expect(rows.map(row => row.labelKey)).not.toContain('mediaInfo.decoder');
    });

    it('has nothing to say about a file it could not read', () => {
      expect(summarizeMediaInfo(mediaInfo())).toEqual([]);
    });
  });
});
