// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import {
  computed,
  nextTick,
  onBeforeUnmount,
  ref,
  watch,
  type ComponentPublicInstance,
  type ComputedRef,
  type Ref,
} from 'vue';

const DEFAULT_OVERSCAN_PX = 256;

export interface VerticalVirtualPositionedItem {
  size: number;
  start: number;
}

export interface VerticalVirtualItem<Item> extends VerticalVirtualPositionedItem {
  index: number;
  item: Item;
}

type VerticalVirtualViewportReference
  = | Element
    | (ComponentPublicInstance & {
      viewportElement?: HTMLElement | Ref<HTMLElement | null>;
    })
    | null;

export function createVerticalVirtualItems<Item>(
  items: readonly Item[],
  getItemSize: (item: Item) => number,
): VerticalVirtualItem<Item>[] {
  let start = 0;

  return items.map((item, index) => {
    const size = Math.max(0, getItemSize(item));
    const virtualItem = {
      index,
      item,
      size,
      start,
    };

    start += size;
    return virtualItem;
  });
}

/** One group of items laid out together, optionally under a header row of its own. */
export interface SectionedVirtualSection<SectionKey extends string, Item> {
  key: SectionKey;
  items: readonly Item[];
  /** Height of one row of items, before the gap. */
  itemHeight: number;
  /** Items per row; 1 lays the section out as a plain list. */
  columnCount: number;
}

export interface SectionedVirtualRowsOptions {
  /** Height of a section header, before the gap. Zero omits header rows entirely. */
  headerHeight: number;
  /** Added to every row's height — the gutter below it. */
  gap?: number;
}

export type SectionedVirtualRow<SectionKey extends string, Item>
  = | (VerticalVirtualPositionedItem & {
    type: 'section';
    key: SectionKey;
    /** How many items the section holds, for a count badge. */
    count: number;
    /** Position among the sections that actually rendered. */
    sectionIndex: number;
  })
  | (VerticalVirtualPositionedItem & {
    type: 'items';
    key: SectionKey;
    items: Item[];
    rowIndex: number;
  });

/**
 * Lays sections of items out as positioned rows: a header, then the items chunked into rows
 * of `columnCount`, per section, accumulating offsets as it goes. Empty sections contribute
 * nothing, header and all.
 *
 * Both browsing surfaces build their rows from this — the navigator's grid sections and the
 * file dialog's folders/files sections — so the offset arithmetic that a virtual list depends
 * on being exactly right exists once. Callers map the result onto their own row types; what
 * is shared is the geometry, not the presentation.
 */
export function buildSectionedVirtualRows<SectionKey extends string, Item>(
  sections: readonly SectionedVirtualSection<SectionKey, Item>[],
  options: SectionedVirtualRowsOptions,
): SectionedVirtualRow<SectionKey, Item>[] {
  const gap = options.gap ?? 0;
  const rows: SectionedVirtualRow<SectionKey, Item>[] = [];
  let start = 0;
  let sectionIndex = 0;

  for (const section of sections) {
    if (section.items.length === 0) {
      continue;
    }

    if (options.headerHeight > 0) {
      const size = options.headerHeight + gap;

      rows.push({
        type: 'section',
        key: section.key,
        count: section.items.length,
        sectionIndex,
        start,
        size,
      });

      start += size;
    }

    const columnCount = Math.max(1, Math.floor(section.columnCount));
    const size = section.itemHeight + gap;

    for (let itemIndex = 0, rowIndex = 0; itemIndex < section.items.length; itemIndex += columnCount, rowIndex += 1) {
      rows.push({
        type: 'items',
        key: section.key,
        items: section.items.slice(itemIndex, itemIndex + columnCount),
        rowIndex,
        start,
        size,
      });

      start += size;
    }

    sectionIndex += 1;
  }

  return rows;
}

export function computeVerticalVirtualRange<PositionedItem extends VerticalVirtualPositionedItem>(options: {
  items: readonly PositionedItem[];
  overscanPx: number;
  scrollTop: number;
  viewportHeight: number;
}): {
  start: number;
  end: number;
} {
  const items = options.items;

  if (items.length === 0) {
    return {
      start: 0,
      end: 0,
    };
  }

  const viewportHeight = Math.max(0, options.viewportHeight);
  const lastItem = items[items.length - 1];
  const totalHeight = lastItem.start + lastItem.size;
  const maxScrollTop = Math.max(0, totalHeight - viewportHeight);
  const effectiveScrollTop = Math.min(Math.max(0, options.scrollTop), maxScrollTop);
  const viewportStart = Math.max(0, effectiveScrollTop - options.overscanPx);
  const viewportEnd = effectiveScrollTop + viewportHeight + options.overscanPx;
  let startSearchIndex = 0;
  let startSearchEnd = items.length;

  while (startSearchIndex < startSearchEnd) {
    const middleIndex = Math.floor((startSearchIndex + startSearchEnd) / 2);
    const item = items[middleIndex];

    if (item.start + item.size <= viewportStart) {
      startSearchIndex = middleIndex + 1;
    }
    else {
      startSearchEnd = middleIndex;
    }
  }

  const rangeStart = startSearchIndex;
  let endSearchIndex = rangeStart;
  let endSearchEnd = items.length;

  while (endSearchIndex < endSearchEnd) {
    const middleIndex = Math.floor((endSearchIndex + endSearchEnd) / 2);

    if (items[middleIndex].start < viewportEnd) {
      endSearchIndex = middleIndex + 1;
    }
    else {
      endSearchEnd = middleIndex;
    }
  }

  return {
    start: rangeStart,
    end: Math.max(rangeStart, endSearchIndex),
  };
}

