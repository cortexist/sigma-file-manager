// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
// Copyright © 2026 Cortexist, LLC (modifications). All rights reserved.

import { defineStore } from 'pinia';
import { ref, computed, markRaw, shallowRef } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { toast, ToastStatic } from '@/components/ui/toaster';
import { i18n } from '@/localization';
import type { DirContents } from '@/types/dir-entry';
import { canonicalizePath, getParentDirectory } from '@/utils/normalize-path';
import {
  emitAuxiliaryWindowEvent,
  findAuxiliaryWindow,
  hasAuxiliaryWindowBeenShown,
  markAuxiliaryWindowShown,
  releaseAuxiliaryWindow,
  runAuxiliaryWindowTask,
} from '@/utils/auxiliary-windows';

export type QuickViewFileType = 'image' | 'video' | 'audio' | 'pdf' | 'text' | 'unsupported';

const IMAGE_EXTENSIONS = ['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'ico', 'tiff', 'tif', 'avif'];
const VIDEO_EXTENSIONS = ['mp4', 'webm', 'ogg', 'ogv', 'mov', 'avi', 'mkv', 'm4v', 'wmv', 'flv'];
const AUDIO_EXTENSIONS = ['mp3', 'wav', 'ogg', 'oga', 'flac', 'aac', 'm4a', 'wma', 'opus'];
const PDF_EXTENSIONS = ['pdf'];
const TEXT_EXTENSIONS = ['txt', 'md', 'json', 'xml', 'html', 'css', 'js', 'ts', 'vue', 'jsx', 'tsx', 'yaml', 'yml', 'toml', 'ini', 'cfg', 'conf', 'log', 'sh', 'bash', 'zsh', 'ps1', 'bat', 'cmd', 'py', 'rb', 'rs', 'go', 'java', 'c', 'cpp', 'h', 'hpp', 'cs', 'swift', 'kt', 'php', 'sql', 'graphql', 'env', 'gitignore', 'dockerignore', 'editorconfig', 'prettierrc', 'eslintrc'];

export function isHttpOrHttpsUrl(value: string): boolean {
  return /^https?:\/\//i.test(value);
}

export function getFileExtension(path: string): string {
  let candidate = path;

  if (isHttpOrHttpsUrl(path)) {
    try {
      candidate = new URL(path).pathname;
    }
    catch {
      candidate = path.split('?')[0]?.split('#')[0] ?? path;
    }
  }
  else {
    candidate = path.split('?')[0]?.split('#')[0] ?? path;
  }

  const parts = candidate.split('.');
  return parts.length > 1 ? parts[parts.length - 1].toLowerCase() : '';
}

export function getFileName(path: string): string {
  if (isHttpOrHttpsUrl(path)) {
    try {
      const parsed = new URL(path);
      const segments = parsed.pathname.split('/').filter(Boolean);
      return segments[segments.length - 1] || parsed.hostname;
    }
    catch {
    }
  }

  const normalized = path.replace(/\\/g, '/');
  const parts = normalized.split('/');
  return parts[parts.length - 1] || '';
}

export function determineFileType(path: string): QuickViewFileType {
  const extension = getFileExtension(path);

  if (IMAGE_EXTENSIONS.includes(extension)) return 'image';
  if (VIDEO_EXTENSIONS.includes(extension)) return 'video';
  if (AUDIO_EXTENSIONS.includes(extension)) return 'audio';
  if (PDF_EXTENSIONS.includes(extension)) return 'pdf';
  if (TEXT_EXTENSIONS.includes(extension)) return 'text';

  return 'unsupported';
}

export function isQuickViewSupported(path: string): boolean {
  return determineFileType(path) !== 'unsupported';
}

export function quickViewSupportedPathsFromVisibleEntries(
  entries: readonly {
    path: string;
    is_file: boolean;
  }[],
): string[] {
  const paths: string[] = [];

  for (const entry of entries) {
    if (entry.is_file && isQuickViewSupported(entry.path)) {
      paths.push(entry.path);
    }
  }

  return paths;
}

export async function fetchQuickViewSiblingPathsFromDisk(filePath: string): Promise<string[]> {
  if (isHttpOrHttpsUrl(filePath)) {
    return [filePath];
  }

  const parentDir = getParentDirectory(filePath);

  if (!parentDir) {
    return [filePath];
  }

  try {
    const contents = await invoke<DirContents>('read_dir', { path: parentDir });
    const paths = contents.entries
      .filter(entry => entry.is_file && isQuickViewSupported(entry.path))
      .map(entry => entry.path)
      .sort((pathA, pathB) =>
        getFileName(pathA).localeCompare(getFileName(pathB), undefined, { sensitivity: 'base' }),
      );

    if (paths.length === 0) {
      return [filePath];
    }

    if (!paths.includes(filePath)) {
      return [filePath];
    }

    return paths;
  }
  catch {
    return [filePath];
  }
}

export function getFileAssetUrl(path: string): string {
  if (!path) return '';
  return convertFileSrc(path);
}

export function getQuickViewDisplayUrl(pathOrUrl: string): string {
  if (!pathOrUrl) return '';

  if (isHttpOrHttpsUrl(pathOrUrl)) {
    return pathOrUrl;
  }

  return convertFileSrc(pathOrUrl);
}

export const QUICK_VIEW_DISPLAYED_PATH_CHANGED_EVENT = 'quick-view:displayed-path-changed';
export const QUICK_VIEW_LOAD_FILE_EVENT = 'quick-view:load-file';
/**
 * A second app launch carrying a media file, resolved and re-emitted by the backend. In a
 * normal session the main window answers by opening Quick View; a standalone viewer answers
 * for itself by swapping its file.
 */
export const OPEN_MEDIA_REQUEST_EVENT = 'open-media-request';
export const QUICK_VIEW_SIBLING_PATHS_CHANGED_EVENT = 'quick-view:sibling-paths-changed';
export const PRINT_VIEW_LOAD_FILE_EVENT = 'quick-view:load-file:print-view';

async function runAuxiliaryWindowSteps(
  isCurrent: () => boolean,
  steps: Array<() => Promise<void | boolean>>,
): Promise<boolean> {
  for (const step of steps) {
    if (!isCurrent()) {
      return false;
    }

    const stepResult = await step();

    if (stepResult === false) {
      return false;
    }
  }

  return isCurrent();
}

export const useQuickViewStore = defineStore('quickView', () => {
  const currentFilePath = ref<string | null>(null);
  const isLoading = ref(false);
  const lastOpenedPath = ref<string | null>(null);
  /** Folder Quick View is bound to; outlives the displayed file being deleted. */
  const lastOpenedDirectory = ref<string | null>(null);
  const displayedPathEventUnlisteners = shallowRef<UnlistenFn[]>([]);

  const fileType = computed((): QuickViewFileType => {
    if (!currentFilePath.value) return 'unsupported';
    return determineFileType(currentFilePath.value);
  });

  const fileName = computed((): string => {
    if (!currentFilePath.value) return '';
    return getFileName(currentFilePath.value);
  });

  const fileAssetUrl = computed((): string => {
    if (!currentFilePath.value) return '';
    return getQuickViewDisplayUrl(currentFilePath.value);
  });

  async function getQuickViewWindow() {
    return findAuxiliaryWindow('quick-view');
  }

  async function getPrintViewWindow() {
    return findAuxiliaryWindow('print-view');
  }

  function showUnsupportedFileToast(
    fileName: string,
    context: 'quickView' | 'printView',
  ): void {
    const { t } = i18n.global;
    const titleTranslationKey = context === 'printView'
      ? 'notifications.printViewFileIsNotSupported'
      : 'notifications.quickViewFileIsNotSupported';

    toast.custom(markRaw(ToastStatic), {
      componentProps: {
        data: {
          title: t(titleTranslationKey),
          description: fileName,
        },
      },
      duration: 3000,
    });
  }

  async function openFileFromMainWindow(
    path: string,
    siblingPaths?: string[] | null,
  ): Promise<boolean> {
    const type = determineFileType(path);

    if (type === 'unsupported') {
      showUnsupportedFileToast(getFileName(path), 'quickView');
      return false;
    }

    const opened = await runAuxiliaryWindowTask('quick-view', async ({ window: quickWindow, isCurrent }) => {
      function loadFile() {
        return emitAuxiliaryWindowEvent('quick-view', QUICK_VIEW_LOAD_FILE_EVENT, {
          path,
          siblingPaths:
            siblingPaths === undefined || siblingPaths === null || siblingPaths.length === 0
              ? null
              : siblingPaths,
        });
      }

      async function showWindow() {
        await quickWindow.show();
        markAuxiliaryWindowShown('quick-view');
      }

      /**
       * Handing the file over before showing is what makes the window appear with its content
       * already in place, rather than filling in afterwards. It is only safe once the window
       * has a surface: a prelaunched window has never been on screen, and a media pipeline
       * built against it cannot preroll, leaving the player buffering behind a spinner that
       * never clears. So the first open shows first and loads second, and every open after
       * that — the window having kept its surface through being hidden — keeps the nicer
       * order. See `hasAuxiliaryWindowBeenShown`.
       */
      const firstOpen = !hasAuxiliaryWindowBeenShown('quick-view');

      return runAuxiliaryWindowSteps(isCurrent, [
        () => quickWindow.setTitle(`Sigma File Manager | Quick View - ${getFileName(path)}`),
        () => quickWindow.center(),
        ...(firstOpen ? [showWindow, loadFile] : [loadFile, showWindow]),
        () => quickWindow.setFocus(),
      ]);
    });

    if (opened) {
      lastOpenedPath.value = path;
      lastOpenedDirectory.value = canonicalizePath(getParentDirectory(path));
    }

    return opened ?? false;
  }

  async function openPrintViewFromMainWindow(path: string): Promise<boolean> {
    const type = determineFileType(path);

    if (type === 'unsupported' || type === 'audio') {
      showUnsupportedFileToast(getFileName(path), 'printView');
      return false;
    }

    const opened = await runAuxiliaryWindowTask('print-view', async ({ window: printWindow, isCurrent }) => {
      return runAuxiliaryWindowSteps(isCurrent, [
        () => printWindow.setTitle(`Sigma File Manager | Print - ${getFileName(path)}`),
        () => printWindow.center(),
        // Already the safe order: nothing is loaded until the window has a surface.
        async () => {
          await printWindow.show();
          markAuxiliaryWindowShown('print-view');
        },
        () => printWindow.setFocus(),
        async () => {
          const didEmitLoadEvent = await emitAuxiliaryWindowEvent('print-view', PRINT_VIEW_LOAD_FILE_EVENT, {
            path,
            siblingPaths: null,
          });

          return didEmitLoadEvent;
        },
      ]);
    });

    return opened ?? false;
  }

  function loadFile(path: string): void {
    currentFilePath.value = path;
    isLoading.value = false;
  }

  async function closeWindow(): Promise<void> {
    await releaseAuxiliaryWindow('quick-view');
    lastOpenedPath.value = null;
    lastOpenedDirectory.value = null;
  }

  /**
   * Keeps the Quick View filmstrip in step with the browser listing it was opened from, so a
   * file created or deleted on disk appears or disappears there too.
   *
   * The directory guard matters because every open tab and split pane runs its own listing:
   * without it, a refresh in some unrelated folder would replace the strip with that folder's
   * files. It is tracked separately from `lastOpenedPath` because deleting the open file
   * leaves Quick View showing nothing while still bound to that folder — deriving the folder
   * from the displayed file would stop the updates exactly when they are still wanted.
   */
  async function syncSiblingPaths(directory: string, paths: string[]): Promise<boolean> {
    if (getCurrentWebviewWindow().label !== 'main') {
      return false;
    }

    const boundDirectory = lastOpenedDirectory.value;

    if (!boundDirectory || boundDirectory !== canonicalizePath(directory)) {
      return false;
    }

    // Resolves false on its own when the window is gone or unresponsive.
    return await emitAuxiliaryWindowEvent(
      'quick-view',
      QUICK_VIEW_SIBLING_PATHS_CHANGED_EVENT,
      { paths },
    );
  }

  async function isWindowVisible(): Promise<boolean> {
    const quickWindow = await getQuickViewWindow();

    if (quickWindow) {
      return await quickWindow.isVisible();
    }

    return false;
  }

  async function toggleQuickView(
    path: string,
    siblingPaths?: string[] | null,
  ): Promise<boolean> {
    const isVisible = await isWindowVisible();

    if (isVisible && lastOpenedPath.value === path) {
      await closeWindow();
      return true;
    }

    return await openFileFromMainWindow(path, siblingPaths);
  }

  async function ensureMainWindowDisplayedPathListener(): Promise<void> {
    if (displayedPathEventUnlisteners.value.length > 0) {
      return;
    }

    if (getCurrentWebviewWindow().label !== 'main') {
      return;
    }

    const unlisten = await listen<{ path: string | null }>(
      QUICK_VIEW_DISPLAYED_PATH_CHANGED_EVENT,
      (event) => {
        lastOpenedPath.value = event.payload.path;

        // A null path means the open file was deleted, not that the window moved on from the
        // folder, so the binding stays put and the strip keeps receiving updates.
        if (event.payload.path) {
          lastOpenedDirectory.value = canonicalizePath(getParentDirectory(event.payload.path));
        }
      },
    );

    displayedPathEventUnlisteners.value = [unlisten];
  }

  return {
    currentFilePath,
    isLoading,
    fileType,
    fileName,
    fileAssetUrl,
    lastOpenedPath,
    lastOpenedDirectory,
    loadFile,
    openFileFromMainWindow,
    openPrintViewFromMainWindow,
    closeWindow,
    isWindowVisible,
    syncSiblingPaths,
    toggleQuickView,
    getQuickViewWindow,
    getPrintViewWindow,
    ensureMainWindowDisplayedPathListener,
  };
});
