// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import {
  computed, onUnmounted, ref, toValue, watch,
} from 'vue';
import type { MaybeRefOrGetter } from 'vue';
import { useDocumentVisibility } from '@vueuse/core';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import { isRelativeDateDisplayEnabled } from '@/utils/relative-date-display';

export function useRelativeDateDisplay(relativeDisplay: MaybeRefOrGetter<boolean> = true) {
  const userSettingsStore = useUserSettingsStore();

  const isEnabled = computed(() => {
    return isRelativeDateDisplayEnabled(
      userSettingsStore.userSettings.dateTime.showRelativeDates,
      toValue(relativeDisplay),
    );
  });

  return { isEnabled };
}

/**
 * How often the reference time is refreshed for relative labels.
 *
 * A minute is the finest thing these labels say, so a tick per second produced fifty-nine
 * re-renders that could not change a word. This is fast enough that a label crosses its minute
 * boundary while the eye is still on it, and slow enough to stop being a heartbeat.
 */
const RELATIVE_CLOCK_INTERVAL_MS = 15 * 1000;

export function useRelativeDateDisplayClock(trackRelativeTime: MaybeRefOrGetter<boolean> = true) {
  const documentVisibility = useDocumentVisibility();
  const clockRef = ref(Date.now());
  let intervalId: ReturnType<typeof setInterval> | undefined;
  const { isEnabled } = useRelativeDateDisplay(trackRelativeTime);

  function clearClockInterval(): void {
    if (intervalId !== undefined) {
      clearInterval(intervalId);
      intervalId = undefined;
    }
  }

  const shouldRunRelativeClock = computed(() => {
    return isEnabled.value && documentVisibility.value === 'visible';
  });

  watch(
    shouldRunRelativeClock,
    (enabled) => {
      clearClockInterval();

      if (enabled) {
        clockRef.value = Date.now();
        intervalId = setInterval(() => {
          clockRef.value = Date.now();
        }, RELATIVE_CLOCK_INTERVAL_MS);
      }
    },
    { immediate: true },
  );

  onUnmounted(() => {
    clearClockInterval();
  });

  return {
    clockRef,
    isEnabled,
    shouldRunRelativeClock,
  };
}
