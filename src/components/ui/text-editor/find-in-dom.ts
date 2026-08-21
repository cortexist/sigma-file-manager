// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { findTextMatches, type FindOptions } from './find-in-text';

const FIND_HIGHLIGHT_NAME = 'text-editor-find';
const FIND_ACTIVE_HIGHLIGHT_NAME = 'text-editor-find-active';

/**
 * Ceiling on matches that get drawn. The count and the navigation run over the whole list;
 * only the painting stops here, because thousands of marks cost real layout time and past
 * the first screenful they are noise anyway.
 */
export const MAX_HIGHLIGHTED_MATCHES = 5000;

interface TextNodeIndex {
  nodes: Text[];
  /** Offset of each node's first character in the stitched `text`. */
  starts: number[];
  text: string;
}

/**
 * Stitches the text nodes under `root` into one string, remembering where each began. The
 * rendered view has no single string to search: what the reader sees as one sentence is spread
 * over several nodes once any of it is bold or linked, so a match can start in one node and
 * end in another.
 */
function indexTextNodes(root: Element): TextNodeIndex {
  const walker = root.ownerDocument.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const nodes: Text[] = [];
  const starts: number[] = [];
  let text = '';

  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const textNode = node as Text;

    if (textNode.data.length === 0) {
      continue;
    }

    nodes.push(textNode);
    starts.push(text.length);
    text += textNode.data;
  }

  return {
    nodes,
    starts,
    text,
  };
}

/**
 * The node an offset falls in. An offset on a boundary belongs to the node starting there,
 * except as a match's end, where it belongs to the node ending there — a range that ended at
 * offset 0 of the next node would be right by arithmetic and span a node it has nothing in.
 */
function locateOffset(index: TextNodeIndex, offset: number, isEnd: boolean): {
  node: Text;
  offset: number;
} {
  let low = 0;
  let high = index.starts.length - 1;

  while (low < high) {
    const middle = (low + high + 1) >> 1;
    const startsBefore = isEnd ? index.starts[middle] < offset : index.starts[middle] <= offset;

    if (startsBefore) {
      low = middle;
    }
    else {
      high = middle - 1;
    }
  }

  return {
    node: index.nodes[low],
    offset: offset - index.starts[low],
  };
}

/** Every occurrence of `query` in the text under `root`, each as a DOM range over the nodes it spans. */
export function collectDomTextMatches(root: Element, query: string, options: FindOptions): Range[] {
  const index = indexTextNodes(root);

  if (index.nodes.length === 0) {
    return [];
  }

  return findTextMatches(index.text, query, options).map((match) => {
    const range = root.ownerDocument.createRange();
    const start = locateOffset(index, match.start, false);
    const end = locateOffset(index, match.end, true);

    range.setStart(start.node, start.offset);
    range.setEnd(end.node, end.offset);

    return range;
  });
}

/**
 * The CSS Custom Highlight API paints ranges without touching the markup, which is what makes
 * it usable over rendered markdown: wrapping matches in elements would break the very nodes
 * the ranges point into, and re-rendering to drop them again would lose the scroll position.
 */
export function supportsFindHighlights(): boolean {
  return typeof CSS !== 'undefined' && 'highlights' in CSS && typeof Highlight === 'function';
}

/** Paints the matches, the active one in its own color. Anything past the drawing cap stays unpainted. */
export function applyFindHighlights(ranges: readonly Range[], activeIndex: number): void {
  if (!supportsFindHighlights()) {
    return;
  }

  const others = new Highlight();
  const drawn = Math.min(ranges.length, MAX_HIGHLIGHTED_MATCHES);

  for (let index = 0; index < drawn; index += 1) {
    if (index !== activeIndex) {
      others.add(ranges[index]);
    }
  }

  CSS.highlights.set(FIND_HIGHLIGHT_NAME, others);

  const active = ranges[activeIndex];

  if (active) {
    CSS.highlights.set(FIND_ACTIVE_HIGHLIGHT_NAME, new Highlight(active));
  }
  else {
    CSS.highlights.delete(FIND_ACTIVE_HIGHLIGHT_NAME);
  }
}

export function clearFindHighlights(): void {
  if (!supportsFindHighlights()) {
    return;
  }

  CSS.highlights.delete(FIND_HIGHLIGHT_NAME);
  CSS.highlights.delete(FIND_ACTIVE_HIGHLIGHT_NAME);
}

/** The nearest ancestor that actually scrolls vertically, or null when nothing above does. */
export function getScrollParent(element: Element | null): HTMLElement | null {
  for (let node = element?.parentElement ?? null; node; node = node.parentElement) {
    const { overflowY } = getComputedStyle(node);

    if ((overflowY === 'auto' || overflowY === 'scroll') && node.scrollHeight > node.clientHeight) {
      return node;
    }
  }

  return null;
}

/**
 * Brings a match to the middle of whatever scrolls it, and only when it is out of view: a
 * match the reader can already see should stay put rather than jump to the center.
 */
export function revealInScrollParent(target: Element | Range): void {
  const anchor = target instanceof Range ? target.startContainer.parentElement : target;
  const scroller = getScrollParent(anchor);

  if (!scroller) {
    return;
  }

  const rect = target.getBoundingClientRect();
  const box = scroller.getBoundingClientRect();

  if (rect.top >= box.top && rect.bottom <= box.bottom) {
    return;
  }

  scroller.scrollTop += rect.top - box.top - (scroller.clientHeight - rect.height) / 2;
}
