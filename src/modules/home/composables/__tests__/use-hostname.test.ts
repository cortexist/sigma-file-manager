// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import {
  beforeEach, describe, expect, it, vi,
} from 'vitest';

const hostnameMock = vi.fn();

vi.mock('@tauri-apps/plugin-os', () => ({
  hostname: () => hostnameMock(),
}));

// The composable caches on module scope, so each case needs a fresh module.
async function loadUseHostname() {
  vi.resetModules();
  return (await import('../use-hostname')).useHostname;
}

// A macrotask boundary drains every pending microtask, so the composable's
// `.then` has definitely run. Cases that assert the ref is *still* null need
// this: without it they pass whether the guard exists or not, because
// "not resolved yet" and "resolved to nothing" look identical.
function flushPromises() {
  return new Promise(resolve => setTimeout(resolve, 0));
}

describe('useHostname', () => {
  beforeEach(() => {
    hostnameMock.mockReset();
  });

  it('exposes the resolved hostname', async () => {
    hostnameMock.mockResolvedValue('nowhere');
    const useHostname = await loadUseHostname();

    const { hostname } = useHostname();
    await vi.waitFor(() => expect(hostname.value).toBe('nowhere'));
  });

  it('starts null so callers can render their fallback heading', async () => {
    hostnameMock.mockReturnValue(new Promise(() => {}));
    const useHostname = await loadUseHostname();

    expect(useHostname().hostname.value).toBeNull();
  });

  it('stays null when the lookup fails', async () => {
    hostnameMock.mockRejectedValue(new Error('os plugin unavailable'));
    const useHostname = await loadUseHostname();

    const { hostname } = useHostname();
    await flushPromises();
    expect(hostname.value).toBeNull();
  });

  it('ignores a blank hostname rather than rendering an empty heading', async () => {
    hostnameMock.mockResolvedValue('   ');
    const useHostname = await loadUseHostname();

    const { hostname } = useHostname();
    await flushPromises();
    expect(hostname.value).toBeNull();
  });

  // The home page and the home banner both call this; the name cannot change
  // while the app is open, so the second caller must not cost another IPC hop.
  it('looks the hostname up once no matter how many callers ask', async () => {
    hostnameMock.mockResolvedValue('nowhere');
    const useHostname = await loadUseHostname();

    const first = useHostname();
    const second = useHostname();
    await vi.waitFor(() => expect(first.hostname.value).toBe('nowhere'));

    expect(second.hostname.value).toBe('nowhere');
    expect(hostnameMock).toHaveBeenCalledTimes(1);
  });
});
