// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { flushPromises, mount, type VueWrapper } from '@vue/test-utils';
import { describe, expect, it } from 'vitest';
import MarkdownView from '../markdown-view.vue';

function lastReport(wrapper: VueWrapper) {
  const reports = wrapper.emitted('find-matches') ?? [];

  return reports[reports.length - 1];
}

describe('MarkdownView', () => {
  it('renders the markdown as a page', async () => {
    const wrapper = mount(MarkdownView, {
      props: { source: '# Title\n\nSome **bold** text.' },
    });
    await flushPromises();

    expect(wrapper.get('h1').text()).toBe('Title');
    expect(wrapper.get('strong').text()).toBe('bold');
  });

  /**
   * What the reader sees as one sentence is several text nodes once any of it is bold or
   * linked; a match has to be found across them, the way the eye reads it, not per node.
   */
  it('counts matches over the rendered text, across inline markup', async () => {
    const wrapper = mount(MarkdownView, {
      props: {
        source: 'some **bold** text',
        find: {
          query: 'e bold t',
          matchCase: false,
          activeIndex: 0,
        },
      },
    });
    await flushPromises();

    expect(lastReport(wrapper)).toEqual([1]);
  });

  it('re-counts when the query changes', async () => {
    const wrapper = mount(MarkdownView, {
      props: {
        source: 'one two one',
        find: {
          query: 'one',
          matchCase: false,
          activeIndex: 0,
        },
      },
    });
    await flushPromises();

    expect(lastReport(wrapper)).toEqual([2]);

    await wrapper.setProps({
      find: {
        query: 'two',
        matchCase: false,
        activeIndex: 0,
      },
    });
    await flushPromises();

    expect(lastReport(wrapper)).toEqual([1]);
  });
});
