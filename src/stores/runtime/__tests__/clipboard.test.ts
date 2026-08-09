// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
// Copyright © 2026 Cortexist, LLC (modifications). All rights reserved.

import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import type { DirEntry } from '@/types/dir-entry';

const invokeMock = vi.hoisted(() => vi.fn());
const copyMoveStartJobMock = vi.hoisted(() => vi.fn());
const handleDirectoryContentsChangedMock = vi.hoisted(() => vi.fn());
const invalidateDirSizesMock = vi.hoisted(() => vi.fn());
const clipboardSettingsMock = vi.hoisted(() => ({
  showToolbarForExternalImages: true,
  showToolbarForExternalPaths: true,
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}));

vi.mock('@/stores/storage/user-settings', () => ({
  useUserSettingsStore: () => ({
    userSettings: {
      clipboard: clipboardSettingsMock,
    },
  }),
}));

vi.mock('@/stores/runtime/copy-move-jobs', () => ({
  useCopyMoveJobsStore: () => ({
    startJob: copyMoveStartJobMock,
  }),
}));

vi.mock('@/stores/storage/workspaces', () => ({
  useWorkspacesStore: () => ({
    handleDirectoryContentsChanged: handleDirectoryContentsChangedMock,
  }),
}));

vi.mock('@/stores/runtime/dir-sizes', () => ({
  useDirSizesStore: () => ({
    invalidate: invalidateDirSizesMock,
  }),
}));

import { useClipboardStore } from '@/stores/runtime/clipboard';

function createEntry(overrides: Partial<DirEntry> = {}): DirEntry {
  return {
    name: 'file.txt',
    ext: 'txt',
    path: 'C:/Source/file.txt',
    size: 10,
    item_count: null,
    modified_time: 0,
    accessed_time: 0,
    created_time: 0,
    mime: 'text/plain',
    is_file: true,
    is_dir: false,
    is_symlink: false,
    is_hidden: false,
    link_type: null,
    link_target: null,
    link_status: null,
    hard_link_count: null,
    ...overrides,
  };
}

function createDeferred<T = void>() {
  let resolvePromise!: (value: T | PromiseLike<T>) => void;
  let rejectPromise!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolve, reject) => {
    resolvePromise = resolve;
    rejectPromise = reject;
  });

  return {
    promise,
    resolve: resolvePromise,
    reject: rejectPromise,
  };
}

