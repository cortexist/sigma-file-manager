// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { createPinia, setActivePinia } from 'pinia';
import {
  beforeEach, describe, expect, it, vi,
} from 'vitest';

const order: string[] = [];
const emittedEvents: string[] = [];
const emittedPayloads: unknown[] = [];
const isVisibleMock = vi.fn(async () => false);

function recordingWindow() {
  return {
    setTitle: vi.fn(async () => { order.push('setTitle'); }),
    center: vi.fn(async () => { order.push('center'); }),
    show: vi.fn(async () => { order.push('show'); }),
    setFocus: vi.fn(async () => { order.push('setFocus'); }),
    isVisible: (...args: unknown[]) => isVisibleMock(...(args as [])),
  };
}

vi.mock('@/utils/auxiliary-windows', () => ({
  runAuxiliaryWindowTask: async (
    _label: string,
    task: (context: {
      window: unknown;
      isCurrent: () => boolean;
    }) => Promise<unknown>,
  ) => task({
    window: recordingWindow(),
    isCurrent: () => true,
  }),
  emitAuxiliaryWindowEvent: async (_label: string, event: string, payload: unknown) => {
    emittedEvents.push(event);
    emittedPayloads.push(payload);
    order.push(event === 'quick-view:load-file' ? 'load' : event);
    return true;
  },
  hasAuxiliaryWindowBeenShown: () => true,
  markAuxiliaryWindowShown: vi.fn(),
  findAuxiliaryWindow: async () => recordingWindow(),
  releaseAuxiliaryWindow: async () => undefined,
}));

vi.mock('@tauri-apps/api/event', () => ({ listen: async () => vi.fn() }));
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ label: 'main' }),
}));
vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (value: string) => value,
  invoke: async () => undefined,
}));
vi.mock('@/components/ui/toaster', () => ({
  toast: { custom: vi.fn() },
  ToastStatic: {},
}));
vi.mock('@/localization', () => ({ i18n: { global: { t: (key: string) => key } } }));

const VIDEO = '/home/user/Videos/clip.mp4';
const OTHER_VIDEO = '/home/user/Videos/other.mp4';

describe('quick view background playback', () => {
  beforeEach(() => {
    order.length = 0;
    emittedEvents.length = 0;
    emittedPayloads.length = 0;
    isVisibleMock.mockReset();
    isVisibleMock.mockResolvedValue(false);
    setActivePinia(createPinia());
  });

  /**
   * The gesture that makes background playback usable: pressing the shortcut again on the
   * file you can still hear brings that window back. Reloading it would start the file over,
   * which is the opposite of coming back to what was playing.
   */
  it('restores the playing window instead of reopening the file', async () => {
    const { useQuickViewStore } = await import('@/stores/runtime/quick-view');
    const store = useQuickViewStore();
    store.backgroundPlaybackPath = VIDEO;

    const restored = await store.toggleQuickView(VIDEO);

    expect(restored).toBe(true);
    expect(order).toEqual(['show', 'setFocus', 'quick-view:restored']);
    expect(emittedEvents).not.toContain('quick-view:load-file');
    // The session is over once its window is back on screen.
    expect(store.backgroundPlaybackPath).toBeNull();
    expect(store.lastOpenedPath).toBe(VIDEO);
  });

  /** A different file is a request to watch that file, not to resurrect the last one. */
  it('opens normally when the shortcut is pressed on another file', async () => {
    const { useQuickViewStore } = await import('@/stores/runtime/quick-view');
    const store = useQuickViewStore();
    store.backgroundPlaybackPath = VIDEO;

    await store.toggleQuickView(OTHER_VIDEO);

    expect(emittedEvents).toContain('quick-view:load-file');
    expect(store.lastOpenedPath).toBe(OTHER_VIDEO);
  });

  /**
   * A visible window showing another file over the song — a text file opened while it played
   * — is the same ask: the song comes back in front, where it is, rather than from the top.
   */
  it('restores the playing file over another file in a visible window', async () => {
    isVisibleMock.mockResolvedValue(true);

    const { useQuickViewStore } = await import('@/stores/runtime/quick-view');
    const store = useQuickViewStore();
    store.lastOpenedPath = '/home/user/Documents/notes.md';
    store.backgroundPlaybackPath = VIDEO;

    const restored = await store.toggleQuickView(VIDEO);

    expect(restored).toBe(true);
    expect(order).toEqual(['show', 'setFocus', 'quick-view:restored']);
    expect(emittedEvents).not.toContain('quick-view:load-file');
    expect(store.backgroundPlaybackPath).toBeNull();
    expect(store.lastOpenedPath).toBe(VIDEO);
  });

  /**
   * Another application is handed a viewer and only that: its files arrive read-only, and so
   * does everything the window reaches from them. Sigma's own opens may edit.
   */
  it('marks files from another application read-only and its own editable', async () => {
    const { useQuickViewStore } = await import('@/stores/runtime/quick-view');
    const store = useQuickViewStore();

    await store.openFileFromMainWindow(VIDEO, null, 'external');
    await store.openFileFromMainWindow(OTHER_VIDEO);

    expect(emittedPayloads).toEqual([
      expect.objectContaining({
        path: VIDEO,
        editable: false,
      }),
      expect.objectContaining({
        path: OTHER_VIDEO,
        editable: true,
      }),
    ]);
  });

  /** With the window already on screen the shortcut still means close, as it always did. */
  it('still closes a visible window rather than restoring it', async () => {
    isVisibleMock.mockResolvedValue(true);

    const { useQuickViewStore } = await import('@/stores/runtime/quick-view');
    const store = useQuickViewStore();
    store.lastOpenedPath = VIDEO;
    store.backgroundPlaybackPath = VIDEO;

    await store.toggleQuickView(VIDEO);

    expect(order).not.toContain('show');
    expect(emittedEvents).not.toContain('quick-view:load-file');
  });
});
