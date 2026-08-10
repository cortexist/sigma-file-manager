// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { describe, expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
import {
  computed,
  defineComponent,
  h,
  ref,
  type ComponentPublicInstance,
  type Ref,
} from 'vue';
import {
  buildSectionedVirtualRows,
  computeVerticalVirtualRange,
  createVerticalVirtualItems,
  useVerticalVirtualList,
} from '@/composables/use-vertical-virtual-list';

type NumberVerticalVirtualList = ReturnType<typeof useVerticalVirtualList<number>>;

function createViewport(clientHeight: number): HTMLElement {
  const viewport = document.createElement('div');
  Object.defineProperty(viewport, 'clientHeight', {
    configurable: true,
    value: clientHeight,
  });
  return viewport;
}

function mountNumberVirtualList(viewport: HTMLElement): {
  virtualList: NumberVerticalVirtualList;
  wrapper: ReturnType<typeof mount>;
} {
  let virtualList: NumberVerticalVirtualList | null = null;
  const wrapper = mount(defineComponent({
    setup() {
      const items = computed(() =>
        Array.from({ length: 100 }, (_, index) => index));
      virtualList = useVerticalVirtualList({
        items,
        getItemSize: () => 32,
        overscanPx: 0,
      });
      virtualList.setScrollViewportRef(viewport);

      return () => h('div');
    },
  }));

  if (!virtualList) {
    throw new Error('Expected virtual list to mount');
  }

  return {
    virtualList,
    wrapper,
  };
}

describe('createVerticalVirtualItems', () => {
  it('positions variable-height items consecutively', () => {
    const items = createVerticalVirtualItems(
      [
        {
          id: 'heading',
          height: 28,
        },
        {
          id: 'first',
          height: 32,
        },
        {
          id: 'second',
          height: 32,
        },
      ],
      item => item.height,
    );

    expect(items).toEqual([
      {
        index: 0,
        item: {
          id: 'heading',
          height: 28,
        },
        size: 28,
        start: 0,
      },
      {
        index: 1,
        item: {
          id: 'first',
          height: 32,
        },
        size: 32,
        start: 28,
      },
      {
        index: 2,
        item: {
          id: 'second',
          height: 32,
        },
        size: 32,
        start: 60,
      },
    ]);
  });
});

describe('buildSectionedVirtualRows', () => {
  const sections = [
    {
      key: 'dirs' as const,
      items: ['a', 'b', 'c'],
      itemHeight: 52,
      columnCount: 2,
    },
    {
      key: 'files' as const,
      items: ['d'],
      itemHeight: 120,
      columnCount: 2,
    },
  ];

  it('chunks each section into rows and accumulates offsets across sections', () => {
    const rows = buildSectionedVirtualRows(sections, {
      headerHeight: 40,
      gap: 10,
    });

    expect(rows.map(row => row.type)).toEqual(['section', 'items', 'items', 'section', 'items']);
    // header 50, two dir rows of 62, header 50, one file row of 130.
    expect(rows.map(row => row.size)).toEqual([50, 62, 62, 50, 130]);
    expect(rows.map(row => row.start)).toEqual([0, 50, 112, 174, 224]);
  });

  it('splits items across columns and leaves the last row short', () => {
    const rows = buildSectionedVirtualRows(sections, {
      headerHeight: 40,
      gap: 10,
    });
    const dirRows = rows.filter(row => row.type === 'items' && row.key === 'dirs');

    expect(dirRows.map(row => (row.type === 'items' ? row.items : []))).toEqual([['a', 'b'], ['c']]);
  });

  /** The file dialog's list view: one column, no gutter. */
  it('lays a single column out as a plain list', () => {
    const rows = buildSectionedVirtualRows(
      [{
        key: 'files' as const,
        items: ['a', 'b', 'c'],
        itemHeight: 32,
        columnCount: 1,
      }],
      { headerHeight: 40 },
    );

    expect(rows.map(row => row.size)).toEqual([40, 32, 32, 32]);
    expect(rows.map(row => row.start)).toEqual([0, 40, 72, 104]);
  });

  it('omits an empty section entirely, header included', () => {
    const rows = buildSectionedVirtualRows(
      [
        {
          key: 'dirs' as const,
          items: [],
          itemHeight: 52,
          columnCount: 2,
        },
        {
          key: 'files' as const,
          items: ['a'],
          itemHeight: 32,
          columnCount: 2,
        },
      ],
      { headerHeight: 40 },
    );

    expect(rows.map(row => row.key)).toEqual(['files', 'files']);
    expect(rows[0].start).toBe(0);
  });

  it('drops header rows when no header height is given', () => {
    const rows = buildSectionedVirtualRows(
      [{
        key: 'files' as const,
        items: ['a', 'b'],
        itemHeight: 32,
        columnCount: 1,
      }],
      { headerHeight: 0 },
    );

    expect(rows.every(row => row.type === 'items')).toBe(true);
    expect(rows.map(row => row.start)).toEqual([0, 32]);
  });
});

