// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

/**
 * The handful of markdown formats a quick editor offers. Each one is a small, reversible
 * edit around the selection — enough to tidy a note, deliberately short of an editor.
 */
export type MarkdownFormat = 'bold' | 'italic' | 'heading' | 'link' | 'code' | 'list';

/** A replacement of one range of the text, and where the selection lands afterwards. */
export interface TextEdit {
  start: number;
  end: number;
  replacement: string;
  /** Absolute offsets in the text as it reads after the edit. */
  selectionStart: number;
  selectionEnd: number;
}

function lineStartAt(text: string, offset: number): number {
  return text.lastIndexOf('\n', Math.max(0, offset - 1)) + 1;
}

function lineEndAt(text: string, offset: number): number {
  const newline = text.indexOf('\n', offset);

  return newline === -1 ? text.length : newline;
}

/**
 * Wraps the selection in a marker, or unwraps it when it is already wrapped — whether the
 * markers sit inside the selection or just outside it, since people select either way.
 */
function toggleWrap(text: string, start: number, end: number, marker: string): TextEdit {
  const selected = text.slice(start, end);
  const markerLength = marker.length;

  if (
    text.slice(start - markerLength, start) === marker
    && text.slice(end, end + markerLength) === marker
  ) {
    return {
      start: start - markerLength,
      end: end + markerLength,
      replacement: selected,
      selectionStart: start - markerLength,
      selectionEnd: start - markerLength + selected.length,
    };
  }

  if (
    selected.length >= markerLength * 2
    && selected.startsWith(marker)
    && selected.endsWith(marker)
  ) {
    const inner = selected.slice(markerLength, selected.length - markerLength);

    return {
      start,
      end,
      replacement: inner,
      selectionStart: start,
      selectionEnd: start + inner.length,
    };
  }

  return {
    start,
    end,
    replacement: `${marker}${selected}${marker}`,
    selectionStart: start + markerLength,
    selectionEnd: start + markerLength + selected.length,
  };
}

/** Cycles the heading level of the line the selection starts on: none, #, ##, ###, none. */
function cycleHeading(text: string, start: number, end: number): TextEdit {
  const lineStart = lineStartAt(text, start);
  const lineEnd = lineEndAt(text, start);
  const line = text.slice(lineStart, lineEnd);
  const existing = /^(#{1,6})[ \t]+/.exec(line);
  const level = existing === null ? 0 : existing[1].length;
  const body = existing === null ? line : line.slice(existing[0].length);
  const nextLevel = level >= 3 ? 0 : level + 1;
  const replacement = nextLevel === 0 ? body : `${'#'.repeat(nextLevel)} ${body}`;
  const delta = replacement.length - line.length;

  return {
    start: lineStart,
    end: lineEnd,
    replacement,
    selectionStart: Math.max(lineStart, start + delta),
    selectionEnd: Math.max(lineStart, end + delta),
  };
}

/**
 * Toggles a bullet on every line the selection touches: all bulleted means remove them,
 * anything else means add the missing ones. Blank lines are left blank.
 */
function toggleList(text: string, start: number, end: number): TextEdit {
  const blockStart = lineStartAt(text, start);
  const blockEnd = lineEndAt(text, end);
  const lines = text.slice(blockStart, blockEnd).split('\n');
  const bullet = /^(\s*)- /;
  const contentLines = lines.filter(line => line.trim().length > 0);
  const allBulleted = contentLines.length > 0 && contentLines.every(line => bullet.test(line));

  const replacement = lines
    .map((line) => {
      if (line.trim().length === 0) {
        return line;
      }

      if (allBulleted) {
        return line.replace(bullet, '$1');
      }

      return bullet.test(line) ? line : `- ${line}`;
    })
    .join('\n');

  return {
    start: blockStart,
    end: blockEnd,
    replacement,
    selectionStart: blockStart,
    selectionEnd: blockStart + replacement.length,
  };
}

function linkSelection(text: string, start: number, end: number): TextEdit {
  const selected = text.slice(start, end);

  if (selected.length === 0) {
    return {
      start,
      end,
      replacement: '[text](url)',
      selectionStart: start + 1,
      selectionEnd: start + 5,
    };
  }

  const replacement = `[${selected}](url)`;
  const urlStart = start + selected.length + 3;

  return {
    start,
    end,
    replacement,
    selectionStart: urlStart,
    selectionEnd: urlStart + 3,
  };
}

function codeSelection(text: string, start: number, end: number): TextEdit {
  const selected = text.slice(start, end);

  if (!selected.includes('\n')) {
    return toggleWrap(text, start, end, '`');
  }

  const fence = '```';
  const replacement = `${fence}\n${selected}\n${fence}`;

  return {
    start,
    end,
    replacement,
    selectionStart: start + fence.length + 1,
    selectionEnd: start + fence.length + 1 + selected.length,
  };
}

export function formatMarkdown(
  text: string,
  selectionStart: number,
  selectionEnd: number,
  format: MarkdownFormat,
): TextEdit {
  const start = Math.min(selectionStart, selectionEnd);
  const end = Math.max(selectionStart, selectionEnd);

  switch (format) {
    case 'bold':
      return toggleWrap(text, start, end, '**');
    case 'italic':
      return toggleWrap(text, start, end, '*');
    case 'heading':
      return cycleHeading(text, start, end);
    case 'link':
      return linkSelection(text, start, end);
    case 'code':
      return codeSelection(text, start, end);
    case 'list':
      return toggleList(text, start, end);
  }
}

/** Applies an edit to a string, for tests and for callers without a textarea. */
export function applyTextEdit(text: string, edit: TextEdit): string {
  return `${text.slice(0, edit.start)}${edit.replacement}${text.slice(edit.end)}`;
}
