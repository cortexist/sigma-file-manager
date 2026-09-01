// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { describe, expect, it } from 'vitest';
import * as LucideIcons from '@lucide/vue';
import GithubFillIcon from '@/components/icons/github-fill-icon.vue';
import { getIconComponent, userDirectoryIconNames } from '../use-user-directories';

describe('user directory icon resolution', () => {
  it('offers the GitHub mark in the chooser', () => {
    expect(userDirectoryIconNames).toContain('GithubFillIcon');
  });

  it('resolves a bundled icon that Lucide does not provide', () => {
    expect(getIconComponent('GithubFillIcon')).toBe(GithubFillIcon);
  });

  it('still resolves Lucide icons', () => {
    expect(getIconComponent('HomeIcon')).toBe(LucideIcons.HomeIcon);
  });

  it('falls back to a folder for an unknown name', () => {
    expect(getIconComponent('NoSuchIcon')).toBe(LucideIcons.FolderIcon);
  });

  // Every name the chooser renders must resolve to something real — a typo in
  // the list would otherwise show up as a silent grid of folder icons.
  it('resolves every name the chooser offers', () => {
    const unresolved = userDirectoryIconNames.filter(
      name => name !== 'FolderIcon' && getIconComponent(name) === LucideIcons.FolderIcon,
    );
    expect(unresolved).toEqual([]);
  });
});
