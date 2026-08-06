// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { nextTick } from 'vue';
import { mount, type VueWrapper } from '@vue/test-utils';
import {
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import ImageViewer from '../image-viewer.vue';

vi.mock('vue-i18n', () => ({
  useI18n: () => ({
    t: (key: string) => key,
  }),
}));

const ORIGINAL_SRC = 'asset://photo.png';
const PREVIEW_SRC = 'asset://thumb.png';
const PLACEHOLDER_SRC = 'data:placeholder';

const CONTAINER_WIDTH = 800;
const CONTAINER_HEIGHT = 600;

function mountViewer(props: Record<string, unknown> = {}) {
  const wrapper = mount(ImageViewer, {
    props: {
      src: ORIGINAL_SRC,
      ...props,
    },
    attachTo: document.body,
  });

  // jsdom reports every element as 0x0, and the viewer treats a zero-sized container as
  // "not measurable yet" and skips pan clamping. Give it a real box.
  const container = wrapper.element as HTMLElement;
  Object.defineProperty(container, 'clientWidth', {
    configurable: true,
    value: CONTAINER_WIDTH,
  });
  Object.defineProperty(container, 'clientHeight', {
    configurable: true,
    value: CONTAINER_HEIGHT,
  });
  container.getBoundingClientRect = () => ({
    x: 0,
    y: 0,
    top: 0,
    left: 0,
    right: CONTAINER_WIDTH,
    bottom: CONTAINER_HEIGHT,
    width: CONTAINER_WIDTH,
    height: CONTAINER_HEIGHT,
    toJSON: () => ({}),
  });

  return wrapper;
}

/**
 * `wrapper.trigger` builds a typed DOM event and then assigns the given properties onto it,
 * which throws for the read-only ones this component reads (`clientX`, `button`, `deltaY`).
 * Dispatching a plain cancelable Event with those attached as own properties sidesteps that
 * and still exercises the real listener. The event is returned so `defaultPrevented` can be
 * asserted directly rather than through a stubbed `preventDefault`.
 */
async function dispatch(
  element: Element,
  type: string,
  init: Record<string, unknown> = {},
): Promise<Event> {
  const event = new Event(type, {
    bubbles: true,
    cancelable: true,
  });
  Object.assign(event, init);
  element.dispatchEvent(event);
  await nextTick();

  return event;
}

/** The visible layer. The off-screen decoder shares the `img` tag but is aria-hidden. */
function visibleImage(wrapper: VueWrapper) {
  return wrapper.get('.image-viewer__image');
}

function transformOf(wrapper: VueWrapper): {
  x: number;
  y: number;
} {
  const style = visibleImage(wrapper).attributes('style') ?? '';
  const [, x, y] = /translate\((-?[\d.]+)px,\s*(-?[\d.]+)px\)/.exec(style) ?? [];

  return {
    x: Number.parseFloat(x ?? '0'),
    y: Number.parseFloat(y ?? '0'),
  };
}

function zoomLabel(wrapper: VueWrapper): string {
  return wrapper.get('.image-viewer__zoom-level').text();
}

/** Reports the natural size the viewer reads off a loaded `img`. */
async function completeLoad(target: ReturnType<typeof visibleImage>, width = 1000, height = 500) {
  Object.defineProperty(target.element, 'naturalWidth', {
    configurable: true,
    value: width,
  });
  Object.defineProperty(target.element, 'naturalHeight', {
    configurable: true,
    value: height,
  });
  await target.trigger('load');
}

describe('ImageViewer', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
  });

  describe('progressive loading', () => {
    it('shows the thumbnail first and swaps to the original once it decodes', async () => {
      const wrapper = mountViewer({ previewSrc: PREVIEW_SRC });

      expect(visibleImage(wrapper).attributes('src')).toBe(PREVIEW_SRC);

      // The off-screen decoder holds the original and drives the swap.
      const decoder = wrapper.get('.image-viewer__decoder');
      expect(decoder.attributes('src')).toBe(ORIGINAL_SRC);

      await completeLoad(decoder);

      expect(visibleImage(wrapper).attributes('src')).toBe(ORIGINAL_SRC);
      expect(wrapper.find('.image-viewer__decoder').exists()).toBe(false);
    });

    it('starts from the original when there is no thumbnail to bridge with', () => {
      const wrapper = mountViewer();

      expect(visibleImage(wrapper).attributes('src')).toBe(ORIGINAL_SRC);
      expect(wrapper.find('.image-viewer__decoder').exists()).toBe(false);
    });

    it('keeps the thumbnail up when the original cannot be decoded', async () => {
      const wrapper = mountViewer({ previewSrc: PREVIEW_SRC });

      await wrapper.get('.image-viewer__decoder').trigger('error');

      // Better a usable thumbnail than an error glyph replacing a picture that renders.
      expect(visibleImage(wrapper).attributes('src')).toBe(PREVIEW_SRC);
      expect(wrapper.find('.image-viewer__error').exists()).toBe(false);
    });

    it('shows the blurred placeholder only until the original arrives', async () => {
      const wrapper = mountViewer({
        previewSrc: PREVIEW_SRC,
        placeholderSrc: PLACEHOLDER_SRC,
      });

      expect(wrapper.find('.image-viewer__placeholder').exists()).toBe(true);

      await completeLoad(wrapper.get('.image-viewer__decoder'));

      expect(wrapper.find('.image-viewer__placeholder').exists()).toBe(false);
    });

    it('shows an error glyph when the visible layer itself fails', async () => {
      const wrapper = mountViewer();

      await visibleImage(wrapper).trigger('error');

      expect(wrapper.find('.image-viewer__error').exists()).toBe(true);
    });
  });

  describe('zooming', () => {
    it('zooms in on a wheel scroll up', async () => {
      const wrapper = mountViewer();
      await completeLoad(visibleImage(wrapper));

      const event = await dispatch(wrapper.element, 'wheel', { deltaY: -240 });

      expect(event.defaultPrevented).toBe(true);
      expect(Number.parseInt(zoomLabel(wrapper), 10)).toBeGreaterThan(100);
    });

    /**
     * The surrounding info panel scrolls. If the viewer swallowed every wheel event that
     * merely passed over it, scrolling the panel would stall whenever the pointer crossed a
     * preview — so an unzoomed scroll-out is deliberately left unhandled.
     */
    it('lets an unzoomed scroll-out through so the surrounding panel can scroll', async () => {
      const wrapper = mountViewer();
      await completeLoad(visibleImage(wrapper));

      const event = await dispatch(wrapper.element, 'wheel', { deltaY: 240 });

      expect(event.defaultPrevented).toBe(false);
      expect(zoomLabel(wrapper)).toBe('100%');
    });

    it('consumes a scroll-out once zoomed, so it zooms back out instead of scrolling', async () => {
      const wrapper = mountViewer();
      await completeLoad(visibleImage(wrapper));
      await dispatch(wrapper.element, 'wheel', { deltaY: -400 });

      const zoomedLabel = zoomLabel(wrapper);
      const event = await dispatch(wrapper.element, 'wheel', { deltaY: 200 });

      expect(event.defaultPrevented).toBe(true);
      expect(Number.parseInt(zoomLabel(wrapper), 10))
        .toBeLessThan(Number.parseInt(zoomedLabel, 10));
    });

    it('zooms with the control buttons and resets to fit', async () => {
      const wrapper = mountViewer();
      await completeLoad(visibleImage(wrapper));

      const [zoomOut, resetZoom, zoomIn] = wrapper.findAll('.image-viewer__controls button');

      expect(zoomOut.attributes('disabled')).toBeDefined();

      await zoomIn.trigger('click');
      expect(zoomLabel(wrapper)).toBe('140%');
      expect(wrapper.get('.image-viewer__controls button').attributes('disabled')).toBeUndefined();

      await resetZoom.trigger('click');
      expect(zoomLabel(wrapper)).toBe('100%');
    });

    it('never zooms out past fit', async () => {
      const wrapper = mountViewer();
      await completeLoad(visibleImage(wrapper));

      await dispatch(wrapper.element, 'wheel', { deltaY: 5000 });

      expect(zoomLabel(wrapper)).toBe('100%');
    });

    it('toggles zoom on double click', async () => {
      const wrapper = mountViewer();
      await completeLoad(visibleImage(wrapper));

      await dispatch(wrapper.element, 'dblclick', {
        clientX: 400,
        clientY: 300,
      });
      expect(zoomLabel(wrapper)).toBe('200%');

      await dispatch(wrapper.element, 'dblclick', {
        clientX: 400,
        clientY: 300,
      });
      expect(zoomLabel(wrapper)).toBe('100%');
    });

    it('zooms about the pointer so the pixel under it stays put', async () => {
      const wrapper = mountViewer();
      await completeLoad(visibleImage(wrapper));

      // Anchor right of centre: the transform must shift left to hold that point in place.
      await dispatch(wrapper.element, 'wheel', {
        deltaY: -240,
        clientX: 700,
        clientY: 300,
      });

      expect(transformOf(wrapper).x).toBeLessThan(0);
    });

    it('resets the view when a different image is selected', async () => {
      const wrapper = mountViewer();
      await completeLoad(visibleImage(wrapper));
      await dispatch(wrapper.element, 'wheel', { deltaY: -400 });
      expect(zoomLabel(wrapper)).not.toBe('100%');

      await wrapper.setProps({ src: 'asset://other.png' });

      expect(zoomLabel(wrapper)).toBe('100%');
      expect(transformOf(wrapper)).toEqual({
        x: 0,
        y: 0,
      });
    });
  });

  describe('panning', () => {
    it('does not pan while the image is fit to the pane', async () => {
      const wrapper = mountViewer();
      await completeLoad(visibleImage(wrapper));

      await dispatch(wrapper.element, 'pointerdown', {
        button: 0,
        pointerId: 1,
        clientX: 0,
        clientY: 0,
      });
      await dispatch(wrapper.element, 'pointermove', {
        pointerId: 1,
        clientX: 120,
        clientY: 0,
      });

      expect(transformOf(wrapper)).toEqual({
        x: 0,
        y: 0,
      });
    });

    /**
     * Clamped against the *rendered* box, not the container: a 1000x500 image in an 800x600
     * pane letterboxes to 800x400, so at 2x only the width overflows and vertical drag has
     * nowhere to go.
     */
    it('pans within bounds once zoomed and refuses to drag past the edge', async () => {
      const wrapper = mountViewer();
      await completeLoad(visibleImage(wrapper), 1000, 500);
      await dispatch(wrapper.element, 'dblclick', {
        clientX: 400,
        clientY: 300,
      });

      await dispatch(wrapper.element, 'pointerdown', {
        button: 0,
        pointerId: 1,
        clientX: 0,
        clientY: 0,
      });
      await dispatch(wrapper.element, 'pointermove', {
        pointerId: 1,
        clientX: 10_000,
        clientY: 10_000,
      });

      // Rendered box 800x400 at 2x = 1600x800, against an 800x600 pane: 400px of horizontal
      // slack and 100px vertical.
      expect(transformOf(wrapper)).toEqual({
        x: 400,
        y: 100,
      });
    });

    it('stops panning after the pointer is released', async () => {
      const wrapper = mountViewer();
      await completeLoad(visibleImage(wrapper), 1000, 500);
      await dispatch(wrapper.element, 'dblclick', {
        clientX: 400,
        clientY: 300,
      });

      await dispatch(wrapper.element, 'pointerdown', {
        button: 0,
        pointerId: 1,
        clientX: 0,
        clientY: 0,
      });
      await dispatch(wrapper.element, 'pointermove', {
        pointerId: 1,
        clientX: 100,
        clientY: 0,
      });
      const afterDrag = transformOf(wrapper);

      await dispatch(wrapper.element, 'pointerup', { pointerId: 1 });
      await dispatch(wrapper.element, 'pointermove', {
        pointerId: 1,
        clientX: 300,
        clientY: 0,
      });

      expect(transformOf(wrapper)).toEqual(afterDrag);
    });
  });

  describe('keyboard', () => {
    it('zooms with + and - and resets with 0', async () => {
      const wrapper = mountViewer();
      await completeLoad(visibleImage(wrapper));

      await dispatch(wrapper.element, 'keydown', { key: '+' });
      expect(zoomLabel(wrapper)).toBe('140%');

      await dispatch(wrapper.element, 'keydown', { key: '-' });
      expect(zoomLabel(wrapper)).toBe('100%');

      await dispatch(wrapper.element, 'keydown', { key: '+' });
      await dispatch(wrapper.element, 'keydown', { key: '0' });
      expect(zoomLabel(wrapper)).toBe('100%');
    });

    /**
     * Arrow keys pan only while zoomed. Unzoomed they must bubble, because Quick View uses
     * them to move between files.
     */
    it('leaves arrow keys alone until the image is zoomed', async () => {
      const wrapper = mountViewer();
      await completeLoad(visibleImage(wrapper), 1000, 500);

      const unzoomed = await dispatch(wrapper.element, 'keydown', { key: 'ArrowRight' });
      expect(unzoomed.defaultPrevented).toBe(false);

      await dispatch(wrapper.element, 'dblclick', {
        clientX: 400,
        clientY: 300,
      });

      const zoomed = await dispatch(wrapper.element, 'keydown', { key: 'ArrowRight' });
      expect(zoomed.defaultPrevented).toBe(true);
      expect(transformOf(wrapper).x).toBeLessThan(0);
    });
  });
});
