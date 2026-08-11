// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
// Copyright © 2026 Cortexist, LLC (modifications). All rights reserved.

import { defineComponent, nextTick, ref } from 'vue';
import { createPinia, setActivePinia } from 'pinia';
import { mount } from '@vue/test-utils';
import {
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import type { DirEntry } from '@/types/dir-entry';
import { useInfoPanelImagePreview } from '../use-info-panel-image-preview';
import { useUserSettingsStore } from '@/stores/storage/user-settings';

const mockConvertFileSrc = vi.hoisted(() => vi.fn((path: string) => `asset://${path}`));
const mockGetImageThumbnail = vi.hoisted(() => vi.fn<(entry: DirEntry, maxDimension?: number) => string | undefined>());
const mockGetImageThumbnailPlaceholder = vi.hoisted(() => vi.fn<(entry: DirEntry, maxDimension?: number) => string | undefined>());
const mockClearThumbnails = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (path: string) => mockConvertFileSrc(path),
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/plugin-store', () => ({
  LazyStore: class {
    async save(): Promise<void> {}

    async set(): Promise<void> {}

    async entries(): Promise<[string, unknown][]> {
      return [];
    }
  },
}));

vi.mock('@tauri-apps/api/event', () => ({
  emit: vi.fn(),
  listen: vi.fn(async () => vi.fn()),
}));

vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    setZoom: vi.fn(),
  }),
}));

vi.mock('@tauri-apps/api/path', () => ({
  appDataDir: vi.fn(),
}));

vi.mock('@/stores/storage/user-paths', () => ({
  useUserPathsStore: () => ({
    customPaths: {
      appUserDataSettingsPath: '/tmp/user-data/user-settings.json',
    },
  }),
}));

vi.mock('@/modules/navigator/composables/use-navigator-image-thumbnails', () => ({
  useNavigatorImageThumbnails: () => ({
    getImageThumbnail: mockGetImageThumbnail,
    getImageThumbnailPlaceholder: mockGetImageThumbnailPlaceholder,
    clearThumbnails: mockClearThumbnails,
  }),
}));

/** The content version every `createImageEntry` carries: its `modified_time` and `size`. */
const ENTRY_VERSION = '123-1024';

function createImageEntry(extension: string, pathSuffix = extension): DirEntry {
  return {
    name: `photo.${extension}`,
    ext: extension,
    path: `C:/media/photo.${pathSuffix}`,
    size: 1024,
    item_count: null,
    modified_time: 123,
    accessed_time: 123,
    created_time: 123,
    mime: 'image/jpeg',
    is_file: true,
    is_dir: false,
    is_symlink: false,
    is_hidden: false,
  };
}

function mountInfoPanelImagePreview(selectedEntry = ref<DirEntry | null>(null)) {
  let composable!: ReturnType<typeof useInfoPanelImagePreview>;

  mount(defineComponent({
    setup() {
      composable = useInfoPanelImagePreview(selectedEntry);
      return {};
    },
    template: '<div />',
  }));

  return composable;
}

