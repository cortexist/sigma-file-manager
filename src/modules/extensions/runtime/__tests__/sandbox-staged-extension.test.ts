// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { describe, expect, it } from 'vitest';
import { validateExtensionCode } from '@/modules/extensions/runtime/sandbox';

/**
 * Shapes taken from real extension code that the validator used to reject. Each one is a
 * false positive it produced by reading prose and string contents as though they were
 * accesses, which broke the extension's installation with a security error naming
 * something the code never touched.
 */
const PREVIOUSLY_REJECTED_SOURCES: Array<{
  label: string;
  code: string;
}> = [
  {
    label: 'a doc comment mentioning the navigator',
    code: [
      '/**',
      ' * Resolves the file to edit. The context menu supplies a selection; the command',
      ' * palette supplies nothing, so it falls back to whatever the navigator has selected.',
      ' */',
      'export function resolveTargetPath(paths) { return paths[0] ?? null; }',
    ].join('\n'),
  },
  {
    label: 'a comment mentioning a window',
    code: '// Opens in its own window rather than the document body.\nexport const a = 1;',
  },
  {
    label: 'a comment mentioning fetch',
    code: '// The host will fetch this on the extension\'s behalf.\nexport const a = 1;',
  },
  {
    label: 'a comment beside a regular expression with a quote class',
    code: 'const quotes = /[\'"]/g; // strips the navigator\'s smart quotes\nexport { quotes };',
  },
];

describe('validator false positives that broke real extensions', () => {
  for (const { label, code } of PREVIOUSLY_REJECTED_SOURCES) {
    it(`accepts ${label}`, () => {
      expect(validateExtensionCode(code)).toEqual({
        valid: true,
        errors: [],
      });
    });
  }
});
