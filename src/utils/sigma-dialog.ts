// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

/**
 * File dialogs, raised by Sigma's own picker instead of the platform's.
 *
 * `@tauri-apps/plugin-dialog` reaches GTK directly through rfd, which puts the system's file
 * chooser in front of a user who chose this file manager — and, on this fork, in front of the
 * very picker Sigma already supplies to every other application as the desktop portal backend.
 * These wrappers spawn that same picker, so a dialog raised inside Sigma is the dialog Sigma
 * raises everywhere else.
 *
 * They mirror the plugin's `open` and `save` signatures, so a call site changes only its
 * import. Options the picker has no concept of are accepted and ignored rather than rejected,
 * because they are platform hints rather than instructions.
 */

import { invoke } from '@tauri-apps/api/core';
import {
  open as openPlatformDialog,
  save as savePlatformDialog,
  type DialogFilter,
} from '@tauri-apps/plugin-dialog';

export type { DialogFilter };

export interface SigmaOpenDialogOptions {
  title?: string;
  filters?: DialogFilter[];
  defaultPath?: string;
  multiple?: boolean;
  directory?: boolean;
  recursive?: boolean;
  canCreateDirectories?: boolean;
}

export interface SigmaSaveDialogOptions {
  title?: string;
  filters?: DialogFilter[];
  defaultPath?: string;
  canCreateDirectories?: boolean;
}

interface PickerFilter {
  name: string;
  globs: string[];
  mimes: string[];
}

interface PickerRequest {
  title: string;
  multiple: boolean;
  directory: boolean;
  currentFolder: string | null;
  save: boolean;
  suggestedName: string | null;
  filters: PickerFilter[];
  currentFilter: string | null;
}

/**
 * The plugin describes a filter by bare extensions; the picker takes globs and MIME types.
 * An extension of `*` means "everything", which is a filter with no patterns at all rather
 * than one that matches a file literally named `*`.
 */
export function toPickerFilters(filters: DialogFilter[] | undefined): PickerFilter[] {
  return (filters ?? []).map((filter) => {
    const extensions = filter.extensions ?? [];
    const matchesEverything = extensions.some(extension => extension.trim() === '*');

    return {
      name: filter.name,
      globs: matchesEverything
        ? []
        : extensions
            .map(extension => extension.trim().replace(/^[*.]+/, ''))
            .filter(extension => extension.length > 0)
            .map(extension => `*.${extension}`),
      mimes: [],
    };
  });
}

/**
 * Splits `defaultPath` the way each kind of dialog reads it. An open dialog treats it as the
 * folder to start in. A save dialog treats a trailing segment as the suggested filename,
 * unless the caller clearly pointed at a directory.
 */
export function toPickerLocation(
  defaultPath: string | undefined,
  isSaveDialog: boolean,
): {
  currentFolder: string | null;
  suggestedName: string | null;
} {
  if (defaultPath === undefined || defaultPath.length === 0) {
    return {
      currentFolder: null,
      suggestedName: null,
    };
  }

  const normalizedPath = defaultPath.replace(/\\/g, '/');

  if (!isSaveDialog || normalizedPath.endsWith('/')) {
    return {
      currentFolder: defaultPath.replace(/\/+$/, '') || '/',
      suggestedName: null,
    };
  }

  const lastSeparatorIndex = normalizedPath.lastIndexOf('/');

  if (lastSeparatorIndex < 0) {
    return {
      currentFolder: null,
      suggestedName: defaultPath,
    };
  }

  return {
    currentFolder: defaultPath.slice(0, lastSeparatorIndex) || '/',
    suggestedName: defaultPath.slice(lastSeparatorIndex + 1) || null,
  };
}

export function buildPickerRequest(
  options: SigmaOpenDialogOptions & { save?: boolean },
): PickerRequest {
  const isSaveDialog = options.save === true;
  const location = toPickerLocation(options.defaultPath, isSaveDialog);

  return {
    title: options.title ?? '',
    multiple: options.multiple === true,
    directory: options.directory === true,
    currentFolder: location.currentFolder,
    save: isSaveDialog,
    suggestedName: location.suggestedName,
    filters: toPickerFilters(options.filters),
    currentFilter: options.filters?.[0]?.name ?? null,
  };
}

async function runPicker(options: SigmaOpenDialogOptions & { save?: boolean }): Promise<string[]> {
  return invoke<string[]>('file_picker_open', { request: buildPickerRequest(options) });
}

/**
 * Resolves to an array only when `multiple` is literally true, the same way the plugin's
 * return type does, so call sites that pass `multiple: false` keep their narrowed `string`.
 */
export type SigmaOpenDialogReturn<T extends SigmaOpenDialogOptions>
  = T['multiple'] extends true ? string[] | null : string | null;

/**
 * Opens a file or directory dialog. Returns the chosen path, an array when `multiple` is set,
 * or null for a cancel — matching the plugin exactly.
 *
 * If Sigma's picker cannot be started at all, the platform dialog is used rather than leaving
 * the user with a button that does nothing. That is a broken installation, not a preference,
 * so it is logged.
 */
export async function open<T extends SigmaOpenDialogOptions>(
  options?: T,
): Promise<SigmaOpenDialogReturn<T>> {
  const dialogOptions = options ?? ({} as T);
  let paths: string[];

  try {
    paths = await runPicker(dialogOptions);
  }
  catch (error) {
    console.warn('Sigma file picker unavailable, falling back to the platform dialog:', error);
    return openPlatformDialog(dialogOptions) as Promise<SigmaOpenDialogReturn<T>>;
  }

  if (paths.length === 0) {
    return null as SigmaOpenDialogReturn<T>;
  }

  return (dialogOptions.multiple === true ? paths : paths[0]) as SigmaOpenDialogReturn<T>;
}

/** Opens a save dialog. Returns the chosen path, or null for a cancel. */
export async function save(options: SigmaSaveDialogOptions = {}): Promise<string | null> {
  let paths: string[];

  try {
    paths = await runPicker({
      ...options,
      save: true,
    });
  }
  catch (error) {
    console.warn('Sigma file picker unavailable, falling back to the platform dialog:', error);
    return savePlatformDialog(options);
  }

  return paths[0] ?? null;
}
