// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { describe, expect, it } from 'vitest';
import { applyTextEdit, formatMarkdown, type MarkdownFormat } from '../markdown-format';

function format(text: string, selected: string, kind: MarkdownFormat) {
  const start = text.indexOf(selected);
  const end = start + selected.length;
  const edit = formatMarkdown(text, start, end, kind);
  const result = applyTextEdit(text, edit);

  return {
    result,
    selection: result.slice(edit.selectionStart, edit.selectionEnd),
  };
}

describe('formatMarkdown', () => {
  it('wraps a selection in bold and keeps it selected', () => {
    expect(format('make this strong', 'this', 'bold'))
      .toEqual({
        result: 'make **this** strong',
        selection: 'this',
      });
  });

  it('unwraps bold whether the markers were selected or not', () => {
    expect(format('make **this** strong', 'this', 'bold').result).toBe('make this strong');
    expect(format('make **this** strong', '**this**', 'bold').result).toBe('make this strong');
  });

  it('puts the caret between the markers when nothing is selected', () => {
    const edit = formatMarkdown('ab', 1, 1, 'italic');

    expect(applyTextEdit('ab', edit)).toBe('a**b');
    expect(edit.selectionStart).toBe(2);
    expect(edit.selectionEnd).toBe(2);
  });

  it('cycles the heading level of the current line and keeps the caret on its word', () => {
    const one = formatMarkdown('intro\ntitle here\nmore', 8, 8, 'heading');
    const afterOne = applyTextEdit('intro\ntitle here\nmore', one);

    expect(afterOne).toBe('intro\n# title here\nmore');
    expect(afterOne.slice(one.selectionStart)).toMatch(/^tle here/);

    const two = applyTextEdit(afterOne, formatMarkdown(afterOne, 8, 8, 'heading'));
    const three = applyTextEdit(two, formatMarkdown(two, 8, 8, 'heading'));
    const none = applyTextEdit(three, formatMarkdown(three, 8, 8, 'heading'));

    expect(two).toBe('intro\n## title here\nmore');
    expect(three).toBe('intro\n### title here\nmore');
    expect(none).toBe('intro\ntitle here\nmore');
  });

  it('bullets every selected line, and removes the bullets when they all have one', () => {
    const listed = format('one\ntwo\n\nthree', 'one\ntwo\n\nthree', 'list');

    expect(listed.result).toBe('- one\n- two\n\n- three');
    expect(listed.selection).toBe('- one\n- two\n\n- three');

    expect(format(listed.result, listed.result, 'list').result).toBe('one\ntwo\n\nthree');
  });

  it('only adds the bullets that are missing', () => {
    expect(format('- one\ntwo', '- one\ntwo', 'list').result).toBe('- one\n- two');
  });

  it('links the selection and selects the url placeholder', () => {
    expect(format('see the docs now', 'the docs', 'link'))
      .toEqual({
        result: 'see [the docs](url) now',
        selection: 'url',
      });
  });

  it('offers a link skeleton with the text selected when nothing is selected', () => {
    const edit = formatMarkdown('', 0, 0, 'link');

    expect(applyTextEdit('', edit)).toBe('[text](url)');
    expect('[text](url)'.slice(edit.selectionStart, edit.selectionEnd)).toBe('text');
  });

  it('uses inline code for one line and a fence for several', () => {
    expect(format('run ls now', 'ls', 'code').result).toBe('run `ls` now');

    const fenced = format('a\nb', 'a\nb', 'code');

    expect(fenced.result).toBe('```\na\nb\n```');
    expect(fenced.selection).toBe('a\nb');
  });

  it('accepts a backwards selection', () => {
    const edit = formatMarkdown('bold me', 4, 0, 'bold');

    expect(applyTextEdit('bold me', edit)).toBe('**bold** me');
  });
});
