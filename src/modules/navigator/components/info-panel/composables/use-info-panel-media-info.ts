// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import {
  ref,
  toValue,
  watch,
  type MaybeRefOrGetter,
} from 'vue';
import { useDebounceFn } from '@vueuse/core';
import { UI_CONSTANTS } from '@/constants';
import { determineFileType } from '@/stores/runtime/quick-view';
import { readMediaInfo, summarizeMediaInfo, type MediaInfoRow } from '@/utils/media-info';

/**
 * What the selected file is made of, for the panel to list beside its size and dates.
 *
 * Two things shape this. Reading is not free — the backend opens the file and decodes enough of
 * it to answer — so the selection has to settle before anything is read, or holding an arrow key
 * through a folder of videos would start a read per entry. And the answer can arrive after the
 * selection has moved on, so each read carries a token and a late one is dropped rather than
 * shown against the wrong file.
 *
 * Rows are cleared the moment the selection changes rather than when its replacement arrives:
 * briefly showing nothing is honest, while briefly showing the previous file's numbers is not.
 */
export function useInfoPanelMediaInfo(path: MaybeRefOrGetter<string | undefined>) {
  const rows = ref<MediaInfoRow[]>([]);
  let latestRequest = 0;

  async function read(target: string, token: number) {
    try {
      const info = await readMediaInfo(target);

      if (token === latestRequest) {
        rows.value = summarizeMediaInfo(info);
      }
    }
    catch {
      // A format the backend cannot read contributes no rows. The rest of the panel is
      // unaffected, and an "unavailable" line here would be noise beside the size and dates.
      if (token === latestRequest) {
        rows.value = [];
      }
    }
  }

  const readWhenSettled = useDebounceFn(
    read,
    UI_CONSTANTS.INFO_PANEL_MEDIA_INFO_DEBOUNCE_MS,
  );

  watch(() => toValue(path), (target) => {
    // Bumping here rather than inside the debounced call means a read already in flight is
    // disowned the instant the selection moves, not when its replacement is finally issued.
    const token = ++latestRequest;
    rows.value = [];

    if (!target) return;

    const kind = determineFileType(target);

    if (kind !== 'video' && kind !== 'audio' && kind !== 'image') return;

    void readWhenSettled(target, token);
  }, { immediate: true });

  return { rows };
}
