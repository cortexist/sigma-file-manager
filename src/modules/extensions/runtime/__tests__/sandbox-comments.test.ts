// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { describe, expect, it } from 'vitest';
import { stripComments, validateExtensionCode } from '@/modules/extensions/runtime/sandbox';

describe('stripComments', () => {
  it('blanks a line comment but keeps the code beside it', () => {
    expect(stripComments('const a = 1; // navigator').trimEnd()).toBe('const a = 1;');
  });

  it('blanks a block comment across lines', () => {
    const stripped = stripComments('a;\n/* window\n document */\nb;');

    expect(stripped).not.toContain('window');
    expect(stripped).toContain('a;');
    expect(stripped).toContain('b;');
  });

  it('keeps every offset and line stable', () => {
    const code = 'const a = 1; // navigator\nconst b = 2;';
    const stripped = stripComments(code);

    expect(stripped).toHaveLength(code.length);
    expect(stripped.split('\n')).toHaveLength(2);
  });

  it('leaves string contents alone', () => {
    expect(stripComments('const a = \'eval(\';')).toBe('const a = \'eval(\';');
  });

  /** The `//` here belongs to a URL, not to a comment. */
  it('does not treat a protocol separator inside a string as a comment', () => {
    const code = 'const url = \'https://example.com\'; navigator.userAgent;';

    expect(stripComments(code)).toContain('navigator.userAgent');
  });

  it('does not mistake a character class for a string', () => {
    const code = 'const pattern = /[\'"]/g;\nnavigator.userAgent;';

    expect(stripComments(code)).toContain('navigator.userAgent');
  });

  it('does not mistake division for a regular expression', () => {
    const code = 'const ratio = width / height; const other = total / 2;\nwindow.x;';

    expect(stripComments(code)).toContain('window.x');
  });

  it('handles an escaped quote inside a string', () => {
    const code = 'const a = \'it\\\'s fine\'; document.body;';

    expect(stripComments(code)).toContain('document.body');
  });

  it('handles a comment marker inside a template literal', () => {
    const code = 'const a = `// not a comment`; fetch(x);';

    expect(stripComments(code)).toContain('fetch(x)');
  });

  it('survives an unterminated block comment', () => {
    expect(() => stripComments('a; /* never closed')).not.toThrow();
  });
});

describe('validateExtensionCode', () => {
  /**
   * The regression this exists for: an extension whose prose mentions the navigator was
   * rejected as though it had accessed it, which silently broke its whole installation.
   */
  it('accepts prose that merely names a blocked global', () => {
    const code = [
      '/**',
      ' * The command palette supplies nothing, so it falls back to whatever the',
      ' * navigator has selected. Opens in a window, not the document body.',
      ' */',
      'export function activate() { return 1; }',
    ].join('\n');

    expect(validateExtensionCode(code)).toEqual({
      valid: true,
      errors: [],
    });
  });

  it('still rejects a real access', () => {
    const result = validateExtensionCode('export function activate() { return navigator.userAgent; }');

    expect(result.valid).toBe(false);
    expect(result.errors).toContain('Direct navigator access is not allowed');
  });

  it('still rejects an access that a comment sits beside', () => {
    const result = validateExtensionCode('// harmless note\nwindow.location = "x";');

    expect(result.valid).toBe(false);
  });

  it('is not fooled by an access hidden after a string containing a comment marker', () => {
    const result = validateExtensionCode('const a = \'x//y\'; eval(\'1\');');

    expect(result.valid).toBe(false);
    expect(result.errors).toContain('eval() is not allowed');
  });

  it('accepts ordinary extension code', () => {
    const code = [
      'import { openTagEditor } from \'./editor-modal.js\';',
      'export async function activate() {',
      '  const ratio = 16 / 9;',
      '  await sigma.i18n.mergeFromPath(\'locales\');',
      '  return ratio;',
      '}',
    ].join('\n');

    expect(validateExtensionCode(code)).toEqual({
      valid: true,
      errors: [],
    });
  });
});
