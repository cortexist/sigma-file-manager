// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';

import {
  copyCurrentVideoFrameToClipboard,
  encodeCurrentVideoFrameToPng,
} from '@/utils/video-frame-capture';

const invokeMock = vi.fn();
const isLinux = { value: false };

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock('@/utils/platform-info', () => ({
  ensurePlatformInfo: () => Promise.resolve({ isLinux: isLinux.value }),
}));

const PNG_BYTES = new Uint8Array([0x89, 0x50, 0x4E, 0x47, 1, 2, 3]);
const VIDEO_PATH = '/home/user/Videos/clip.mp4';

let drawImage: ReturnType<typeof vi.fn>;
let toBlob: ReturnType<typeof vi.fn>;

/** jsdom ships no 2D backend, so both ends of the canvas round trip are stubbed. */
function stubCanvas(blob: Blob | null = new Blob([PNG_BYTES], { type: 'image/png' })) {
  drawImage = vi.fn();
  toBlob = vi.fn((callback: BlobCallback) => {
    callback(blob);
  });

  HTMLCanvasElement.prototype.getContext = vi.fn(
    () => ({ drawImage }),
  ) as unknown as HTMLCanvasElement['getContext'];
  HTMLCanvasElement.prototype.toBlob = toBlob as unknown as HTMLCanvasElement['toBlob'];
}

function videoWithFrame(width = 1920, height = 1080, currentTime = 0) {
  const video = document.createElement('video');
  Object.defineProperty(video, 'videoWidth', {
    configurable: true,
    value: width,
  });
  Object.defineProperty(video, 'videoHeight', {
    configurable: true,
    value: height,
  });
  Object.defineProperty(video, 'currentTime', {
    configurable: true,
    value: currentTime,
  });

  return video;
}

describe('video frame capture', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
    isLinux.value = false;
    stubCanvas();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('reading the frame out of the element', () => {
    it('captures at the file dimensions rather than the size the element is drawn at', async () => {
      const video = videoWithFrame(1920, 1080);

      const bytes = await encodeCurrentVideoFrameToPng(video);

      expect(drawImage).toHaveBeenCalledWith(video, 0, 0, 1920, 1080);
      expect(toBlob).toHaveBeenCalledWith(expect.any(Function), 'image/png');
      expect(Array.from(bytes)).toEqual(Array.from(PNG_BYTES));
    });

    /** Before the first frame is decoded there is genuinely nothing on screen to copy. */
    it('refuses a video that has not decoded a frame yet', async () => {
      await expect(encodeCurrentVideoFrameToPng(videoWithFrame(0, 0))).rejects.toThrow();
      expect(drawImage).not.toHaveBeenCalled();
    });

    it('reports a failed encode rather than putting nothing on the clipboard', async () => {
      stubCanvas(null);

      await expect(copyCurrentVideoFrameToClipboard(videoWithFrame(), VIDEO_PATH))
        .rejects.toThrow();
      expect(invokeMock).not.toHaveBeenCalled();
    });

    it('hands the PNG to the clipboard command as bytes', async () => {
      await copyCurrentVideoFrameToClipboard(videoWithFrame(), VIDEO_PATH);

      expect(invokeMock).toHaveBeenCalledWith('set_system_clipboard_image_from_png_bytes', {
        pngBytes: Array.from(PNG_BYTES),
      });
    });
  });

  /**
   * WebKitGTK keeps decoded video in a GPU buffer JavaScript only ever samples as black, so
   * the element is asked for nothing but the playback position and the file is decoded again
   * natively. Drawing to a canvas there would silently copy a black rectangle.
   */
  describe('on Linux', () => {
    beforeEach(() => {
      isLinux.value = true;
    });

    it('decodes the frame natively at the position the player is sitting on', async () => {
      await copyCurrentVideoFrameToClipboard(videoWithFrame(1920, 1080, 42.5), VIDEO_PATH);

      expect(invokeMock).toHaveBeenCalledWith('copy_video_frame_to_system_clipboard', {
        path: VIDEO_PATH,
        positionSeconds: 42.5,
      });
    });

    it('never reads the frame off a canvas', async () => {
      await copyCurrentVideoFrameToClipboard(videoWithFrame(), VIDEO_PATH);

      expect(drawImage).not.toHaveBeenCalled();
      expect(toBlob).not.toHaveBeenCalled();
    });
  });
});
