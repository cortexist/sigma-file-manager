// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import {
  beforeEach, describe, expect, it, vi,
} from 'vitest';

const { invokeMock, openPlatformDialogMock, savePlatformDialogMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  openPlatformDialogMock: vi.fn(),
  savePlatformDialogMock: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: openPlatformDialogMock,
  save: savePlatformDialogMock,
}));

const { buildPickerRequest, open, save, toPickerFilters, toPickerLocation } = await import('@/utils/sigma-dialog');

beforeEach(() => {
  invokeMock.mockReset();
  openPlatformDialogMock.mockReset();
  savePlatformDialogMock.mockReset();
});

describe('filter translation', () => {
  it('turns bare extensions into globs', () => {
    expect(toPickerFilters([{
      name: 'Images',
      extensions: ['png', 'jpg'],
    }]))
      .toEqual([{
        name: 'Images',
        globs: ['*.png', '*.jpg'],
        mimes: [],
      }]);
  });

  it('tolerates extensions that already carry a dot or a star', () => {
    expect(toPickerFilters([{
      name: 'Audio',
      extensions: ['.mp3', '*.flac'],
    }])[0].globs)
      .toEqual(['*.mp3', '*.flac']);
  });

  it('treats a wildcard extension as no restriction at all', () => {
    expect(toPickerFilters([{
      name: 'All files',
      extensions: ['*'],
    }])[0].globs).toEqual([]);
  });

  it('returns nothing when the caller set no filters', () => {
    expect(toPickerFilters(undefined)).toEqual([]);
  });
});

describe('defaultPath translation', () => {
  it('is the starting folder for an open dialog', () => {
    expect(toPickerLocation('/home/zero/Music', false))
      .toEqual({
        currentFolder: '/home/zero/Music',
        suggestedName: null,
      });
  });

  it('splits into folder and filename for a save dialog', () => {
    expect(toPickerLocation('/home/zero/Music/track.mp3', true))
      .toEqual({
        currentFolder: '/home/zero/Music',
        suggestedName: 'track.mp3',
      });
  });

  it('is a folder for a save dialog when it clearly names one', () => {
    expect(toPickerLocation('/home/zero/Music/', true))
      .toEqual({
        currentFolder: '/home/zero/Music',
        suggestedName: null,
      });
  });

  it('treats a bare name in a save dialog as the suggested filename', () => {
    expect(toPickerLocation('track.mp3', true))
      .toEqual({
        currentFolder: null,
        suggestedName: 'track.mp3',
      });
  });

  it('keeps the root folder addressable', () => {
    expect(toPickerLocation('/track.mp3', true))
      .toEqual({
        currentFolder: '/',
        suggestedName: 'track.mp3',
      });
  });

  it('has no opinion when the caller gave no path', () => {
    expect(toPickerLocation(undefined, false))
      .toEqual({
        currentFolder: null,
        suggestedName: null,
      });
  });
});

describe('request building', () => {
  it('defaults every flag the picker expects rather than omitting it', () => {
    expect(buildPickerRequest({})).toEqual({
      title: '',
      multiple: false,
      directory: false,
      currentFolder: null,
      save: false,
      suggestedName: null,
      filters: [],
      currentFilter: null,
    });
  });

  it('preselects the caller\'s first filter', () => {
    const request = buildPickerRequest({
      filters: [{
        name: 'Images',
        extensions: ['png'],
      }, {
        name: 'All',
        extensions: ['*'],
      }],
    });

    expect(request.currentFilter).toBe('Images');
  });
});

describe('open', () => {
  it('raises Sigma\'s picker rather than the platform dialog', async () => {
    invokeMock.mockResolvedValueOnce(['/home/zero/Music']);

    await open({
      directory: true,
      title: 'Choose',
    });

    expect(invokeMock).toHaveBeenCalledWith('file_picker_open', {
      request: expect.objectContaining({
        directory: true,
        title: 'Choose',
      }),
    });
    expect(openPlatformDialogMock).not.toHaveBeenCalled();
  });

  it('returns a single path when multiple selection was not asked for', async () => {
    invokeMock.mockResolvedValueOnce(['/a', '/b']);

    expect(await open({ directory: true })).toBe('/a');
  });

  it('returns the whole list when multiple selection was asked for', async () => {
    invokeMock.mockResolvedValueOnce(['/a', '/b']);

    expect(await open({ multiple: true })).toEqual(['/a', '/b']);
  });

  it('reports a cancel as null, the way the plugin does', async () => {
    invokeMock.mockResolvedValueOnce([]);

    expect(await open({})).toBeNull();
  });

  it('falls back to the platform dialog when the picker cannot start', async () => {
    invokeMock.mockRejectedValueOnce(new Error('Failed to spawn a file picker'));
    openPlatformDialogMock.mockResolvedValueOnce('/fallback');
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});

    expect(await open({ directory: true })).toBe('/fallback');
    expect(openPlatformDialogMock).toHaveBeenCalled();

    warn.mockRestore();
  });
});

describe('save', () => {
  it('asks the picker for a save dialog', async () => {
    invokeMock.mockResolvedValueOnce(['/home/zero/out.zip']);

    expect(await save({ defaultPath: '/home/zero/out.zip' })).toBe('/home/zero/out.zip');
    expect(invokeMock).toHaveBeenCalledWith('file_picker_open', {
      request: expect.objectContaining({
        save: true,
        suggestedName: 'out.zip',
      }),
    });
  });

  it('reports a cancel as null', async () => {
    invokeMock.mockResolvedValueOnce([]);

    expect(await save({})).toBeNull();
  });

  it('falls back to the platform dialog when the picker cannot start', async () => {
    invokeMock.mockRejectedValueOnce(new Error('Failed to spawn a file picker'));
    savePlatformDialogMock.mockResolvedValueOnce('/fallback.zip');
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});

    expect(await save({})).toBe('/fallback.zip');

    warn.mockRestore();
  });
});
