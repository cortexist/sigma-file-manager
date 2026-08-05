// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import {
  computed,
  toValue,
  type MaybeRefOrGetter,
} from 'vue';
import { isVideoFile as checkIsVideo } from '@/modules/navigator/components/file-browser/utils';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import type { DirEntry } from '@/types/dir-entry';

export function useInfoPanelVideoPreview(selectedEntry: MaybeRefOrGetter<DirEntry | null>) {
  const userSettingsStore = useUserSettingsStore();

  const isVideoFile = computed(() => {
    const entry = toValue(selectedEntry);

    if (!entry) {
      return false;
    }

    return checkIsVideo(entry);
  });

  const muteVideoPreviewByDefault = computed(
    () => userSettingsStore.userSettings.navigator.infoPanel.muteVideoPreviewByDefault,
  );

  const autoplayVideoPreview = computed(
    () => userSettingsStore.userSettings.navigator.infoPanel.autoplayVideoPreview,
  );

  return {
    isVideoFile,
    muteVideoPreviewByDefault,
    autoplayVideoPreview,
  };
}
