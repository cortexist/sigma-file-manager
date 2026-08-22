// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { describe, expect, it } from 'vitest';
import {
  findTextMatches,
  firstMatchIndexFrom,
  replaceAllMatches,
  replaceMatch,
  segmentTextByMatches,
  stepMatchIndex,
} from '../find-in-text';

describe('findTextMatches', () => {
  it('lists every occurrence in order, ignoring case by default', () => {
    expect(findTextMatches('Fox fox FOX', 'fox', { matchCase: false })).toEqual([
      {
        start: 0,
        end: 3,
      },
      {
        start: 4,
        end: 7,
      },
      {
        start: 8,
        end: 11,
      },
    ]);
  });

  it('honors the case toggle', () => {
    expect(findTextMatches('Fox fox FOX', 'fox', { matchCase: true })).toEqual([{
      start: 4,
      end: 7,
    }]);
  });

  it('treats the query as literal text, not a pattern', () => {
    expect(findTextMatches('a.c abc', 'a.c', { matchCase: false })).toEqual([{
      start: 0,
      end: 3,
    }]);
  });

  it('does not overlap matches', () => {
    expect(findTextMatches('aaaa', 'aa', { matchCase: false })).toEqual([
      {
        start: 0,
        end: 2,
      },
      {
        start: 2,
        end: 4,
      },
    ]);
  });

  it('finds nothing for an empty query', () => {
    expect(findTextMatches('anything', '', { matchCase: false })).toEqual([]);
  });

  it('stops at the collection limit', () => {
    expect(findTextMatches('a'.repeat(50), 'a', { matchCase: false }, 10)).toHaveLength(10);
  });
});

describe('firstMatchIndexFrom', () => {
  const matches = [
    {
      start: 0,
      end: 1,
    },
    {
      start: 10,
      end: 11,
    },
    {
      start: 20,
      end: 21,
    },
  ];

  it('lands on the first match at or after the position', () => {
    expect(firstMatchIndexFrom(matches, 0)).toBe(0);
    expect(firstMatchIndexFrom(matches, 5)).toBe(1);
    expect(firstMatchIndexFrom(matches, 10)).toBe(1);
  });

  it('wraps to the first match when nothing lies ahead', () => {
    expect(firstMatchIndexFrom(matches, 21)).toBe(0);
  });

  it('reports no match for an empty list', () => {
    expect(firstMatchIndexFrom([], 0)).toBe(-1);
  });
});

describe('stepMatchIndex', () => {
  it('wraps around at both ends', () => {
    expect(stepMatchIndex(2, 3, 1)).toBe(0);
    expect(stepMatchIndex(0, 3, -1)).toBe(2);
  });

  it('enters the list from either end when nothing was active', () => {
    expect(stepMatchIndex(-1, 3, 1)).toBe(0);
    expect(stepMatchIndex(-1, 3, -1)).toBe(2);
  });

  it('has nowhere to go with no matches', () => {
    expect(stepMatchIndex(0, 0, 1)).toBe(-1);
  });
});

describe('replacing', () => {
  it('replaces one match in place', () => {
    expect(replaceMatch('one two three', {
      start: 4,
      end: 7,
    }, 'deux')).toBe('one deux three');
  });

  it('replaces every match in one pass', () => {
    const text = 'fox fox fox';
    const matches = findTextMatches(text, 'fox', { matchCase: false });

    expect(replaceAllMatches(text, matches, 'cat')).toBe('cat cat cat');
  });

  it('leaves text without matches untouched', () => {
    expect(replaceAllMatches('nothing here', [], 'x')).toBe('nothing here');
  });
});

describe('segmentTextByMatches', () => {
  it('alternates plain runs and matches, keeping every character', () => {
    const text = 'ab-ab';
    const matches = findTextMatches(text, 'ab', { matchCase: false });

    expect(segmentTextByMatches(text, matches)).toEqual([
      {
        text: 'ab',
        matchIndex: 0,
      },
      {
        text: '-',
        matchIndex: null,
      },
      {
        text: 'ab',
        matchIndex: 1,
      },
    ]);
  });

  it('leaves matches past the drawing limit as plain text', () => {
    const text = 'ab ab ab';
    const matches = findTextMatches(text, 'ab', { matchCase: false });

    expect(segmentTextByMatches(text, matches, 1)).toEqual([
      {
        text: 'ab',
        matchIndex: 0,
      },
      {
        text: ' ab ab',
        matchIndex: null,
      },
    ]);
  });
});
