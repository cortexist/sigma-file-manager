// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import { invoke } from '@tauri-apps/api/core';

export async function isMediaViewerRegistrationAvailable(): Promise<boolean> {
  return await invoke<boolean>('media_viewer_registration_available');
}

export async function isDefaultMediaViewer(): Promise<boolean> {
  return await invoke<boolean>('is_default_media_viewer');
}

export async function setDefaultMediaViewer(enabled: boolean): Promise<boolean> {
  return await invoke<boolean>('set_default_media_viewer', { enabled });
}
