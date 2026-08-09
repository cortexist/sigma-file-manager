// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

export const UI_CONSTANTS = {
  SMALL_SCREEN_BREAKPOINT: 800,
  FILE_BROWSER_TOOLBAR_ADDRESS_BAR_WRAP_WIDTH: 400,
  FILE_BROWSER_TOOLBAR_NAV_COLLAPSE_WIDTH: 600,
  DOUBLE_CLICK_DELAY: 300,
  WORKSPACE_MAX_PANE_COUNT: 2,
  WORKSPACE_SAVE_DEBOUNCE_MS: 500,
  /**
   * How long the selection must settle before the info panel reads a file's media details.
   * Each read decodes enough of the file to answer, so holding an arrow key through a folder
   * of videos must not start one per entry. Long enough to outlast key repeat, short enough
   * that a deliberate single selection still feels immediate.
   */
  INFO_PANEL_MEDIA_INFO_DEBOUNCE_MS: 150,
  DRAG_ACTIVATION_THRESHOLD: 8,
  DRAG_OVERLAY_OFFSET_X: 16,
  DRAG_OVERLAY_OFFSET_Y: 16,
} as const;

export const DIR_SIZE_CONSTANTS = {
  BATCH_LIMIT: 50,
} as const;

export const SEARCH_CONSTANTS = {
  DEFAULT_RESULT_LIMIT: 50,
  MIN_RESULT_LIMIT: 10,
  MAX_RESULT_LIMIT: 500,
} as const;

export const FILE_EXTENSIONS: Record<string, readonly string[]> = {
  IMAGE: ['jpg', 'jpeg', 'png', 'gif', 'bmp', 'webp', 'svg', 'ico', 'tiff', 'tif'],
  VIDEO: ['mp4', 'mkv', 'avi', 'mov', 'wmv', 'flv', 'webm', 'm4v', 'mpeg', 'mpg'],
  AUDIO: ['mp3', 'wav', 'flac', 'aac', 'ogg', 'wma', 'm4a', 'opus'],
  CODE: ['js', 'ts', 'jsx', 'tsx', 'vue', 'py', 'java', 'cpp', 'c', 'h', 'rs', 'go', 'rb', 'php', 'swift', 'kt', 'cs', 'html', 'css', 'scss', 'sass', 'less', 'json', 'xml', 'yaml', 'yml', 'toml', 'md', 'sh', 'bash', 'ps1', 'sql'],
  ARCHIVE: ['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz', 'iso'],
  TEXT: ['txt', 'log', 'ini', 'cfg', 'conf', 'env'],
};
