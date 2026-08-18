// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import {
  afterEach, describe, expect, it, vi,
} from 'vitest';
import { resolveViewportContentWidth } from '../use-file-browser-virtual-layout';

function createViewport(options: {
  viewportWidth: number;
  entriesContainerWidth?: number;
}): HTMLElement {
  const viewport = document.createElement('div');

  Object.defineProperty(viewport, 'clientWidth', {
    value: options.viewportWidth,
    configurable: true,
  });

  if (options.entriesContainerWidth !== undefined) {
    const entriesContainer = document.createElement('div');
    entriesContainer.className = 'file-browser__entries-container';
    Object.defineProperty(entriesContainer, 'clientWidth', {
      value: options.entriesContainerWidth,
      configurable: true,
    });
    viewport.append(entriesContainer);
  }

  return viewport;
}

describe('resolveViewportContentWidth', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  function stubPadding() {
    vi.spyOn(window, 'getComputedStyle').mockReturnValue({
      paddingLeft: '0px',
      paddingRight: '0px',
    } as CSSStyleDeclaration);
  }

  it('measures the entries container when it fits the viewport', () => {
    stubPadding();

    expect(resolveViewportContentWidth(createViewport({
      viewportWidth: 900,
      entriesContainerWidth: 860,
    }))).toBe(860);
  });

  it('never reports more room than the viewport has', () => {
    stubPadding();

    // Something inside the grid — a long directory heading, say — has pushed the container
    // wider than its viewport. Believing that width picks a column count that does not fit,
    // which relays the grid out, which changes the width again: the list flickers.
    expect(resolveViewportContentWidth(createViewport({
      viewportWidth: 500,
      entriesContainerWidth: 720,
    }))).toBe(500);
  });

  it('falls back to the viewport when there is no entries container', () => {
    stubPadding();

    expect(resolveViewportContentWidth(createViewport({ viewportWidth: 640 }))).toBe(640);
  });
});
