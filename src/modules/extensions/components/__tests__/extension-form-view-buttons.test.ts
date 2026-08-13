// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { mount } from '@vue/test-utils';
import { describe, expect, it, vi } from 'vitest';
import type { UIElement } from '@/types/extension';
import ExtensionFormView from '@/modules/extensions/components/extension-form-view.vue';

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('vue-i18n', async importOriginal => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useI18n: () => ({ t: (key: string) => key }),
}));

function mountForm(content: UIElement[]) {
  return mount(ExtensionFormView, {
    props: {
      title: 'ID3 Tags',
      content,
      values: {},
      buttons: [{
        id: 'save',
        label: 'Save',
        variant: 'primary',
      }],
    },
    global: {
      stubs: {
        ExtensionModalHeader: true,
        ExtensionModalActionFooter: true,
        ScrollArea: { template: '<div><slot /></div>' },
      },
    },
  });
}

const COVER_CONTROLS: UIElement[] = [
  {
    type: 'image',
    id: 'cover',
    value: '',
    label: 'No cover art',
  },
  {
    type: 'button',
    id: 'matchOnline',
    label: '',
    icon: 'ScanSearch',
    tooltip: 'Match online',
    size: 'xs',
  },
  {
    type: 'button',
    id: 'chooseCover',
    label: '',
    icon: 'ImagePlus',
    tooltip: 'Choose cover',
    size: 'xs',
  },
  {
    type: 'button',
    id: 'removeCover',
    label: '',
    icon: 'ImageOff',
    tooltip: 'Remove cover',
    size: 'xs',
    disabled: true,
  },
];

describe('icon-only buttons carrying a tooltip', () => {
  /**
   * The regression this exists for: extension modals mount outside the window's tooltip
   * provider, and a tooltip without one throws instead of degrading — which took the
   * button it was attached to off the screen entirely. The view now supplies its own.
   */
  it('renders every control rather than throwing on the missing provider', () => {
    const wrapper = mountForm(COVER_CONTROLS);

    expect(wrapper.findAll('button').length).toBeGreaterThanOrEqual(3);
  });

  it('renders an icon inside a button that has no label', () => {
    const wrapper = mountForm(COVER_CONTROLS);
    const iconOnlyButtons = wrapper.findAll('.ext-form-view__inline-button--icon-only');

    expect(iconOnlyButtons).toHaveLength(3);
  });

  it('disables a control the extension marked unavailable', () => {
    const wrapper = mountForm(COVER_CONTROLS);
    const disabledButtons = wrapper.findAll('button[disabled]');

    expect(disabledButtons.length).toBeGreaterThanOrEqual(1);
  });

  it('emits the button id when pressed, so the extension hears it', async () => {
    const wrapper = mountForm(COVER_CONTROLS);
    await wrapper.findAll('.ext-form-view__inline-button')[0].trigger('click');

    expect(wrapper.emitted('buttonClick')?.[0]).toEqual(['matchOnline']);
  });

  it('still renders a plain labelled button with no tooltip', () => {
    const wrapper = mountForm([
      {
        type: 'button',
        id: 'applyMatch',
        label: 'Apply match',
        variant: 'primary',
        size: 'sm',
      },
    ]);

    expect(wrapper.text()).toContain('Apply match');
  });

  it('renders a placeholder frame when an image has no source', () => {
    const wrapper = mountForm(COVER_CONTROLS);

    expect(wrapper.find('.ext-form-view__image-placeholder').exists()).toBe(true);
    expect(wrapper.find('img.ext-form-view__image').exists()).toBe(false);
  });

  it('renders the image itself once there is a source', () => {
    const wrapper = mountForm([
      {
        type: 'image',
        id: 'cover',
        value: 'data:image/png;base64,AA',
        label: 'Cover',
      },
    ]);

    expect(wrapper.find('img.ext-form-view__image').exists()).toBe(true);
    expect(wrapper.find('.ext-form-view__image-placeholder').exists()).toBe(false);
  });
});
