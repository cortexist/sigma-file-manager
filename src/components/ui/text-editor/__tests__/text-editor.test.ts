// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { flushPromises, mount, type VueWrapper } from '@vue/test-utils';
import {
  afterEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import { defineComponent, h, ref } from 'vue';
import TextEditor from '../text-editor.vue';

vi.mock('vue-i18n', () => ({
  useI18n: () => ({
    t: (key: string, params?: Record<string, unknown>) =>
      key === 'textEditor.matchPosition' ? `${params?.current} of ${params?.total}` : key,
  }),
}));

/** Renders its default slot in a plain div: the tooltip chrome is not what is under test. */
const SlotStub = defineComponent({
  setup(_, { slots }) {
    return () => h('div', slots.default?.());
  },
});

const STUBS = {
  Tooltip: SlotStub,
  TooltipTrigger: SlotStub,
  TooltipContent: true,
  ScrollArea: SlotStub,
};

type EditorProps = {
  modelValue: string;
  readonly?: boolean;
  markdown?: boolean;
  mode?: 'read' | 'split' | 'edit';
  canSave?: boolean;
};

type Exposed = {
  openFind: (withReplace?: boolean) => void;
  closeFind: () => boolean;
};

let mounted: VueWrapper[] = [];

/**
 * Mounts the editor under a parent that owns the text, the way Quick View does, so a
 * replacement round-trips through `update:modelValue` and back into the editor.
 */
function mountEditor(props: EditorProps) {
  const text = ref(props.modelValue);
  const Host = defineComponent({
    setup() {
      return () => h(TextEditor, {
        ...props,
        'modelValue': text.value,
        'onUpdate:modelValue': (value: string) => {
          text.value = value;
        },
      });
    },
  });
  const wrapper = mount(Host, {
    attachTo: document.body,
    global: { stubs: STUBS },
  });

  mounted.push(wrapper);

  const editor = wrapper.findComponent(TextEditor);

  return {
    wrapper,
    editor,
    text,
    exposed: editor.vm as unknown as Exposed,
  };
}

function pressOnWindow(init: KeyboardEventInit) {
  window.dispatchEvent(new KeyboardEvent('keydown', {
    bubbles: true,
    cancelable: true,
    ...init,
  }));
}

async function openFindWithQuery(editor: VueWrapper, query: string) {
  pressOnWindow({
    code: 'KeyF',
    ctrlKey: true,
  });
  await flushPromises();
  await editor.get('.text-find-bar__input').setValue(query);
  await flushPromises();
}

function countText(editor: VueWrapper) {
  return editor.get('.text-find-bar__count').text();
}

function textarea(editor: VueWrapper) {
  return editor.get('textarea').element as HTMLTextAreaElement;
}

afterEach(() => {
  for (const wrapper of mounted) {
    wrapper.unmount();
  }

  mounted = [];
});

describe('TextEditor', () => {
  describe('find', () => {
    it('opens on Ctrl+F with the query focused', async () => {
      const { editor } = mountEditor({ modelValue: 'hello' });

      expect(editor.find('.text-find-bar').exists()).toBe(false);

      pressOnWindow({
        code: 'KeyF',
        ctrlKey: true,
      });
      await flushPromises();

      expect(editor.find('.text-find-bar').exists()).toBe(true);
      expect(document.activeElement).toBe(editor.get('.text-find-bar__input').element);
    });

    it('counts the matches and steps through them with Enter, wrapping at the end', async () => {
      const { editor } = mountEditor({ modelValue: 'fox fox fox' });
      await openFindWithQuery(editor, 'fox');

      expect(countText(editor)).toBe('1 of 3');

      const input = editor.get('.text-find-bar__input');

      await input.trigger('keydown', { key: 'Enter' });
      expect(countText(editor)).toBe('2 of 3');

      await input.trigger('keydown', { key: 'Enter' });
      await input.trigger('keydown', { key: 'Enter' });
      expect(countText(editor)).toBe('1 of 3');

      await input.trigger('keydown', {
        key: 'Enter',
        shiftKey: true,
      });
      expect(countText(editor)).toBe('3 of 3');
    });

    /**
     * The marks are drawn in a layer behind the textarea that repeats its text with the same
     * metrics. One stray character in that copy — template whitespace, say — and every mark
     * after it sits under the wrong letters.
     */
    it('repeats the text exactly in the highlight layer', async () => {
      const text = 'ab  ab\n\tab ab';
      const { editor } = mountEditor({ modelValue: text });
      await openFindWithQuery(editor, 'ab');

      const backdrop = editor.get('.text-editor__backdrop');

      expect(backdrop.element.textContent).toBe(text);
      expect(backdrop.findAll('mark')).toHaveLength(4);
      expect(backdrop.findAll('.text-editor__match--active')).toHaveLength(1);
    });

    it('says so when nothing matches', async () => {
      const { editor } = mountEditor({ modelValue: 'hello' });
      await openFindWithQuery(editor, 'xyz');

      expect(countText(editor)).toBe('textEditor.noMatches');
    });

    it('respects the case toggle', async () => {
      const { editor } = mountEditor({ modelValue: 'Fox fox' });
      await openFindWithQuery(editor, 'fox');

      expect(countText(editor)).toBe('1 of 2');

      await editor.get('[aria-label="textEditor.matchCase"]').trigger('click');
      await flushPromises();

      expect(countText(editor)).toBe('1 of 1');
    });

    /** Typing a query should find the next occurrence from where the reader is, not the first in the file. */
    it('starts from the caret', async () => {
      const { editor } = mountEditor({ modelValue: 'a b a b a' });

      textarea(editor).setSelectionRange(3, 3);
      await openFindWithQuery(editor, 'a');

      expect(countText(editor)).toBe('2 of 3');
    });

    it('selects the active match in the editor so closing leaves it ready to copy', async () => {
      const { editor, exposed } = mountEditor({ modelValue: 'one two three' });
      await openFindWithQuery(editor, 'two');

      const element = textarea(editor);

      expect([element.selectionStart, element.selectionEnd]).toEqual([4, 7]);

      expect(exposed.closeFind()).toBe(true);
      await flushPromises();

      expect(editor.find('.text-find-bar').exists()).toBe(false);
      expect(document.activeElement).toBe(element);
      // Nothing was open the second time, so the owner knows Escape is theirs.
      expect(exposed.closeFind()).toBe(false);
    });

    it('seeds the query with a single-line selection', async () => {
      const { editor } = mountEditor({ modelValue: 'alpha beta alpha' });

      textarea(editor).setSelectionRange(0, 5);
      pressOnWindow({
        code: 'KeyF',
        ctrlKey: true,
      });
      await flushPromises();

      expect((editor.get('.text-find-bar__input').element as HTMLInputElement).value).toBe('alpha');
      expect(countText(editor)).toBe('1 of 2');
    });

    it('leaves keys typed into another field alone', async () => {
      const { editor } = mountEditor({ modelValue: 'hello' });
      const outside = document.createElement('input');
      document.body.append(outside);
      outside.focus();

      outside.dispatchEvent(new KeyboardEvent('keydown', {
        code: 'KeyF',
        ctrlKey: true,
        bubbles: true,
      }));
      await flushPromises();

      expect(editor.find('.text-find-bar').exists()).toBe(false);
      outside.remove();
    });
  });

  describe('replace', () => {
    it('replaces the active match and moves on to the next', async () => {
      const { editor, text } = mountEditor({ modelValue: 'fox fox fox' });
      await openFindWithQuery(editor, 'fox');
      await editor.get('[aria-label="textEditor.toggleReplace"]').trigger('click');
      await editor.get('[aria-label="textEditor.replacePlaceholder"]').setValue('cat');

      const replaceButtons = editor.findAll('button').filter(button => button.text() === 'textEditor.replace');
      await replaceButtons[0].trigger('click');
      await flushPromises();

      expect(text.value).toBe('cat fox fox');
      expect(countText(editor)).toBe('1 of 2');
    });

    /** A replacement containing the query must not be found again, or Replace would never end. */
    it('continues past a replacement that contains the query', async () => {
      const { editor, text } = mountEditor({ modelValue: 'a a' });
      await openFindWithQuery(editor, 'a');
      await editor.get('[aria-label="textEditor.toggleReplace"]').trigger('click');
      await editor.get('[aria-label="textEditor.replacePlaceholder"]').setValue('aa');

      const replaceButton = editor.findAll('button').find(button => button.text() === 'textEditor.replace');
      await replaceButton?.trigger('click');
      await flushPromises();

      expect(text.value).toBe('aa a');
      // Matches are now at 0, 1 and 3; the one after the replacement is the third.
      expect(countText(editor)).toBe('3 of 3');
    });

    it('replaces every match at once', async () => {
      const { editor, text } = mountEditor({ modelValue: 'fox fox fox' });
      await openFindWithQuery(editor, 'fox');
      await editor.get('[aria-label="textEditor.toggleReplace"]').trigger('click');
      await editor.get('[aria-label="textEditor.replacePlaceholder"]').setValue('cat');

      const replaceAll = editor.findAll('button').find(button => button.text() === 'textEditor.replaceAll');
      await replaceAll?.trigger('click');
      await flushPromises();

      expect(text.value).toBe('cat cat cat');
      expect(countText(editor)).toBe('textEditor.noMatches');
    });

    it('is not offered for read-only text', async () => {
      const { editor, text } = mountEditor({
        modelValue: 'fox',
        readonly: true,
      });
      await openFindWithQuery(editor, 'fox');

      expect(editor.find('[aria-label="textEditor.toggleReplace"]').exists()).toBe(false);

      pressOnWindow({
        code: 'KeyH',
        ctrlKey: true,
      });
      await flushPromises();

      expect(editor.find('[aria-label="textEditor.replacePlaceholder"]').exists()).toBe(false);
      expect(text.value).toBe('fox');
    });
  });

  describe('toolbar', () => {
    it('offers save and revert only while there is something to save', async () => {
      const { editor } = mountEditor({
        modelValue: 'fox',
        canSave: false,
      });

      expect((editor.get('[aria-label="textEditor.save"]').element as HTMLButtonElement).disabled).toBe(true);

      const { editor: dirty } = mountEditor({
        modelValue: 'fox',
        canSave: true,
      });
      await dirty.get('[aria-label="textEditor.save"]').trigger('click');
      await dirty.get('[aria-label="textEditor.revert"]').trigger('click');

      expect(dirty.emitted('save')).toHaveLength(1);
      expect(dirty.emitted('revert')).toHaveLength(1);
      expect(dirty.get('[aria-label="textEditor.save"]').classes()).toContain('text-editor-tool--on');
    });

    it('asks its owner to switch a plain text file between reading and editing', async () => {
      const { editor } = mountEditor({
        modelValue: 'fox',
        mode: 'read',
      });

      expect((editor.get('textarea').element as HTMLTextAreaElement).readOnly).toBe(true);

      await editor.get('[aria-label="textEditor.edit"]').trigger('click');
      expect(editor.emitted('update:mode')).toEqual([['edit']]);
    });
  });

  describe('markdown', () => {
    it('shows no markdown controls for plain text', () => {
      const { editor } = mountEditor({ modelValue: 'plain' });

      expect(editor.find('[aria-label="textEditor.split"]').exists()).toBe(false);
      expect(editor.find('[aria-label="textEditor.formatBold"]').exists()).toBe(false);
    });

    it('shows source and page in split mode, the page alone in read mode, the source alone in edit mode', async () => {
      const { editor } = mountEditor({
        modelValue: '# Title',
        markdown: true,
        mode: 'split',
      });
      await flushPromises();

      expect(editor.find('textarea').exists()).toBe(true);
      expect(editor.find('.text-editor-markdown').exists()).toBe(true);

      const { editor: reader } = mountEditor({
        modelValue: '# Title',
        markdown: true,
        mode: 'read',
      });
      await flushPromises();

      expect(reader.find('textarea').exists()).toBe(false);
      expect(reader.get('.text-editor-markdown h1').text()).toBe('Title');

      const { editor: writer } = mountEditor({
        modelValue: '# Title',
        markdown: true,
        mode: 'edit',
      });

      expect(writer.find('textarea').exists()).toBe(true);
      expect(writer.find('.text-editor-markdown').exists()).toBe(false);
    });

    it('asks its owner to change the mode rather than doing so itself', async () => {
      const { editor } = mountEditor({
        modelValue: '# Title',
        markdown: true,
        mode: 'split',
      });

      // The edit toggle reads "on" in the split, and turning it off means reading.
      await editor.get('[aria-label="textEditor.read"]').trigger('click');
      expect(editor.emitted('update:mode')).toEqual([['read']]);

      // Still split: the owner decides, and it has not answered.
      expect(editor.find('textarea').exists()).toBe(true);
    });

    it('has one split toggle: off goes back to editing, on from reading opens the split', async () => {
      const { editor } = mountEditor({
        modelValue: '# Title',
        markdown: true,
        mode: 'split',
      });

      const toggle = editor.get('[aria-label="textEditor.splitSideBySide"]');
      expect(toggle.attributes('aria-pressed')).toBe('true');

      await toggle.trigger('click');
      expect(editor.emitted('update:mode')).toEqual([['edit']]);

      const { editor: reader } = mountEditor({
        modelValue: '# Title',
        markdown: true,
        mode: 'read',
      });

      await reader.get('[aria-label="textEditor.split"]').trigger('click');
      expect(reader.emitted('update:mode')).toEqual([['split']]);
    });

    /** A viewer handed to another application gets the page and nothing to change it with. */
    it('shows a read-only markdown file rendered, with no editing controls', async () => {
      const { editor } = mountEditor({
        modelValue: '# Title',
        markdown: true,
        readonly: true,
        mode: 'split',
      });
      await flushPromises();

      expect(editor.find('textarea').exists()).toBe(false);
      expect(editor.get('.text-editor-markdown h1').text()).toBe('Title');
      expect(editor.find('[aria-label="textEditor.edit"]').exists()).toBe(false);
      expect(editor.find('[aria-label="textEditor.save"]').exists()).toBe(false);
      expect(editor.find('[aria-label="textEditor.find"]').exists()).toBe(true);
    });

    it('wraps the selection when a format button is pressed, through the text\'s owner', async () => {
      const { editor, text } = mountEditor({
        modelValue: 'make this strong',
        markdown: true,
        mode: 'edit',
      });
      const textarea = editor.get('textarea').element as HTMLTextAreaElement;
      textarea.setSelectionRange(5, 9);

      await editor.get('[aria-label="textEditor.formatBold"]').trigger('click');
      await flushPromises();

      expect(text.value).toBe('make **this** strong');
      expect(textarea.selectionStart).toBe(7);
      expect(textarea.selectionEnd).toBe(11);
    });

    it('finds in the rendered page when that is all that is shown', async () => {
      const { editor } = mountEditor({
        modelValue: 'some **bold** text and more text',
        markdown: true,
        mode: 'read',
      });
      await openFindWithQuery(editor, 'text');

      expect(countText(editor)).toBe('1 of 2');
      expect(editor.find('[aria-label="textEditor.toggleReplace"]').exists()).toBe(false);
    });
  });
});