describe('computeVerticalVirtualRange', () => {
  const items = createVerticalVirtualItems(
    Array.from({ length: 100 }, (_, index) => index),
    () => 32,
  );

  it('returns only rows near the viewport', () => {
    expect(computeVerticalVirtualRange({
      items,
      overscanPx: 0,
      scrollTop: 320,
      viewportHeight: 96,
    })).toEqual({
      start: 10,
      end: 13,
    });
  });

  it('includes pixel overscan on both sides', () => {
    expect(computeVerticalVirtualRange({
      items,
      overscanPx: 64,
      scrollTop: 320,
      viewportHeight: 96,
    })).toEqual({
      start: 8,
      end: 15,
    });
  });

  it('clamps stale scroll positions after the list shrinks', () => {
    const shortItems = items.slice(0, 3);

    expect(computeVerticalVirtualRange({
      items: shortItems,
      overscanPx: 0,
      scrollTop: 320,
      viewportHeight: 96,
    })).toEqual({
      start: 0,
      end: 3,
    });
  });
});

describe('useVerticalVirtualList', () => {
  it('resolves an exposed scroll-area viewport and limits rendered items', () => {
    const viewport = createViewport(320);

    const viewportComponent = {
      viewportElement: ref(viewport),
    } as unknown as ComponentPublicInstance & {
      viewportElement: Ref<HTMLElement | null>;
    };

    const wrapper = mount(defineComponent({
      setup() {
        const items = computed(() =>
          Array.from({ length: 5_000 }, (_, index) => index));
        const virtualList = useVerticalVirtualList({
          items,
          getItemSize: () => 32,
        });

        virtualList.setScrollViewportRef(viewportComponent);

        return () => h(
          'div',
          virtualList.visibleItems.value.map(item =>
            h('div', {
              key: item.index,
              class: 'virtual-row',
            })),
        );
      },
    }));

    expect(wrapper.findAll('.virtual-row').length).toBeGreaterThan(0);
    expect(wrapper.findAll('.virtual-row').length).toBeLessThan(30);

    wrapper.unmount();
  });

  it('updates the visible range from scroll events', () => {
    const viewport = createViewport(96);
    const { virtualList, wrapper } = mountNumberVirtualList(viewport);
    viewport.scrollTop = 320;
    const scrollEvent = new Event('scroll');
    Object.defineProperty(scrollEvent, 'currentTarget', {
      configurable: true,
      value: viewport,
    });

    virtualList.handleScroll(scrollEvent);

    expect(virtualList.scrollTop.value).toBe(320);
    expect(virtualList.visibleItems.value.map(item => item.index)).toEqual([10, 11, 12]);

    wrapper.unmount();
  });

  it('uses the scroll viewport client height', () => {
    const clientHeight = window.innerHeight + 100;
    const viewport = createViewport(clientHeight);
    const { virtualList, wrapper } = mountNumberVirtualList(viewport);

    expect(virtualList.viewportHeight.value).toBe(clientHeight);

    wrapper.unmount();
  });

  it('scrolls an offscreen item into view', () => {
    const viewport = createViewport(96);
    const { virtualList, wrapper } = mountNumberVirtualList(viewport);

    expect(virtualList.scrollItemIntoView(20)).toBe(true);
    expect(viewport.scrollTop).toBe(576);
    expect(virtualList.scrollTop.value).toBe(576);

    wrapper.unmount();
  });
});