async function flushPromises(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

describe('clipboard store', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    invokeMock.mockReset();
    copyMoveStartJobMock.mockReset();
    handleDirectoryContentsChangedMock.mockReset();
    invalidateDirSizesMock.mockReset();
    clipboardSettingsMock.showToolbarForExternalImages = true;
    clipboardSettingsMock.showToolbarForExternalPaths = true;
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('syncs local file clipboard entries to the system clipboard', async () => {
    invokeMock.mockResolvedValue(undefined);
    const store = useClipboardStore();

    store.setClipboard('copy', [
      createEntry({ path: 'C:/Source/file.txt' }),
    ]);
    await flushPromises();

    expect(invokeMock).toHaveBeenCalledWith('set_system_clipboard_files', {
      paths: ['C:/Source/file.txt'],
      operation: 'copy',
    });
    expect(store.hasFileItems).toBe(true);
    expect(store.hasImageContent).toBe(false);
    expect(store.showClipboardUi).toBe(true);
  });

  it('waits for a pending system clipboard write before syncing from the system clipboard', async () => {
    const clipboardWrite = createDeferred();
    invokeMock.mockImplementation(async (commandName: string, args?: unknown) => {
      if (commandName === 'set_system_clipboard_files') {
        return await clipboardWrite.promise;
      }

      if (commandName === 'read_system_clipboard_files') {
        return {
          paths: ['C:/Source/file.txt'],
          operation: 'copy',
        };
      }

      if (commandName === 'paths_are_directories') {
        return [false];
      }

      if (commandName === 'get_dir_entry_with_timeout') {
        return createEntry({
          path: (args as { path: string }).path,
        });
      }

      return undefined;
    });
    const store = useClipboardStore();

    store.setClipboard('copy', [
      createEntry({ path: 'C:/Source/file.txt' }),
    ]);
    const syncPromise = store.syncFromSystemClipboard();
    await flushPromises();

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith('set_system_clipboard_files', {
      paths: ['C:/Source/file.txt'],
      operation: 'copy',
    });

    clipboardWrite.resolve();
    await syncPromise;

    expect(invokeMock).toHaveBeenCalledWith('read_system_clipboard_files');
    expect(store.clipboardItems).toHaveLength(1);
    expect(store.clipboardItems[0].path).toBe('C:/Source/file.txt');
    expect(store.showClipboardUi).toBe(true);
  });

  it('preserves internal clipboard UI after syncing equivalent system paths', async () => {
    invokeMock.mockImplementation(async (commandName: string, args?: unknown) => {
      if (commandName === 'read_system_clipboard_files') {
        return {
          paths: ['c:/source/file.txt'],
          operation: 'copy',
        };
      }

      if (commandName === 'paths_are_directories') {
        return [false];
      }

      if (commandName === 'get_dir_entry_with_timeout') {
        return createEntry({
          path: (args as { path: string }).path,
        });
      }

      return undefined;
    });
    const store = useClipboardStore();

    store.setClipboard('copy', [
      createEntry({ path: 'C:/Source/file.txt' }),
    ], {
      syncToSystemClipboard: false,
    });

    await store.syncFromSystemClipboard();

    expect(store.showClipboardUi).toBe(true);
    expect(store.hasFileItems).toBe(true);
  });

  it('marks file paths synced from the system clipboard as external', async () => {
    invokeMock.mockImplementation(async (commandName: string, args?: unknown) => {
      if (commandName === 'read_system_clipboard_files') {
        return {
          paths: ['C:/External/photo.png'],
          operation: 'copy',
        };
      }

      if (commandName === 'paths_are_directories') {
        return [false];
      }

      if (commandName === 'get_dir_entry_with_timeout') {
        return createEntry({
          name: 'photo.png',
          ext: 'png',
          path: (args as { path: string }).path,
          mime: 'image/png',
        });
      }

      if (commandName === 'read_system_clipboard_image_info') {
        return {
          width: 200,
          height: 100,
        };
      }

      return undefined;
    });
    const store = useClipboardStore();

    await store.syncFromSystemClipboard();

    expect(store.hasFileItems).toBe(true);
    expect(store.hasImageContent).toBe(false);
    expect(store.clipboardItems[0].path).toBe('C:/External/photo.png');
    expect(store.showClipboardUi).toBe(true);
    expect(invokeMock).not.toHaveBeenCalledWith('read_system_clipboard_image_info');
  });

  it('syncs image clipboard content when the system clipboard has no file list', async () => {
    invokeMock.mockImplementation(async (commandName: string) => {
      if (commandName === 'read_system_clipboard_files') {
        return {
          paths: [],
          operation: 'copy',
        };
      }

      if (commandName === 'read_system_clipboard_image_info') {
        return {
          width: 252,
          height: 358,
          sizeBytes: 360864,
          clipboardSequence: 12,
        };
      }

      if (commandName === 'save_system_clipboard_image_to_temp') {
        return {
          path: 'C:/Temp/clipboard-image.png',
          sizeBytes: 7864320,
        };
      }

      return undefined;
    });
    const store = useClipboardStore();

    await store.syncFromSystemClipboard();

    expect(store.hasItems).toBe(true);
    expect(store.hasFileItems).toBe(false);
    expect(store.hasImageContent).toBe(true);
    expect(store.showClipboardUi).toBe(true);
    expect(store.itemCount).toBe(1);
    expect(store.clipboardImage).toEqual({
      width: 252,
      height: 358,
      sizeBytes: 360864,
      clipboardSequence: 12,
    });
    expect(invokeMock).not.toHaveBeenCalledWith('save_system_clipboard_image_to_temp');

    await store.ensureSystemClipboardImageSaved();

    expect(store.clipboardImage).toEqual({
      width: 252,
      height: 358,
      sizeBytes: 360864,
      clipboardSequence: 12,
      tempPath: 'C:/Temp/clipboard-image.png',
      tempVersion: expect.any(Number),
      savedSizeBytes: 7864320,
    });
    expect(store.canPasteTo('C:/Target')).toBe(true);
  });

  it('saves clipboard image before paste when temp file is missing', async () => {
    invokeMock.mockImplementation(async (commandName: string) => {
      if (commandName === 'save_system_clipboard_image_to_temp') {
        return {
          path: 'C:/Temp/clipboard-image.png',
          sizeBytes: 12000,
        };
      }

      if (commandName === 'paste_saved_clipboard_image') {
        return {
          success: true,
          copied_count: 1,
          failed_count: 0,
          skipped_count: 0,
          path: 'C:/Target/clipboard-image.png',
        };
      }

      return undefined;
    });
    const store = useClipboardStore();
    store.setClipboardImage({
      width: 100,
      height: 50,
      sizeBytes: 20000,
      clipboardSequence: 3,
    });

    const result = await store.pasteItems('C:/Target');
    await flushPromises();

    expect(result.success).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith('save_system_clipboard_image_to_temp');
    expect(invokeMock).toHaveBeenCalledWith('paste_saved_clipboard_image', {
      sourcePath: 'C:/Temp/clipboard-image.png',
      destinationPath: 'C:/Target',
    });
  });

  it('clears local and system clipboard state after a successful image paste', async () => {
    invokeMock.mockImplementation(async (commandName: string) => {
      if (commandName === 'paste_saved_clipboard_image') {
        return {
          success: true,
          copied_count: 1,
          failed_count: 0,
          skipped_count: 0,
          path: 'C:/Target/clipboard-image.png',
        };
      }

      return undefined;
    });
    const store = useClipboardStore();
    store.setClipboardImage({
      width: 100,
      height: 50,
      sizeBytes: 20000,
      tempPath: 'C:/Temp/clipboard-image.png',
      tempVersion: 1,
      savedSizeBytes: 12000,
    });

    const result = await store.pasteItems('C:/Target');
    await flushPromises();

    expect(result.success).toBe(true);
    expect(store.hasItems).toBe(false);
    expect(invokeMock).toHaveBeenCalledWith('paste_saved_clipboard_image', {
      sourcePath: 'C:/Temp/clipboard-image.png',
      destinationPath: 'C:/Target',
    });
    expect(invokeMock).toHaveBeenCalledWith('clear_system_clipboard_files');
  });

  it('clears local and system clipboard state after a successful file paste', async () => {
    invokeMock.mockResolvedValue(undefined);
    copyMoveStartJobMock.mockResolvedValue({
      success: true,
      copied_count: 1,
      failed_count: 0,
      skipped_count: 0,
    });
    const store = useClipboardStore();
    store.setClipboard('copy', [
      createEntry({ path: 'C:/Source/file.txt' }),
    ], {
      syncToSystemClipboard: false,
    });

    const result = await store.pasteItems('C:/Target');
    await flushPromises();

    expect(result.success).toBe(true);
    expect(copyMoveStartJobMock).toHaveBeenCalledWith(
      'copy',
      ['C:/Source/file.txt'],
      'C:/Target',
      null,
      undefined,
      expect.objectContaining({
        displayPath: 'Target',
      }),
    );
    expect(store.hasItems).toBe(false);
    expect(invokeMock).toHaveBeenCalledWith('clear_system_clipboard_files');
  });

  it('restores file clipboard state when paste fails', async () => {
    copyMoveStartJobMock.mockResolvedValue({
      success: false,
      error: 'Copy failed',
    });
    const store = useClipboardStore();
    store.setClipboard('copy', [
      createEntry({ path: 'C:/Source/file.txt' }),
    ], {
      syncToSystemClipboard: false,
    });

    const result = await store.pasteItems('C:/Target');

    expect(result.success).toBe(false);
    expect(store.hasFileItems).toBe(true);
    expect(store.clipboardItems[0].path).toBe('C:/Source/file.txt');
  });

  it('does not log expected transient Windows clipboard access errors', async () => {
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    invokeMock.mockRejectedValue('OpenClipboard failed: Access is denied. (0x80070005)');
    const store = useClipboardStore();

    await expect(store.readSystemClipboardFiles()).resolves.toBeNull();

    expect(consoleErrorSpy).not.toHaveBeenCalled();
  });

  it('keeps local clipboard state when syncing from the system clipboard fails', async () => {
    invokeMock.mockRejectedValue('OpenClipboard failed: Access is denied. (0x80070005)');
    const store = useClipboardStore();

    store.setClipboard('copy', [
      createEntry({ path: 'C:/Source/file.txt' }),
    ], {
      syncToSystemClipboard: false,
    });

    await store.syncFromSystemClipboard();

    expect(store.hasFileItems).toBe(true);
    expect(store.clipboardItems[0].path).toBe('C:/Source/file.txt');
  });

  /**
   * A Linux system clipboard carries only a file list, so `read_system_clipboard_files`
   * always reports `copy` there. Syncing used to overwrite the local operation with it, which
   * silently downgraded a cut to a copy: the paste then duplicated the files and left the
   * source in place. Our own entry has to keep the operation we recorded for it.
   */
  it('keeps an internal cut a move when the system clipboard reports copy', async () => {
    invokeMock.mockImplementation(async (commandName: string) => {
      if (commandName === 'path_exists') {
        return true;
      }

      if (commandName === 'read_system_clipboard_files') {
        // What Linux reports back for a file list it cannot tag as a cut.
        return {
          paths: ['C:/Source/file.txt'],
          operation: 'copy',
        };
      }

      return undefined;
    });
    const store = useClipboardStore();

    store.setClipboard('move', [
      createEntry({ path: 'C:/Source/file.txt' }),
    ], {
      syncToSystemClipboard: false,
    });

    await store.syncFromSystemClipboard();

    expect(store.clipboardType).toBe('move');
    expect(store.isMoveOperation).toBe(true);
  });

  it('pastes an internal cut as a move after a system clipboard sync', async () => {
    invokeMock.mockImplementation(async (commandName: string) => {
      if (commandName === 'path_exists') {
        return true;
      }

      if (commandName === 'read_system_clipboard_files') {
        return {
          paths: ['C:/Source/file.txt'],
          operation: 'copy',
        };
      }

      // The sync rebuilds its entries from the paths it read back.
      if (commandName === 'get_dir_entry_with_timeout') {
        return createEntry({ path: 'C:/Source/file.txt' });
      }

      if (commandName === 'paths_are_directories') {
        return [false];
      }

      return undefined;
    });
    copyMoveStartJobMock.mockResolvedValue({ success: true });
    const store = useClipboardStore();

    store.setClipboard('move', [
      createEntry({ path: 'C:/Source/file.txt' }),
    ], {
      syncToSystemClipboard: false,
    });

    await store.syncFromSystemClipboard();
    await store.pasteItems('C:/Destination');

    expect(copyMoveStartJobMock).toHaveBeenCalledWith(
      'move',
      ['C:/Source/file.txt'],
      'C:/Destination',
      null,
      undefined,
      expect.anything(),
    );
  });

  /**
   * The paste paths in `use-file-browser-selection` and `use-dir-entry-actions` read the
   * system clipboard directly rather than going through `pasteItems`, so they resolve the
   * operation through this. It is the single place that decides cut-versus-copy.
   */
  describe('resolveSystemClipboardOperation', () => {
    it('prefers our recorded move over a copy the OS could not represent', () => {
      const store = useClipboardStore();

      store.setClipboard('move', [
        createEntry({ path: 'C:/Source/file.txt' }),
      ], {
        syncToSystemClipboard: false,
      });

      expect(store.resolveSystemClipboardOperation(['C:/Source/file.txt'], 'copy')).toBe('move');
    });

    it('matches our entry regardless of path order', () => {
      const store = useClipboardStore();

      store.setClipboard('move', [
        createEntry({ path: 'C:/Source/a.txt' }),
        createEntry({ path: 'C:/Source/b.txt' }),
      ], {
        syncToSystemClipboard: false,
      });

      expect(store.resolveSystemClipboardOperation(
        ['C:/Source/b.txt', 'C:/Source/a.txt'],
        'copy',
      )).toBe('move');
    });

    it('defers to the OS when the entry is not ours', () => {
      const store = useClipboardStore();

      store.setClipboard('move', [
        createEntry({ path: 'C:/Source/file.txt' }),
      ], {
        syncToSystemClipboard: false,
      });

      // A different set of paths means somebody else replaced the clipboard.
      expect(store.resolveSystemClipboardOperation(['C:/Other/file.txt'], 'copy')).toBe('copy');
    });

    it('defers to the OS when a superset of our paths is on the clipboard', () => {
      const store = useClipboardStore();

      store.setClipboard('move', [
        createEntry({ path: 'C:/Source/a.txt' }),
      ], {
        syncToSystemClipboard: false,
      });

      expect(store.resolveSystemClipboardOperation(
        ['C:/Source/a.txt', 'C:/Source/b.txt'],
        'copy',
      )).toBe('copy');
    });

    it('defers to the OS with an empty local clipboard', () => {
      const store = useClipboardStore();

      expect(store.resolveSystemClipboardOperation(['C:/Source/file.txt'], 'move')).toBe('move');
      expect(store.resolveSystemClipboardOperation(['C:/Source/file.txt'], 'copy')).toBe('copy');
    });
  });

  it('still takes the operation from a foreign clipboard entry', async () => {
    invokeMock.mockImplementation(async (commandName: string) => {
      if (commandName === 'path_exists') {
        return true;
      }

      if (commandName === 'read_system_clipboard_files') {
        // Different paths, so this is somebody else's entry and it is authoritative.
        return {
          paths: ['C:/Elsewhere/other.txt'],
          operation: 'copy',
        };
      }

      return undefined;
    });
    const store = useClipboardStore();

    store.setClipboard('move', [
      createEntry({ path: 'C:/Source/file.txt' }),
    ], {
      syncToSystemClipboard: false,
    });

    await store.syncFromSystemClipboard();

    expect(store.clipboardType).toBe('copy');
  });

  /** Windows does round-trip the operation, and a foreign move must survive the sync. */
  it('accepts a foreign move reported by the system clipboard', async () => {
    invokeMock.mockImplementation(async (commandName: string) => {
      if (commandName === 'path_exists') {
        return true;
      }

      if (commandName === 'read_system_clipboard_files') {
        return {
          paths: ['C:/Elsewhere/other.txt'],
          operation: 'move',
        };
      }

      return undefined;
    });
    const store = useClipboardStore();

    await store.syncFromSystemClipboard();

    expect(store.clipboardType).toBe('move');
  });

  it('dismisses move clipboard when external paste removed the source paths', async () => {
    invokeMock.mockImplementation(async (commandName: string) => {
      if (commandName === 'path_exists') {
        return false;
      }

      if (commandName === 'read_system_clipboard_files') {
        return {
          paths: ['C:/Source/file.txt'],
          operation: 'move',
        };
      }

      return undefined;
    });
    const store = useClipboardStore();

    store.setClipboard('move', [
      createEntry({ path: 'C:/Source/file.txt' }),
    ], {
      syncToSystemClipboard: false,
    });

    const consumed = await store.checkExternalClipboardConsumption();

    expect(consumed).toBe(true);
    expect(store.hasItems).toBe(false);
    expect(handleDirectoryContentsChangedMock).toHaveBeenCalledWith(['C:/Source']);
    expect(invalidateDirSizesMock).toHaveBeenCalledWith(['C:/Source/file.txt']);
  });

  it('dismisses clipboard when the system clipboard no longer has file paths', async () => {
    invokeMock.mockImplementation(async (commandName: string) => {
      if (commandName === 'path_exists') {
        return true;
      }

      if (commandName === 'read_system_clipboard_files') {
        return {
          paths: [],
          operation: 'copy',
        };
      }

      if (commandName === 'read_system_clipboard_image_info') {
        return null;
      }

      return undefined;
    });
    const store = useClipboardStore();

    store.setClipboard('copy', [
      createEntry({ path: 'C:/Source/file.txt' }),
    ], {
      syncToSystemClipboard: false,
    });

    const consumed = await store.checkExternalClipboardConsumption();

    expect(consumed).toBe(true);
    expect(store.hasItems).toBe(false);
  });

  it('keeps copy clipboard when external apps leave the source paths and file list intact', async () => {
    invokeMock.mockImplementation(async (commandName: string) => {
      if (commandName === 'path_exists') {
        return true;
      }

      if (commandName === 'read_system_clipboard_files') {
        return {
          paths: ['C:/Source/file.txt'],
          operation: 'copy',
        };
      }

      return undefined;
    });
    const store = useClipboardStore();

    store.setClipboard('copy', [
      createEntry({ path: 'C:/Source/file.txt' }),
    ], {
      syncToSystemClipboard: false,
    });

    const consumed = await store.checkExternalClipboardConsumption();

    expect(consumed).toBe(false);
    expect(store.hasFileItems).toBe(true);
  });

  it('hides the toolbar for external file paths when the setting is disabled', async () => {
    clipboardSettingsMock.showToolbarForExternalPaths = false;

    invokeMock.mockImplementation(async (commandName: string, args?: unknown) => {
      if (commandName === 'read_system_clipboard_files') {
        return {
          paths: ['C:/External/file.txt'],
          operation: 'copy',
        };
      }

      if (commandName === 'paths_are_directories') {
        return [false];
      }

      if (commandName === 'get_dir_entry_with_timeout') {
        return createEntry({
          path: (args as { path: string }).path,
        });
      }

      return undefined;
    });
    const store = useClipboardStore();

    await store.syncFromSystemClipboard();

    expect(store.hasItems).toBe(true);
    expect(store.showClipboardUi).toBe(false);
  });

  it('hides the clipboard UI for external images when the setting is disabled', async () => {
    clipboardSettingsMock.showToolbarForExternalImages = false;

    invokeMock.mockImplementation(async (commandName: string) => {
      if (commandName === 'read_system_clipboard_files') {
        return {
          paths: [],
          operation: 'copy',
        };
      }

      if (commandName === 'read_system_clipboard_image_info') {
        return {
          width: 100,
          height: 50,
          sizeBytes: 20000,
          clipboardSequence: 4,
        };
      }

      return undefined;
    });
    const store = useClipboardStore();

    await store.syncFromSystemClipboard();

    expect(store.hasItems).toBe(true);
    expect(store.showClipboardUi).toBe(false);
    expect(store.canPasteTo('C:/Target')).toBe(true);
  });

  it('always shows the clipboard UI for internal clipboard even when external settings are disabled', async () => {
    clipboardSettingsMock.showToolbarForExternalImages = false;
    clipboardSettingsMock.showToolbarForExternalPaths = false;
    invokeMock.mockResolvedValue(undefined);
    const store = useClipboardStore();

    store.setClipboard('copy', [
      createEntry({ path: 'C:/Source/file.txt' }),
    ], {
      syncToSystemClipboard: false,
    });

    expect(store.showClipboardUi).toBe(true);
  });
});
