// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { defineComponent, nextTick, ref } from 'vue';
import { mount } from '@vue/test-utils';
import {
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import { useInfoPanelMediaInfo } from '../use-info-panel-media-info';

const readMediaInfo = vi.fn();

// The real summarizer still runs, so the rows asserted here are the ones the panel renders.
vi.mock('@/utils/media-info', async importOriginal => ({
  ...await importOriginal<typeof import('@/utils/media-info')>(),
  readMediaInfo: (...args: unknown[]) => readMediaInfo(...args),
}));

const VIDEO = '/home/user/Videos/a.mp4';
const OTHER_VIDEO = '/home/user/Videos/b.mp4';
const TEXT = '/home/user/notes.txt';

function videoInfo(codec: string) {
  return {
    container: null,
    durationMs: 1000,
    video: [{
      codec,
      width: 1920,
      height: 1080,
      frameRate: null,
      bitrateBps: null,
      decoder: null,
    }],
    audio: [],
    image: null,
  };
}

/** Mounts the composable against a path we can change, and hands back both. */
function mountComposable(initialPath: string | undefined) {
  const path = ref(initialPath);
  // Assigned by `setup`, which `mount` runs before it returns.
  let rows!: ReturnType<typeof useInfoPanelMediaInfo>['rows'];

  const wrapper = mount(defineComponent({
    setup() {
      ({ rows } = useInfoPanelMediaInfo(() => path.value));
      return () => null;
    },
  }));

  return {
    path,
    wrapper,
    rows,
  };
}

describe('useInfoPanelMediaInfo', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    readMediaInfo.mockReset();
    readMediaInfo.mockResolvedValue(videoInfo('H.264'));
  });

  it('reads once the selection settles and lists what came back', async () => {
    const { rows } = mountComposable(VIDEO);

    // Nothing yet: the debounce has not elapsed.
    expect(readMediaInfo).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(200);

    expect(readMediaInfo).toHaveBeenCalledExactlyOnceWith(VIDEO);
    expect(rows.value.map(row => row.value)).toContain('H.264');
  });

  /**
   * The regression this guards: holding an arrow key through a folder used to start a read per
   * entry, and each one opens the file and decodes enough of it to answer.
   */
  it('reads only the file the cursor lands on, not every one it passes', async () => {
    const { path } = mountComposable(VIDEO);

    for (const next of ['/a.mp4', '/b.mp4', '/c.mp4', '/d.mp4']) {
      path.value = next;
      await vi.advanceTimersByTimeAsync(20);
    }

    expect(readMediaInfo).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(200);

    expect(readMediaInfo).toHaveBeenCalledExactlyOnceWith('/d.mp4');
  });

  /** Showing nothing for a moment is honest; showing the last file's numbers is not. */
  it('drops the previous rows the instant the selection changes', async () => {
    const { path, rows } = mountComposable(VIDEO);
    await vi.advanceTimersByTimeAsync(200);
    expect(rows.value.length).toBeGreaterThan(0);

    path.value = OTHER_VIDEO;
    await nextTick();

    // Cleared without waiting for the debounce, let alone for the new answer.
    expect(rows.value).toEqual([]);
    expect(readMediaInfo).toHaveBeenCalledTimes(1);
  });

  it('never asks about a file that is not a recording or a still', async () => {
    const { path } = mountComposable(TEXT);
    await vi.advanceTimersByTimeAsync(200);

    expect(readMediaInfo).not.toHaveBeenCalled();

    path.value = undefined;
    await vi.advanceTimersByTimeAsync(200);

    expect(readMediaInfo).not.toHaveBeenCalled();
  });

  /** A slow answer for a file the cursor has left must not overwrite the current one. */
  it('discards an answer that arrives after the selection moved on', async () => {
    let resolveFirst: (value: unknown) => void = () => {};

    readMediaInfo.mockImplementationOnce(() => new Promise((resolve) => {
      resolveFirst = resolve;
    }));

    const { path, rows } = mountComposable(VIDEO);
    await vi.advanceTimersByTimeAsync(200);

    path.value = OTHER_VIDEO;
    readMediaInfo.mockResolvedValue(videoInfo('AV1'));
    await vi.advanceTimersByTimeAsync(200);

    // The first file finally answers, long after the cursor left it.
    resolveFirst(videoInfo('H.264'));
    await vi.advanceTimersByTimeAsync(0);

    const values = rows.value.map(row => row.value);
    expect(values).toContain('AV1');
    expect(values).not.toContain('H.264');
  });

  it('contributes no rows when the file cannot be read', async () => {
    readMediaInfo.mockRejectedValue(new Error('unsupported'));
    const { rows } = mountComposable(VIDEO);

    await vi.advanceTimersByTimeAsync(200);

    expect(rows.value).toEqual([]);
  });
});
