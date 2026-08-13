// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { buildButtonElement, buildImageElement } from '@/modules/extensions/utils/ui-element-builders';

describe('buildButtonElement', () => {
  /**
   * The regression this exists for: the worker kept its own copy of this builder, which
   * copied only the fields it knew about. A button's icon and tooltip were dropped on the
   * way out, so it rendered as an empty square with no way to tell what it did.
   */
  it('carries every field an extension can set', () => {
    expect(buildButtonElement({
      id: 'matchOnline',
      icon: 'ScanSearch',
      tooltip: 'Match online',
      loading: true,
      variant: 'secondary',
      size: 'sm',
      disabled: true,
    })).toEqual({
      type: 'button',
      id: 'matchOnline',
      label: '',
      icon: 'ScanSearch',
      tooltip: 'Match online',
      loading: true,
      variant: 'secondary',
      size: 'sm',
      disabled: true,
    });
  });

  it('treats a missing label as empty, for an icon-only button', () => {
    expect(buildButtonElement({
      id: 'x',
      icon: 'ImageOff',
    }).label).toBe('');
  });

  it('keeps the xs default size', () => {
    expect(buildButtonElement({
      id: 'x',
      label: 'Go',
    }).size).toBe('xs');
  });
});

describe('buildImageElement', () => {
  it('maps the source onto the value the renderer reads', () => {
    expect(buildImageElement({
      id: 'cover',
      src: 'data:image/png;base64,AA',
      alt: 'Cover',
    }))
      .toEqual({
        type: 'image',
        id: 'cover',
        value: 'data:image/png;base64,AA',
        label: 'Cover',
      });
  });

  it('passes an empty source through, which renders a placeholder frame', () => {
    expect(buildImageElement({ src: '' }).value).toBe('');
  });
});

describe('the worker and the host share these builders', () => {
  /**
   * Both entry points must call the shared functions rather than re-implement them.
   * Re-implementation is what let the two drift in the first place.
   */
  const sources = [
    'src/modules/extensions/runtime/extension-worker.ts',
    'src/modules/extensions/api/create-ui-api.ts',
  ];

  for (const source of sources) {
    it(`${source} delegates rather than rebuilding the element`, () => {
      const text = readFileSync(source, 'utf8');

      expect(text).toContain('buildButtonElement');
      expect(text).toContain('buildImageElement');
      // A literal element type would mean a hand-rolled copy had come back.
      expect(text).not.toContain('type: \'button\'');
      expect(text).not.toContain('type: \'image\'');
    });
  }
});
