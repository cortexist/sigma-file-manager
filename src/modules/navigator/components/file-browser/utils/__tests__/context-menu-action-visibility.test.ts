// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { describe, expect, it } from 'vitest';
import type { DirEntry } from '@/types/dir-entry';
import { isContextMenuActionVisible } from '@/modules/navigator/components/file-browser/utils/context-menu-action-visibility';
import { LOCATIONS_VIRTUAL_PATH } from '@/utils/virtual-path-constants';

function createDirectoryEntry(path: string): DirEntry {
  return {
    name: path,
    path,
    is_file: false,
    is_dir: true,
    is_hidden: false,
    is_symlink: false,
    size: 0,
    item_count: null,
    created_time: 0,
    modified_time: 0,
    accessed_time: 0,
    ext: null,
    mime: null,
  };
}

describe('isContextMenuActionVisible', () => {
  it('allows only location-safe actions for the virtual locations path', () => {
    const entries = [createDirectoryEntry(LOCATIONS_VIRTUAL_PATH)];

    expect(isContextMenuActionVisible('toggle-favorite', entries, { platform: 'windows' })).toBe(true);
    expect(isContextMenuActionVisible('edit-tags', entries, { platform: 'windows' })).toBe(true);
    expect(isContextMenuActionVisible('copy-path', entries, { platform: 'windows' })).toBe(true);
    expect(isContextMenuActionVisible('open-in-new-tab', entries, { platform: 'windows' })).toBe(true);

    expect(isContextMenuActionVisible('rename', entries, { platform: 'windows' })).toBe(false);
    expect(isContextMenuActionVisible('copy', entries, { platform: 'windows' })).toBe(false);
    expect(isContextMenuActionVisible('cut', entries, { platform: 'windows' })).toBe(false);
    expect(isContextMenuActionVisible('paste', entries, { platform: 'windows' })).toBe(false);
    expect(isContextMenuActionVisible('delete', entries, { platform: 'windows' })).toBe(false);
    expect(isContextMenuActionVisible('delete-permanently', entries, { platform: 'windows' })).toBe(false);
    expect(isContextMenuActionVisible('link', entries, { platform: 'windows' })).toBe(false);
    expect(isContextMenuActionVisible('open-with', entries, { platform: 'windows' })).toBe(false);
    expect(isContextMenuActionVisible('properties', entries, { platform: 'windows' })).toBe(false);
    expect(isContextMenuActionVisible('share', entries, { platform: 'windows' })).toBe(false);
  });

  it('blocks destructive actions on windows drive roots but keeps copy available', () => {
    const entries = [createDirectoryEntry('D:/')];

    expect(isContextMenuActionVisible('copy', entries, { platform: 'windows' })).toBe(true);
    expect(isContextMenuActionVisible('copy-path', entries, { platform: 'windows' })).toBe(true);
    expect(isContextMenuActionVisible('open-in-new-tab', entries, { platform: 'windows' })).toBe(true);

    expect(isContextMenuActionVisible('rename', entries, { platform: 'windows' })).toBe(false);
    expect(isContextMenuActionVisible('cut', entries, { platform: 'windows' })).toBe(false);
    expect(isContextMenuActionVisible('delete', entries, { platform: 'windows' })).toBe(false);
    expect(isContextMenuActionVisible('delete-permanently', entries, { platform: 'windows' })).toBe(false);
  });

  it('blocks destructive actions on wsl distribution roots but keeps copy available', () => {
    const entries = [createDirectoryEntry('//wsl.localhost/Ubuntu-24.04/')];

    expect(isContextMenuActionVisible('copy', entries, { platform: 'windows' })).toBe(true);
    expect(isContextMenuActionVisible('copy-path', entries, { platform: 'windows' })).toBe(true);
    expect(isContextMenuActionVisible('open-in-new-tab', entries, { platform: 'windows' })).toBe(true);

    expect(isContextMenuActionVisible('rename', entries, { platform: 'windows' })).toBe(false);
    expect(isContextMenuActionVisible('cut', entries, { platform: 'windows' })).toBe(false);
    expect(isContextMenuActionVisible('link', entries, { platform: 'windows' })).toBe(false);
    expect(isContextMenuActionVisible('delete', entries, { platform: 'windows' })).toBe(false);
    expect(isContextMenuActionVisible('delete-permanently', entries, { platform: 'windows' })).toBe(false);
  });

  it('allows destructive actions inside wsl distributions', () => {
    const entries = [createDirectoryEntry('//wsl.localhost/Ubuntu-24.04/home/user/file.txt')];

    expect(isContextMenuActionVisible('delete', entries, { platform: 'windows' })).toBe(true);
    expect(isContextMenuActionVisible('rename', entries, { platform: 'windows' })).toBe(true);
  });

  it('shows disconnect for network drive entries', () => {
    const entries: DirEntry[] = [{
      ...createDirectoryEntry('Z:/'),
      name: 'test (Z:)',
      drive_metadata: {
        drive_type: 'Network',
        is_removable: false,
        is_mounted: true,
        mount_point: 'Z:\\',
        device_path: 'Z:\\',
      },
    }];

    expect(isContextMenuActionVisible('disconnect', entries, { platform: 'windows' })).toBe(true);
  });

  it('hides disconnect for fixed local drive roots', () => {
    const entries = [createDirectoryEntry('D:/')];

    expect(isContextMenuActionVisible('disconnect', entries, { platform: 'windows' })).toBe(false);
  });

  describe('open containing directory', () => {
    function createFileEntry(path: string): DirEntry {
      return {
        ...createDirectoryEntry(path),
        is_file: true,
        is_dir: false,
      };
    }

    it('is offered for a result shown away from its own folder', () => {
      const entries = [createFileEntry('/home/zero/project/src/main.rs')];

      expect(isContextMenuActionVisible('open-containing-directory', entries, {
        currentDirectoryPath: '/home/zero/project',
      })).toBe(true);
    });

    it('is hidden in a plain listing of the folder the entry is already in', () => {
      const entries = [createFileEntry('/home/zero/project/main.rs')];

      expect(isContextMenuActionVisible('open-containing-directory', entries, {
        currentDirectoryPath: '/home/zero/project',
      })).toBe(false);
    });

    it('is offered when there is no listing behind the entries at all', () => {
      // Global search results, favourites, recent files: no directory is on screen.
      const entries = [createFileEntry('/home/zero/project/main.rs')];

      expect(isContextMenuActionVisible('open-containing-directory', entries)).toBe(true);
    });

    it('is hidden for more than one entry', () => {
      const entries = [
        createFileEntry('/home/zero/a/one.txt'),
        createFileEntry('/home/zero/b/two.txt'),
      ];

      expect(isContextMenuActionVisible('open-containing-directory', entries)).toBe(false);
    });

    it('is hidden for an entry with no folder above it', () => {
      expect(isContextMenuActionVisible('open-containing-directory', [
        createDirectoryEntry('/'),
      ])).toBe(false);
    });
  });
});
