// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { describe, expect, it } from 'vitest';
import { canUseDefaultFileManager } from '@/stores/runtime/platform';

describe('platform capabilities', () => {
  it('allows the default file manager integration for direct Windows installations', () => {
    expect(canUseDefaultFileManager('windows', true)).toBe(true);
  });

  it('disables the default file manager integration when the backend reports it unavailable', () => {
    expect(canUseDefaultFileManager('windows', false)).toBe(false);
  });

  it('allows it on Linux, where the backend sets the XDG association', () => {
    expect(canUseDefaultFileManager('linux', true)).toBe(true);
  });

  it('disables it on Linux when xdg-utils is missing, which the backend reports', () => {
    expect(canUseDefaultFileManager('linux', false)).toBe(false);
  });

  it('disables it on macOS, where the Finder cannot be replaced', () => {
    expect(canUseDefaultFileManager('macos', true)).toBe(false);
  });
});
