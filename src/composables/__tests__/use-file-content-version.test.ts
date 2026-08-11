// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { defineComponent, nextTick, ref } from 'vue';
import { mount } from '@vue/test-utils';
import {
  beforeEach, describe, expect, it, vi,
} from 'vitest';

const { invokeMock, listenMock, dirChangeHandlers } = vi.hoisted(() => {
  const handlers: ((event: { payload: unknown }) => void)[] = [];

  return {
    invokeMock: vi.fn(),
    dirChangeHandlers: handlers,
    listenMock: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
      if (name === 'dir-change') {
        handlers.push(handler);
      }

      return () => {
        const index = handlers.indexOf(handler);

        if (index >= 0) {
          handlers.splice(index, 1);
        }
      };
    }),
  };
});

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }));

import { useWatchedFileEntry } from '@/composables/use-file-content-version';
import type { DirEntry } from '@/types/dir-entry';

const DIRECTORY = '/home/zero/Downloads';
const FILE = `${DIRECTORY}/asimov.jpeg`;

function fileEntry(modifiedTime: number, size: number): DirEntry {
  return {
    name: 'asimov.jpeg',
    ext: 'jpeg',
    path: FILE,
    size,
    item_count: null,
    modified_time: modifiedTime,
    accessed_time: modifiedTime,
    created_time: 1,
    mime: 'image/jpeg',
    is_file: true,
    is_dir: false,
    is_symlink: false,
    is_hidden: false,
  };
}

/** Stands in for the Rust side: the watcher commands, and whatever the file looks like now. */
function mockBackend(onDisk: DirEntry) {
  invokeMock.mockImplementation((command: string) => {
    if (command === 'get_dir_entry_with_timeout') {
      return Promise.resolve(onDisk);
    }

    return Promise.resolve(undefined);
  });
}

function mountWatcher(path: string | null, initial?: DirEntry | null) {
  const pathRef = ref(path);
  let watched!: ReturnType<typeof useWatchedFileEntry>;

  const wrapper = mount(defineComponent({
    setup() {
      watched = useWatchedFileEntry(() => pathRef.value, { initial: () => initial });
      return () => null;
    },
  }));

  return {
    pathRef,
    wrapper,
    watched,
  };
}

function emitDirChange(watchedPath: string) {
  for (const handler of [...dirChangeHandlers]) {
    handler({
      payload: {
        watchedPath,
        changedPath: '',
        kind: 'modify',
      },
    });
  }
}

/** Long enough to outlast the settle the composable waits out before looking again. */
async function waitForSettle() {
  await vi.advanceTimersByTimeAsync(1000);
  await nextTick();
}

describe('useWatchedFileEntry', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    invokeMock.mockReset();
    listenMock.mockClear();
    dirChangeHandlers.length = 0;
    mockBackend(fileEntry(456, 2048));
  });

  it('adopts the entry it was handed rather than stat-ing a file it was just given', async () => {
    const { watched } = mountWatcher(FILE, fileEntry(123, 1024));
    await vi.advanceTimersByTimeAsync(0);

    expect(watched.version.value).toBe('123-1024');
    expect(invokeMock).not.toHaveBeenCalledWith('get_dir_entry_with_timeout', expect.anything());
  });

  it('watches the directory the file is in', async () => {
    mountWatcher(FILE, fileEntry(123, 1024));
    await vi.advanceTimersByTimeAsync(0);

    expect(invokeMock).toHaveBeenCalledWith('watch_directory', { path: DIRECTORY });
  });

  /**
   * The regression this guards: a file rewritten at the same path left every surface showing
   * what it held before the write, because nothing ever looked at the file again.
   */
  it('looks again once the file has stopped changing, and publishes what it finds', async () => {
    const { watched } = mountWatcher(FILE, fileEntry(123, 1024));
    await vi.advanceTimersByTimeAsync(0);

    emitDirChange(DIRECTORY);

    // Not straight away: a write has been reported, not finished.
    await nextTick();
    expect(watched.version.value).toBe('123-1024');

    await waitForSettle();

    expect(watched.entry.value?.size).toBe(2048);
    expect(watched.version.value).toBe('456-2048');
  });

  it('ignores changes reported for some other directory', async () => {
    const { watched } = mountWatcher(FILE, fileEntry(123, 1024));
    await vi.advanceTimersByTimeAsync(0);

    emitDirChange('/home/zero/Pictures');
    await waitForSettle();

    expect(watched.version.value).toBe('123-1024');
    expect(invokeMock).not.toHaveBeenCalledWith('get_dir_entry_with_timeout', expect.anything());
  });

  it('stats a path it was given nothing for', async () => {
    const { watched } = mountWatcher(FILE);
    await vi.advanceTimersByTimeAsync(0);

    expect(invokeMock).toHaveBeenCalledWith('get_dir_entry_with_timeout', expect.objectContaining({ path: FILE }));
    expect(watched.version.value).toBe('456-2048');
  });

  it('releases the directory when the window showing the file goes away', async () => {
    const { wrapper } = mountWatcher(FILE, fileEntry(123, 1024));
    await vi.advanceTimersByTimeAsync(0);

    wrapper.unmount();
    await vi.advanceTimersByTimeAsync(0);

    expect(invokeMock).toHaveBeenCalledWith('unwatch_directory', { path: DIRECTORY });
    expect(dirChangeHandlers).toHaveLength(0);
  });
});