function resolveElement(
  element: VerticalVirtualViewportReference,
): HTMLElement | null {
  if (element instanceof HTMLElement) {
    return element;
  }

  if (element && 'viewportElement' in element) {
    const exposedViewport = element.viewportElement;

    if (exposedViewport instanceof HTMLElement) {
      return exposedViewport;
    }

    if (exposedViewport?.value instanceof HTMLElement) {
      return exposedViewport.value;
    }
  }

  if (element && '$el' in element && element.$el instanceof HTMLElement) {
    return element.$el;
  }

  return null;
}

export function useVerticalVirtualList<Item>(options: {
  items: ComputedRef<readonly Item[]>;
  getItemSize: (item: Item) => number;
  overscanPx?: number;
}) {
  const scrollViewportRef = ref<HTMLElement | null>(null);
  const scrollTop = ref(0);
  const viewportHeight = ref(0);
  const overscanPx = options.overscanPx ?? DEFAULT_OVERSCAN_PX;
  let resizeObserver: ResizeObserver | null = null;

  const virtualItems = computed(() =>
    createVerticalVirtualItems(options.items.value, options.getItemSize));

  const totalHeightPx = computed(() => {
    const lastItem = virtualItems.value[virtualItems.value.length - 1];
    return lastItem ? lastItem.start + lastItem.size : 0;
  });

  const visibleRange = computed(() =>
    computeVerticalVirtualRange({
      items: virtualItems.value,
      overscanPx,
      scrollTop: scrollTop.value,
      viewportHeight: viewportHeight.value,
    }));

  const visibleItems = computed(() =>
    virtualItems.value.slice(visibleRange.value.start, visibleRange.value.end));

  const windowOffsetPx = computed(() => visibleItems.value[0]?.start ?? 0);

  const spacerStyle = computed(() => ({
    height: `${totalHeightPx.value}px`,
  }));

  const windowStyle = computed(() => ({
    transform: `translate3d(0, ${windowOffsetPx.value}px, 0)`,
  }));

  function syncViewportMetrics(viewport: HTMLElement) {
    viewportHeight.value = viewport.clientHeight;
    scrollTop.value = viewport.scrollTop;
  }

  function disconnectResizeObserver() {
    resizeObserver?.disconnect();
    resizeObserver = null;
  }

  function setScrollViewportRef(element: VerticalVirtualViewportReference) {
    const viewport = resolveElement(element);

    if (scrollViewportRef.value === viewport) {
      return;
    }

    disconnectResizeObserver();
    scrollViewportRef.value = viewport;

    if (!viewport) {
      viewportHeight.value = 0;
      scrollTop.value = 0;
      return;
    }

    syncViewportMetrics(viewport);

    if (typeof ResizeObserver !== 'undefined') {
      resizeObserver = new ResizeObserver(() => syncViewportMetrics(viewport));
      resizeObserver.observe(viewport);
    }
  }

  function handleScroll(event: Event) {
    if (event.currentTarget instanceof HTMLElement) {
      scrollTop.value = event.currentTarget.scrollTop;
    }
  }

  function setScrollTop(nextScrollTop: number) {
    const viewport = scrollViewportRef.value;

    if (!viewport) {
      return;
    }

    const maxScrollTop = Math.max(0, totalHeightPx.value - viewportHeight.value);
    const normalizedScrollTop = Math.min(Math.max(0, nextScrollTop), maxScrollTop);
    viewport.scrollTop = normalizedScrollTop;
    scrollTop.value = normalizedScrollTop;
  }

  function scrollItemIntoView(
    index: number,
    align: ScrollLogicalPosition = 'nearest',
  ): boolean {
    const viewport = scrollViewportRef.value;
    const item = virtualItems.value[index];

    if (!viewport || !item) {
      return false;
    }

    const itemEnd = item.start + item.size;
    const currentViewportHeight = viewportHeight.value;
    const currentScrollTop = viewport.scrollTop;

    if (align === 'center') {
      setScrollTop(item.start - (currentViewportHeight - item.size) / 2);
    }
    else if (align === 'start') {
      setScrollTop(item.start);
    }
    else if (align === 'end') {
      setScrollTop(itemEnd - currentViewportHeight);
    }
    else if (item.start < currentScrollTop) {
      setScrollTop(item.start);
    }
    else if (itemEnd > currentScrollTop + currentViewportHeight) {
      setScrollTop(itemEnd - currentViewportHeight);
    }

    return true;
  }

  watch(options.items, () => {
    nextTick(() => {
      const viewport = scrollViewportRef.value;

      if (!viewport) {
        return;
      }

      setScrollTop(viewport.scrollTop);
      syncViewportMetrics(viewport);
    });
  });

  onBeforeUnmount(() => {
    disconnectResizeObserver();
  });

  return {
    scrollViewportRef,
    scrollTop,
    viewportHeight,
    virtualItems,
    visibleItems,
    totalHeightPx,
    spacerStyle,
    windowStyle,
    setScrollViewportRef,
    handleScroll,
    setScrollTop,
    scrollItemIntoView,
  };
}
