// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { createPinia, setActivePinia } from 'pinia';
import {
  beforeEach, describe, expect, it, vi,
} from 'vitest';

const order: string[] = [];
const hasAuxiliaryWindowBeenShownMock = vi.fn();
const markAuxiliaryWindowShownMock = vi.fn();

function recordingWindow(prefix: string) {
  return {
    setTitle: vi.fn(async () => { order.push(`${prefix}setTitle`); }),
    center: vi.fn(async () => { order.push(`${prefix}center`); }),
    show: vi.fn(async () => { order.push(`${prefix}show`); }),
    setFocus: vi.fn(async () => { order.push(`${prefix}setFocus`); }),
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
    window: recordingWindow(''),
    isCurrent: () => true,
  }),
  emitAuxiliaryWindowEvent: async () => {
    order.push('load');
    return true;
  },
  hasAuxiliaryWindowBeenShown: (...args: unknown[]) => hasAuxiliaryWindowBeenShownMock(...args),
  markAuxiliaryWindowShown: (...args: unknown[]) => markAuxiliaryWindowShownMock(...args),
  findAuxiliaryWindow: async () => null,
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

describe('quick view open ordering', () => {
  beforeEach(() => {
    order.length = 0;
    setActivePinia(createPinia());
    hasAuxiliaryWindowBeenShownMock.mockReset();
    markAuxiliaryWindowShownMock.mockReset();
  });

  /**
   * The regression this guards: the file used to be handed over before the window was ever
   * shown. On a prelaunched window — one that has existed hidden since startup and so has no
   * surface — the media pipeline could not preroll, and the first quick view of every session
   * hung behind a spinner that never cleared.
   */
  it('shows the window before handing over the file on the first open', async () => {
    hasAuxiliaryWindowBeenShownMock.mockReturnValue(false);

    const { useQuickViewStore } = await import('@/stores/runtime/quick-view');
    await useQuickViewStore().openFileFromMainWindow(VIDEO);

    expect(order).toEqual(['setTitle', 'center', 'show', 'load', 'setFocus']);
    expect(markAuxiliaryWindowShownMock).toHaveBeenCalledWith('quick-view');
  });

  /**
   * Once the window has a surface it keeps it through being hidden, so later opens can go back
   * to loading first — which is what makes the window appear with its content already there
   * instead of filling in afterwards.
   */
  it('hands over the file before showing once the window has been on screen', async () => {
    hasAuxiliaryWindowBeenShownMock.mockReturnValue(true);

    const { useQuickViewStore } = await import('@/stores/runtime/quick-view');
    await useQuickViewStore().openFileFromMainWindow(VIDEO);

    expect(order).toEqual(['setTitle', 'center', 'load', 'show', 'setFocus']);
  });
});
