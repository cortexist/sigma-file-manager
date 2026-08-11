// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { describe, expect, it } from 'vitest';
import { fileContentVersion, withContentVersion } from '@/utils/file-content-version';

describe('fileContentVersion', () => {
  it('changes when the file is written to, and not otherwise', () => {
    const before = fileContentVersion({
      modified_time: 1000,
      size: 20,
    });

    expect(fileContentVersion({
      modified_time: 1000,
      size: 20,
    })).toBe(before);
    expect(fileContentVersion({
      modified_time: 2000,
      size: 20,
    })).not.toBe(before);

    // Rewritten within the same clock tick: the size is what separates them.
    expect(fileContentVersion({
      modified_time: 1000,
      size: 21,
    })).not.toBe(before);
  });

  it('has no version for nothing selected', () => {
    expect(fileContentVersion(null)).toBeNull();
    expect(fileContentVersion(undefined)).toBeNull();
  });
});

describe('withContentVersion', () => {
  /** The asset protocol: a bare URL, so the version opens the query. */
  it('appends to a URL that has no query', () => {
    expect(withContentVersion('asset://localhost/home/a.png', '1000-20'))
      .toBe('asset://localhost/home/a.png?v=1000-20');
  });

  /**
   * The media server carries the file in `?path=`, so a second `?` would make the version part
   * of the path value and the server would look for a file that does not exist.
   */
  it('extends a URL that already has a query', () => {
    expect(withContentVersion('http://127.0.0.1:1234/token?path=%2Fhome%2Fa.mp4', '1000-20'))
      .toBe('http://127.0.0.1:1234/token?path=%2Fhome%2Fa.mp4&v=1000-20');
  });

  it('leaves the URL alone when there is no version to add', () => {
    expect(withContentVersion('asset://localhost/home/a.png', null)).toBe('asset://localhost/home/a.png');
    expect(withContentVersion('', '1000-20')).toBe('');
  });
});
