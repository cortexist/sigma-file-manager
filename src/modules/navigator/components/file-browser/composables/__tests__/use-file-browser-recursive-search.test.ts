// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { describe, expect, it } from 'vitest';
import { getRecursiveSearchNameQuery } from '../use-file-browser-recursive-search';

describe('recursive search name query', () => {
  it('passes a bare query on as the name filter', () => {
    expect(getRecursiveSearchNameQuery('  report ')).toBe('report');
  });

  it('passes the value of an explicit name query', () => {
    expect(getRecursiveSearchNameQuery('name: report')).toBe('report');
  });

  it('filters nothing for a query the walk cannot answer from names', () => {
    // The walk reads names; sizes and dates are decided once the entries are back, so a
    // name filter here would silently drop entries the query might still match.
    expect(getRecursiveSearchNameQuery('size: >=2mb')).toBe('');
    expect(getRecursiveSearchNameQuery('modified: 2024')).toBe('');
    expect(getRecursiveSearchNameQuery('path: pics')).toBe('');
  });
});
