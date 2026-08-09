// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { describe, expect, it } from 'vitest';
import {
  describeDecoderRow,
  formatBitrate,
  formatChannels,
  formatCoordinates,
  formatDpi,
  formatFrameRate,
  formatResolution,
  formatSampleRate,
  summarizeMediaInfo,
  type MediaInfo,
  type MediaTags,
} from '@/utils/media-info';

function mediaInfo(overrides: Partial<MediaInfo> = {}): MediaInfo {
  return {
    container: null,
    durationMs: null,
    video: [],
    audio: [],
    image: null,
    tags: null,
    ...overrides,
  };
}

function tags(overrides: Partial<MediaTags> = {}): MediaTags {
  return {
    title: null,
    artist: null,
    album: null,
    albumArtist: null,
    composer: null,
    genre: null,
    trackNumber: null,
    year: null,
    encoder: null,
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

  /** The form that pastes straight into a map, with the sign carrying the hemisphere. */
  it('writes coordinates in decimal degrees', () => {
    expect(formatCoordinates(37.774917, -122.419417)).toBe('37.774917, -122.419417');
    // Half a position locates nothing.
    expect(formatCoordinates(37.774917, null)).toBeNull();
    expect(formatCoordinates(null, null)).toBeNull();
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
          exif: null,
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
     * The values here are what the backend actually returned for a real photograph, so this
     * pins the order and wording a photographer would read.
     */
    it('leads a photograph with the camera and the exposure', () => {
      const rows = summarizeMediaInfo(mediaInfo({
        image: {
          format: 'JPEG',
          width: 1200,
          height: 800,
          color: 'RGB 8-bit',
          dpi: null,
          exif: {
            camera: 'FUJIFILM X100F',
            // A fixed-lens body states no lens model, so that row must simply not appear.
            lens: null,
            takenAt: '2021-09-15 16:24:38',
            exposureTime: '1/1600 s',
            fNumber: 'f/3.6',
            iso: '200',
            focalLength: '23 mm',
            // Stripped from this file, as sharing platforms routinely do.
            latitude: null,
            longitude: null,
            software: 'Adobe Photoshop Lightroom Classic 11.3.1 (Macintosh)',
          },
        },
      }));

      expect(rows).toEqual([
        {
          labelKey: 'mediaInfo.camera',
          value: 'FUJIFILM X100F',
        },
        {
          labelKey: 'mediaInfo.takenAt',
          value: '2021-09-15 16:24:38',
        },
        {
          labelKey: 'mediaInfo.exposure',
          value: '1/1600 s',
        },
        {
          labelKey: 'mediaInfo.aperture',
          value: 'f/3.6',
        },
        {
          labelKey: 'mediaInfo.iso',
          value: '200',
        },
        {
          labelKey: 'mediaInfo.focalLength',
          value: '23 mm',
        },
        {
          labelKey: 'mediaInfo.resolution',
          value: '1200 × 800',
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
          labelKey: 'mediaInfo.encodedWith',
          value: 'Adobe Photoshop Lightroom Classic 11.3.1 (Macintosh)',
        },
      ]);
    });

    /** A file that kept its coordinates places them with the rest of the circumstances. */
    it('lists where the shot was taken, between when and how', () => {
      const rows = summarizeMediaInfo(mediaInfo({
        image: {
          format: 'JPEG',
          width: 1200,
          height: 800,
          color: 'RGB 8-bit',
          dpi: null,
          exif: {
            camera: 'FUJIFILM X100F',
            lens: null,
            takenAt: '2021-09-15 16:24:38',
            exposureTime: '1/1600 s',
            fNumber: null,
            iso: null,
            focalLength: null,
            latitude: 37.774917,
            longitude: -122.419417,
            software: null,
          },
        },
      }));

      expect(rows.map(row => row.labelKey)).toEqual([
        'mediaInfo.camera',
        'mediaInfo.takenAt',
        'mediaInfo.location',
        'mediaInfo.exposure',
        'mediaInfo.resolution',
        'mediaInfo.encoding',
        'mediaInfo.color',
      ]);
      expect(rows.find(row => row.labelKey === 'mediaInfo.location')?.value)
        .toBe('37.774917, -122.419417');
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
          exif: null,
        },
      }));

      expect(rows.map(row => row.labelKey)).not.toContain('mediaInfo.density');
      expect(rows).toHaveLength(3);
    });

    /**
     * The decoder depends on what this machine has installed, not on the file, so it has no
     * place in a list of the file's own properties. The player adds it separately.
     */
    it('leaves the decoder out of the file properties', () => {
      const rows = summarizeMediaInfo(mediaInfo({
        video: [{
          codec: 'H.264',
          width: 1920,
          height: 1080,
          frameRate: null,
          bitrateBps: null,
          decoder: 'VA-API H.264 Decoder in AMD Radeon 780M Graphics',
        }],
      }));

      expect(rows.map(row => row.labelKey)).not.toContain('mediaInfo.decoder');
    });

    it('leads with the tags the file carries, then what it is made of', () => {
      const rows = summarizeMediaInfo(mediaInfo({
        container: 'MPEG-1 Layer 3',
        tags: tags({
          title: 'Plastic Love',
          artist: 'Mariya Takeuchi',
          album: 'Variety',
          trackNumber: 3,
          year: 1984,
          encoder: 'Lavf60.16.100',
        }),
        audio: [{
          codec: 'MPEG-1 Layer 3 (MP3)',
          channels: 2,
          sampleRateHz: 44_100,
          bitrateBps: null,
          decoder: 'mpg123 mp3 decoder',
        }],
      }));

      expect(rows.map(row => row.labelKey)).toEqual([
        'mediaInfo.title',
        'mediaInfo.artist',
        'mediaInfo.album',
        'mediaInfo.trackNumber',
        'mediaInfo.year',
        'mediaInfo.container',
        'mediaInfo.encoding',
        'mediaInfo.channels',
        'mediaInfo.sampleRate',
        'mediaInfo.encodedWith',
      ]);
    });

    /** Repeating the track artist as the album artist is a row that says nothing. */
    it('names an album artist only when it differs from the artist', () => {
      const same = summarizeMediaInfo(mediaInfo({
        tags: tags({
          artist: 'Mariya Takeuchi',
          albumArtist: 'Mariya Takeuchi',
        }),
      }));
      expect(same.map(row => row.labelKey)).not.toContain('mediaInfo.albumArtist');

      const compilation = summarizeMediaInfo(mediaInfo({
        tags: tags({
          artist: 'Mariya Takeuchi',
          albumArtist: 'Various Artists',
        }),
      }));
      expect(compilation.map(row => row.labelKey)).toContain('mediaInfo.albumArtist');
    });
  });

  describe('the decoder, which belongs to the machine rather than the file', () => {
    it('describes the picture decoder for a video and the sound decoder for audio', () => {
      const video = describeDecoderRow(mediaInfo({
        video: [{
          codec: 'H.264',
          width: 1920,
          height: 1080,
          frameRate: null,
          bitrateBps: null,
          decoder: 'VA-API H.264 Decoder in AMD Radeon 780M Graphics',
        }],
        audio: [{
          codec: 'AAC',
          channels: 2,
          sampleRateHz: 48_000,
          bitrateBps: null,
          decoder: 'libav AAC decoder',
        }],
      }));

      expect(video).toEqual({
        labelKey: 'mediaInfo.decoder',
        value: 'VA-API H.264 Decoder in AMD Radeon 780M Graphics',
      });

      const audioOnly = describeDecoderRow(mediaInfo({
        audio: [{
          codec: 'AAC',
          channels: 2,
          sampleRateHz: 48_000,
          bitrateBps: null,
          decoder: 'libav AAC decoder',
        }],
      }));

      expect(audioOnly?.value).toBe('libav AAC decoder');
    });

    /** No GStreamer, or a codec nothing installed can take, means no decoder to name. */
    it('has nothing to say when no decoder was named', () => {
      expect(describeDecoderRow(mediaInfo())).toBeNull();
    });

    it('has nothing to say about a file it could not read', () => {
      expect(summarizeMediaInfo(mediaInfo())).toEqual([]);
    });
  });
});
