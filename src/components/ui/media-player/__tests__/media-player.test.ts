// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { mount, type VueWrapper } from '@vue/test-utils';
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

const FIRST_SRC = 'media://first.mp4';
const SECOND_SRC = 'media://second.mp4';

let play: ReturnType<typeof vi.fn>;

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

describe('MediaPlayer', () => {
  beforeEach(() => {
    play = vi.fn(() => Promise.resolve());
    HTMLMediaElement.prototype.play = play as unknown as HTMLMediaElement['play'];
    HTMLMediaElement.prototype.pause = vi.fn() as unknown as HTMLMediaElement['pause'];
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
});