describe('useInfoPanelImagePreview', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    mockGetImageThumbnail.mockReset();
    mockGetImageThumbnailPlaceholder.mockReset();
    mockClearThumbnails.mockReset();
    mockConvertFileSrc.mockClear();
    mockGetImageThumbnail.mockReturnValue('asset://thumb');
    mockGetImageThumbnailPlaceholder.mockReturnValue('data:placeholder');

    Object.defineProperty(window, 'matchMedia', {
      configurable: true,
      writable: true,
      value: vi.fn(() => ({
        matches: false,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })),
    });
  });

  // The viewer always ends up on the original; the thumbnail only bridges the gap until it
  // decodes. So both layers are exposed, rather than one resolved to the other.
  it('always exposes the original as the image source', async () => {
    const selectedEntry = ref<DirEntry | null>(createImageEntry('png'));
    const preview = mountInfoPanelImagePreview(selectedEntry);
    await nextTick();

    expect(preview.imageOriginalSrc.value).toBe(`asset://C:/media/photo.png?v=${ENTRY_VERSION}`);
  });

  /**
   * The regression this guards: a file re-saved in place kept its path, so the source string
   * was identical, so nothing below it could tell that the picture had changed — the pane went
   * on showing the bytes from before the write until another file was selected and back.
   */
  it('re-points the source when the file behind the selection has been written to', async () => {
    const selectedEntry = ref<DirEntry | null>(createImageEntry('png'));
    const preview = mountInfoPanelImagePreview(selectedEntry);
    await nextTick();

    const before = preview.imageOriginalSrc.value;
    const beforeMedia = preview.playableMediaSrc.value;

    // What the info panel hands down after re-reading the file it watches: the same path,
    // carrying the times and size it has now. The wait for the write to finish happens there,
    // so an entry arriving here is already the settled one.
    selectedEntry.value = {
      ...createImageEntry('png'),
      modified_time: 456,
      size: 2048,
    };
    await nextTick();

    expect(preview.imageOriginalSrc.value).toBe('asset://C:/media/photo.png?v=456-2048');
    expect(preview.imageOriginalSrc.value).not.toBe(before);
    expect(preview.playableMediaSrc.value).not.toBe(beforeMedia);
  });

  it('skips the intermediate thumbnail when the full-size setting is enabled', async () => {
    const userSettingsStore = useUserSettingsStore();
    userSettingsStore.userSettings.navigator.infoPanel.showFullSizeImagePreview = true;

    const selectedEntry = ref<DirEntry | null>(createImageEntry('png'));
    const preview = mountInfoPanelImagePreview(selectedEntry);
    await nextTick();

    expect(preview.imageOriginalSrc.value).toBe(`asset://C:/media/photo.png?v=${ENTRY_VERSION}`);
    expect(preview.imageThumbnailSrc.value).toBe('');
    expect(mockGetImageThumbnail).not.toHaveBeenCalled();
    expect(preview.usesThumbnailImagePreview.value).toBe(false);
  });

  it('bridges with a thumbnail for gif files when the setting is disabled', async () => {
    const selectedEntry = ref<DirEntry | null>(createImageEntry('gif'));
    const preview = mountInfoPanelImagePreview(selectedEntry);
    await nextTick();

    expect(preview.imageThumbnailSrc.value).toBe('asset://thumb');
    expect(mockGetImageThumbnail).toHaveBeenCalled();
    expect(preview.usesThumbnailImagePreview.value).toBe(true);
  });

  it('bridges with a thumbnail when the setting is disabled', async () => {
    const selectedEntry = ref<DirEntry | null>(createImageEntry('png'));
    const preview = mountInfoPanelImagePreview(selectedEntry);
    await nextTick();

    expect(preview.imageThumbnailSrc.value).toBe('asset://thumb');
    expect(preview.imageOriginalSrc.value).toBe(`asset://C:/media/photo.png?v=${ENTRY_VERSION}`);
    expect(mockGetImageThumbnail).toHaveBeenCalled();
    expect(preview.usesThumbnailImagePreview.value).toBe(true);
  });

  it('serves svg from the original only, with no thumbnail stage', async () => {
    const selectedEntry = ref<DirEntry | null>(createImageEntry('svg'));
    const preview = mountInfoPanelImagePreview(selectedEntry);
    await nextTick();

    expect(preview.imageOriginalSrc.value).toBe(`asset://C:/media/photo.svg?v=${ENTRY_VERSION}`);
    expect(preview.imageThumbnailSrc.value).toBe('');
    expect(preview.usesThumbnailImagePreview.value).toBe(false);
  });

  /**
   * The pane is measured in device pixels and the request used to be clamped to the grid's
   * 384, which is what made this preview look soft on a high-DPI display.
   */
  it('requests a thumbnail at the measured pane size rather than the grid size', async () => {
    const selectedEntry = ref<DirEntry | null>(createImageEntry('png'));
    const preview = mountInfoPanelImagePreview(selectedEntry);
    await nextTick();

    // The source is a lazy computed, so it has to be read for the request to be made.
    void preview.imageThumbnailSrc.value;

    expect(mockGetImageThumbnail).toHaveBeenCalledWith(expect.anything(), 560);
  });

  it('reports no image source for a non-image entry', async () => {
    const selectedEntry = ref<DirEntry | null>(null);
    const preview = mountInfoPanelImagePreview(selectedEntry);
    await nextTick();

    expect(preview.isImageFile.value).toBe(false);
    expect(preview.imageOriginalSrc.value).toBe('');
    expect(preview.imageThumbnailSrc.value).toBe('');
  });

  it('keeps the shared thumbnail cache when switching between image entries in thumbnail mode', async () => {
    const selectedEntry = ref<DirEntry | null>(createImageEntry('png'));
    mountInfoPanelImagePreview(selectedEntry);
    mockClearThumbnails.mockClear();

    selectedEntry.value = createImageEntry('jpg', 'jpg');
    await nextTick();

    expect(mockClearThumbnails).not.toHaveBeenCalled();
  });

  it('does not clear thumbnails when original preview is enabled', async () => {
    const userSettingsStore = useUserSettingsStore();
    userSettingsStore.userSettings.navigator.infoPanel.showFullSizeImagePreview = true;

    const selectedEntry = ref<DirEntry | null>(createImageEntry('png'));
    mountInfoPanelImagePreview(selectedEntry);
    await nextTick();
    mockClearThumbnails.mockClear();

    selectedEntry.value = createImageEntry('jpg', 'jpg');
    await nextTick();

    expect(mockClearThumbnails).not.toHaveBeenCalled();
  });

  it('keeps the shared thumbnail cache when switching from original preview back to thumbnails', async () => {
    const userSettingsStore = useUserSettingsStore();
    userSettingsStore.userSettings.navigator.infoPanel.showFullSizeImagePreview = true;

    const selectedEntry = ref<DirEntry | null>(createImageEntry('png'));
    mountInfoPanelImagePreview(selectedEntry);
    await nextTick();
    mockClearThumbnails.mockClear();

    userSettingsStore.userSettings.navigator.infoPanel.showFullSizeImagePreview = false;
    await nextTick();

    expect(mockClearThumbnails).not.toHaveBeenCalled();
  });
});
