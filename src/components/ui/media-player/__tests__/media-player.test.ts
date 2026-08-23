// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { flushPromises, mount, type VueWrapper } from '@vue/test-utils';
import {
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import MediaPlayer from '../media-player.vue';

vi.mock('vue-i18n', () => ({
  useI18n: () => ({
    t: (key: string) => key,
  }),
}));

const copyCurrentVideoFrameToClipboard = vi.fn();

vi.mock('@/utils/video-frame-capture', () => ({
  copyCurrentVideoFrameToClipboard: (...args: unknown[]) =>
    copyCurrentVideoFrameToClipboard(...args),
}));

const readMediaInfo = vi.fn();

// Only the backend call is stubbed; the real summarizer still formats what comes back, so the
// rows asserted below are the ones a user would read.
vi.mock('@/utils/media-info', async importOriginal => ({
  ...await importOriginal<typeof import('@/utils/media-info')>(),
  readMediaInfo: (...args: unknown[]) => readMediaInfo(...args),
}));

const CAPTURE_PATH = '/home/user/Videos/first.mp4';

const FIRST_SRC = 'media://first.mp4';
const SECOND_SRC = 'media://second.mp4';

/** Several controls share the button class, and fullscreen — not play — comes first. */
const PLAY_BUTTON = '[aria-label="mediaPlayer.play"]';

let play: ReturnType<typeof vi.fn>;
let load: ReturnType<typeof vi.fn>;

function mountPlayer(props: Record<string, unknown> = {}) {
  return mount(MediaPlayer, {
    props: {
      src: FIRST_SRC,
      kind: 'video',
      ...props,
    },
    global: {
      stubs: { Slider: true },
    },
  });
}

/** jsdom leaves the playback methods unimplemented, and `duration` is always NaN. */
function stubMediaElement(wrapper: VueWrapper, duration = 120) {
  const media = wrapper.get('video').element as HTMLVideoElement;
  Object.defineProperty(media, 'duration', {
    configurable: true,
    value: duration,
  });

  return media;
}

async function loadMetadata(wrapper: VueWrapper) {
  stubMediaElement(wrapper);
  await wrapper.get('video').trigger('loadedmetadata');
}

/** jsdom never really plays, so the finished state has to be described to the element. */
function stubFinished(wrapper: VueWrapper, duration = 120) {
  const media = stubMediaElement(wrapper, duration);
  let time = duration;

  Object.defineProperty(media, 'currentTime', {
    configurable: true,
    get: () => time,
    set: (next: number) => {
      time = next;
    },
  });
  Object.defineProperty(media, 'ended', {
    configurable: true,
    get: () => time >= duration,
  });
  Object.defineProperty(media, 'paused', {
    configurable: true,
    value: true,
  });

  return media;
}

function exposedPlayer(wrapper: VueWrapper) {
  return wrapper.vm as unknown as { restart: () => void };
}

describe('MediaPlayer', () => {
  beforeEach(() => {
    play = vi.fn(() => Promise.resolve());
    load = vi.fn();
    HTMLMediaElement.prototype.play = play as unknown as HTMLMediaElement['play'];
    HTMLMediaElement.prototype.pause = vi.fn() as unknown as HTMLMediaElement['pause'];
    HTMLMediaElement.prototype.load = load as unknown as HTMLMediaElement['load'];
    copyCurrentVideoFrameToClipboard.mockReset();
    copyCurrentVideoFrameToClipboard.mockResolvedValue(undefined);
    readMediaInfo.mockReset();
    readMediaInfo.mockResolvedValue({
      container: null,
      durationMs: null,
      video: [],
      audio: [],
      image: null,
    });
  });

  /**
   * The regression this guards: callers used to key the player on the file path, so moving to
   * the next file built a new element. That element is the one `requestFullscreen` was called
   * on, so replacing it dropped the window out of fullscreen on every next-file. Keeping the
   * same node across sources is what makes fullscreen survive browsing.
   */
  it('reuses the same DOM nodes when the source changes', async () => {
    const wrapper = mountPlayer();
    const rootBefore = wrapper.element;
    const mediaBefore = wrapper.get('video').element;

    await wrapper.setProps({ src: SECOND_SRC });

    expect(wrapper.element).toBe(rootBefore);
    expect(wrapper.get('video').element).toBe(mediaBefore);
    expect(wrapper.get('video').attributes('src')).toBe(SECOND_SRC);
  });

  it('resets everything derived from the previous file', async () => {
    const wrapper = mountPlayer();
    await loadMetadata(wrapper);
    await wrapper.get('video').trigger('timeupdate');

    await wrapper.setProps({ src: SECOND_SRC });

    // 0:00 of 0:00 until the new file reports its own metadata.
    expect(wrapper.get('.media-player__time').text()).toContain('0:00');
  });

  /**
   * The regression this guards: unmounting used to only pause, and a paused element keeps
   * its decode pipeline — audio server stream included — until garbage collection reaps the
   * detached node, which it may never do. Every played file left another corked stream on
   * the server. Detaching the source and reloading is what makes the engine tear the
   * pipeline down at unmount rather than at some later collection.
   */
  it('releases the media source on unmount', () => {
    const wrapper = mountPlayer();
    const media = wrapper.get('video').element;
    load.mockClear();

    wrapper.unmount();

    expect(media.pause).toHaveBeenCalled();
    expect(media.getAttribute('src')).toBeNull();
    expect(load).toHaveBeenCalled();
  });

  describe('autoplay', () => {
    /**
     * The `autoplay` attribute only applies to an element's first load — the spec clears the
     * can-autoplay flag once it has played or been paused. Since the instance is now reused
     * across files, each new source has to be started explicitly or the second file onwards
     * would sit paused.
     */
    it('starts playback for each new source, not just the first', async () => {
      const wrapper = mountPlayer({ autoplay: true });

      await loadMetadata(wrapper);
      expect(play).toHaveBeenCalledTimes(1);

      await wrapper.setProps({ src: SECOND_SRC });
      await loadMetadata(wrapper);

      expect(play).toHaveBeenCalledTimes(2);
    });

    /**
     * The elements used to carry the `autoplay` attribute on top of the explicit start above,
     * which duplicated it on exactly one load — an element's first — so the first file opened
     * in a process took a different path from every file after it. One start per load.
     */
    it('leaves the elements carrying no autoplay attribute of their own', async () => {
      const video = mountPlayer({ autoplay: true });
      await loadMetadata(video);

      expect(video.get('video').attributes('autoplay')).toBeUndefined();
      expect(play).toHaveBeenCalledTimes(1);

      const audio = mountPlayer({
        autoplay: true,
        kind: 'audio',
      });
      expect(audio.get('audio').attributes('autoplay')).toBeUndefined();
    });

    it('does not start playback when autoplay is off', async () => {
      const wrapper = mountPlayer({ autoplay: false });

      await loadMetadata(wrapper);
      await wrapper.setProps({ src: SECOND_SRC });
      await loadMetadata(wrapper);

      expect(play).not.toHaveBeenCalled();
    });

    it('plays the already-open file when the setting is switched on', async () => {
      const wrapper = mountPlayer({ autoplay: false });
      await loadMetadata(wrapper);
      expect(play).not.toHaveBeenCalled();

      const media = wrapper.get('video').element as HTMLVideoElement;
      Object.defineProperty(media, 'paused', {
        configurable: true,
        value: true,
      });

      await wrapper.setProps({ autoplay: true });

      expect(play).toHaveBeenCalledTimes(1);
    });

    it('re-applies the mute setting on each new file', async () => {
      // With one instance spanning the folder, a manual unmute must not leak into the next
      // file — the remount used to re-read the setting for free.
      const wrapper = mountPlayer({ muted: true });
      await loadMetadata(wrapper);

      const [muteButton] = wrapper.findAll('.media-player__controls button')
        .filter(button => button.attributes('aria-label') === 'mediaPlayer.unmute');
      await muteButton.trigger('click');
      expect(wrapper.get('video').element.muted).toBe(false);

      await wrapper.setProps({ src: SECOND_SRC });
      await loadMetadata(wrapper);

      expect(wrapper.get('video').element.muted).toBe(true);
    });

    it('survives a rejected autoplay without surfacing an error', async () => {
      play.mockRejectedValue(new Error('blocked'));
      const wrapper = mountPlayer({ autoplay: true });

      await loadMetadata(wrapper);
      await Promise.resolve();

      expect(wrapper.find('.media-player__error').exists()).toBe(false);
    });
  });

  describe('frame capture', () => {
    it('is not offered unless the caller supplies the file behind the source', async () => {
      const wrapper = mountPlayer();
      await loadMetadata(wrapper);

      expect(wrapper.find('.media-player__capture').exists()).toBe(false);
    });

    // Only a stopped video has a frame anyone is looking at.
    it('shows the button while stopped and hides it during playback', async () => {
      const wrapper = mountPlayer({
        sourcePath: CAPTURE_PATH,
        allowFrameCapture: true,
      });
      await loadMetadata(wrapper);
      expect(wrapper.find('.media-player__capture').exists()).toBe(true);

      await wrapper.get('video').trigger('play');
      expect(wrapper.find('.media-player__capture').exists()).toBe(false);

      await wrapper.get('video').trigger('pause');
      expect(wrapper.find('.media-player__capture').exists()).toBe(true);
    });

    it('copies the frame on screen and reports back on the button', async () => {
      const wrapper = mountPlayer({
        sourcePath: CAPTURE_PATH,
        allowFrameCapture: true,
      });
      await loadMetadata(wrapper);

      await wrapper.get('.media-player__capture').trigger('click');
      await flushPromises();

      expect(copyCurrentVideoFrameToClipboard)
        .toHaveBeenCalledWith(wrapper.get('video').element, CAPTURE_PATH);
      expect(wrapper.get('.media-player__capture').attributes('title'))
        .toBe('mediaPlayer.frameCopied');
    });

    it('reports a failed copy instead of claiming the frame was captured', async () => {
      vi.spyOn(console, 'error').mockImplementation(() => {});
      copyCurrentVideoFrameToClipboard.mockRejectedValue(new Error('no frame'));
      const wrapper = mountPlayer({
        sourcePath: CAPTURE_PATH,
        allowFrameCapture: true,
      });
      await loadMetadata(wrapper);

      await wrapper.get('.media-player__capture').trigger('click');
      await flushPromises();

      expect(wrapper.get('.media-player__capture').attributes('title'))
        .toBe('mediaPlayer.frameCaptureFailed');
    });

    it('drops the previous outcome when the next file opens', async () => {
      const wrapper = mountPlayer({
        sourcePath: CAPTURE_PATH,
        allowFrameCapture: true,
      });
      await loadMetadata(wrapper);
      await wrapper.get('.media-player__capture').trigger('click');
      await flushPromises();

      await wrapper.setProps({ src: SECOND_SRC });

      expect(wrapper.get('.media-player__capture').attributes('title'))
        .toBe('mediaPlayer.captureFrame');
    });
  });

  describe('media details', () => {
    const INFO_TOGGLE = '.media-player__info-toggle';

    const H264 = {
      container: null,
      durationMs: 47_563,
      video: [{
        codec: 'H.264 (Main Profile)',
        width: 1920,
        height: 1080,
        frameRate: 30,
        bitrateBps: null,
        decoder: 'VA-API H.264 Decoder in AMD Radeon 780M Graphics',
      }],
      audio: [],
      image: null,
    };

    /** Unlike frame capture, this is offered for audio too — anything with a file behind it. */
    it('is offered wherever a file backs the source, and not otherwise', async () => {
      const withoutPath = mountPlayer();
      await loadMetadata(withoutPath);
      expect(withoutPath.find(INFO_TOGGLE).exists()).toBe(false);

      const audio = mountPlayer({
        kind: 'audio',
        sourcePath: CAPTURE_PATH,
      });
      expect(audio.find(INFO_TOGGLE).exists()).toBe(true);
    });

    it('reads nothing until asked, then lists what the file is made of', async () => {
      readMediaInfo.mockResolvedValue(H264);
      const wrapper = mountPlayer({ sourcePath: CAPTURE_PATH });
      await loadMetadata(wrapper);

      // Every preview in a folder listing mounts one of these; none should read a file
      // nobody has asked about.
      expect(readMediaInfo).not.toHaveBeenCalled();
      expect(wrapper.find('.media-player__info').exists()).toBe(false);

      await wrapper.get(INFO_TOGGLE).trigger('click');
      await flushPromises();

      expect(readMediaInfo).toHaveBeenCalledWith(CAPTURE_PATH);
      const panel = wrapper.get('.media-player__info').text();
      expect(panel).toContain('1920 × 1080');
      expect(panel).toContain('H.264 (Main Profile)');
      expect(panel).toContain('30 fps');
      // Which GPU — or the CPU — the file would land on, and last so it can wrap freely.
      expect(panel).toContain('VA-API H.264 Decoder in AMD Radeon 780M Graphics');
      expect(panel.trimEnd().endsWith('AMD Radeon 780M Graphics')).toBe(true);
    });

    it('says so rather than sitting blank when the file cannot be read', async () => {
      readMediaInfo.mockRejectedValue(new Error('unsupported'));
      vi.spyOn(console, 'error').mockImplementation(() => {});
      const wrapper = mountPlayer({ sourcePath: CAPTURE_PATH });
      await loadMetadata(wrapper);

      await wrapper.get(INFO_TOGGLE).trigger('click');
      await flushPromises();

      expect(wrapper.get('.media-player__info').text()).toContain('mediaInfo.unavailable');
    });

    /** Browsing with the panel open has to refill it, not leave the previous file's numbers. */
    it('re-reads when the source changes while open', async () => {
      readMediaInfo.mockResolvedValue(H264);
      const wrapper = mountPlayer({ sourcePath: CAPTURE_PATH });
      await loadMetadata(wrapper);
      await wrapper.get(INFO_TOGGLE).trigger('click');
      await flushPromises();

      await wrapper.setProps({
        src: SECOND_SRC,
        sourcePath: '/home/user/Videos/second.mp4',
      });
      await flushPromises();

      expect(readMediaInfo).toHaveBeenCalledTimes(2);
      expect(readMediaInfo).toHaveBeenLastCalledWith('/home/user/Videos/second.mp4');
    });
  });

  describe('stalled playback', () => {
    /** An element claiming to play with its clock frozen at `time`. */
    function stubStalled(wrapper: VueWrapper, time = 0) {
      const media = stubMediaElement(wrapper);
      let clock = time;

      Object.defineProperty(media, 'paused', {
        configurable: true,
        value: false,
      });
      Object.defineProperty(media, 'ended', {
        configurable: true,
        value: false,
      });
      Object.defineProperty(media, 'currentTime', {
        configurable: true,
        get: () => clock,
        set: (next: number) => {
          clock = next;
        },
      });

      return media;
    }

    /**
     * The regression this guards: WebKitGTK can leave the element reporting `paused === false`
     * while the clock never advances — the first quick view of a cold-launched session does it
     * every time, and loads have done it intermittently. Pause/play recovers nothing; only a
     * pipeline rebuild does, which is what closing and reopening the viewer performs by hand.
     */
    it('rebuilds the pipeline when the clock freezes while claiming to play', async () => {
      vi.useFakeTimers();

      try {
        const wrapper = mountPlayer();
        stubStalled(wrapper);
        await wrapper.get('video').trigger('loadedmetadata');
        await wrapper.get('video').trigger('play');

        // Frozen, but not yet past the grace period.
        await vi.advanceTimersByTimeAsync(2500);
        expect(load).not.toHaveBeenCalled();

        await vi.advanceTimersByTimeAsync(1500);
        expect(load).toHaveBeenCalledTimes(1);

        // The rebuilt pipeline reports metadata; playback is asked to resume.
        play.mockClear();
        await wrapper.get('video').trigger('loadedmetadata');
        expect(play).toHaveBeenCalledTimes(1);
      }
      finally {
        vi.useRealTimers();
      }
    });

    it('resumes where the clock stood rather than from the start', async () => {
      vi.useFakeTimers();

      try {
        const wrapper = mountPlayer();
        const media = stubStalled(wrapper, 12.5);
        await wrapper.get('video').trigger('loadedmetadata');
        await wrapper.get('video').trigger('play');

        await vi.advanceTimersByTimeAsync(4000);
        expect(load).toHaveBeenCalledTimes(1);

        media.currentTime = 0;
        await wrapper.get('video').trigger('loadedmetadata');
        expect(media.currentTime).toBe(12.5);
      }
      finally {
        vi.useRealTimers();
      }
    });

    it('leaves a healthy pipeline alone', async () => {
      vi.useFakeTimers();

      try {
        const wrapper = mountPlayer();
        const media = stubStalled(wrapper);
        await wrapper.get('video').trigger('loadedmetadata');
        await wrapper.get('video').trigger('play');

        // The clock advances a little on every watchdog reading.
        for (let tick = 0; tick < 20; tick++) {
          media.currentTime += 0.25;
          await vi.advanceTimersByTimeAsync(500);
        }

        expect(load).not.toHaveBeenCalled();
      }
      finally {
        vi.useRealTimers();
      }
    });

    /** A file nothing can play must settle, not rebuild in a loop forever. */
    it('gives up after the recovery cap', async () => {
      vi.useFakeTimers();

      try {
        const wrapper = mountPlayer();
        stubStalled(wrapper);
        await wrapper.get('video').trigger('loadedmetadata');
        await wrapper.get('video').trigger('play');

        await vi.advanceTimersByTimeAsync(60_000);

        expect(load).toHaveBeenCalledTimes(2);
      }
      finally {
        vi.useRealTimers();
      }
    });
  });

  describe('replaying a file that has finished', () => {
    /**
     * The regression this guards: `play()` on an ended element carries an implicit rewind by
     * spec, and WebKitGTK takes that seek while dropping the play. The clock snapped back to
     * 0:00 and the button flipped to pause over a frame that never moved, so replaying took a
     * pause-then-play. Rewinding by hand and waiting for the seek keeps it to one click.
     */
    it('rewinds first and starts playing only once the seek lands', async () => {
      const wrapper = mountPlayer();
      const media = stubFinished(wrapper);

      await wrapper.get('video').trigger('loadedmetadata');
      await wrapper.get('video').trigger('ended');

      await wrapper.get(PLAY_BUTTON).trigger('click');

      expect(media.currentTime).toBe(0);
      expect(play).not.toHaveBeenCalled();

      media.dispatchEvent(new Event('seeked'));
      await flushPromises();

      expect(play).toHaveBeenCalledTimes(1);
    });

    /** Mid-file playback must not pay the rewind's cost — pressing play resumes where it is. */
    it('resumes in place when the file has not finished', async () => {
      const wrapper = mountPlayer();
      const media = stubFinished(wrapper);
      media.currentTime = 30;

      await wrapper.get('video').trigger('loadedmetadata');
      await wrapper.get(PLAY_BUTTON).trigger('click');
      await flushPromises();

      expect(media.currentTime).toBe(30);
      expect(play).toHaveBeenCalledTimes(1);
    });
  });

  /**
   * Quick View keeps one player across files, and only a changed `src` resets it. When the
   * file it already holds is opened again — the main window reopening a file whose background
   * session played itself out — the path does not change, so the owner asks for the fresh
   * start directly. Without it the reopened file sat paused while any other file autoplayed.
   */
  describe('restarting on the owner\'s request', () => {
    it('rewinds a finished file and plays it once the seek lands', async () => {
      const wrapper = mountPlayer({ autoplay: true });
      const media = stubFinished(wrapper);

      await wrapper.get('video').trigger('loadedmetadata');
      await wrapper.get('video').trigger('ended');
      play.mockClear();

      exposedPlayer(wrapper).restart();

      expect(media.currentTime).toBe(0);
      expect(play).not.toHaveBeenCalled();

      media.dispatchEvent(new Event('seeked'));
      await flushPromises();

      expect(play).toHaveBeenCalledTimes(1);
    });

    it('rewinds a file stopped midway rather than resuming it', async () => {
      const wrapper = mountPlayer({ autoplay: true });
      const media = stubFinished(wrapper);
      media.currentTime = 30;

      await wrapper.get('video').trigger('loadedmetadata');
      play.mockClear();

      exposedPlayer(wrapper).restart();
      media.dispatchEvent(new Event('seeked'));
      await flushPromises();

      expect(media.currentTime).toBe(0);
      expect(play).toHaveBeenCalledTimes(1);
    });

    it('only rewinds when autoplay is off', async () => {
      const wrapper = mountPlayer();
      const media = stubFinished(wrapper);
      media.currentTime = 30;

      await wrapper.get('video').trigger('loadedmetadata');

      exposedPlayer(wrapper).restart();
      media.dispatchEvent(new Event('seeked'));
      await flushPromises();

      expect(media.currentTime).toBe(0);
      expect(play).not.toHaveBeenCalled();
    });
  });

  /**
   * Quick View closes by dropping the file, which unmounts this component while the window
   * itself is only hidden. Leaving the element to be paused by the engine let a video go on
   * playing — audible, with nothing on screen to stop it.
   */
  it('stops playback when it is unmounted', async () => {
    const wrapper = mountPlayer({ autoplay: true });
    await loadMetadata(wrapper);

    const media = wrapper.get('video').element as HTMLVideoElement;
    const pause = media.pause as unknown as ReturnType<typeof vi.fn>;

    expect(pause).not.toHaveBeenCalled();

    wrapper.unmount();

    expect(pause).toHaveBeenCalledTimes(1);
  });
});
