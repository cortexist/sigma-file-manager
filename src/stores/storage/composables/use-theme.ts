// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import {
  ref, computed, type Ref, type ComputedRef, watchEffect,
} from 'vue';
import type { Theme } from '@/types/user-settings';
import { findThemeOption, parseThemeId } from '@/modules/themes/registry';
import { useExtensionsStorageStore } from '@/stores/storage/extensions';

export type ThemeTransitionOrigin = {
  x: number;
  y: number;
};

type ViewTransitionDocument = Document & {
  startViewTransition?: (callback: () => void) => {
    ready: Promise<void>;
    skipTransition: () => void;
  };
};

/**
 * Used when the user has not picked an accent and the active theme does not supply one.
 *
 * Lives here rather than in the settings store because `applyAccentColor` needs it and the
 * store already imports this module — the other direction would be a cycle. The store
 * re-exports it, so existing imports are unaffected.
 */
export const DEFAULT_ACCENT_COLOR = '198 19% 38%';

const THEME_TRANSITION_DURATION_MS = 500;
let activeViewTransition: ReturnType<NonNullable<ViewTransitionDocument['startViewTransition']>> | null = null;
let activeViewTransitionAnimation: Animation | null = null;
let activeViewTransitionId = 0;

export function useTheme(
  themeSettingRef: Ref<Theme> | ComputedRef<Theme>,
  transitionOriginRef?: Ref<ThemeTransitionOrigin | null> | ComputedRef<ThemeTransitionOrigin | null>,
  transitionsEnabledRef?: Ref<boolean> | ComputedRef<boolean>,
  /** `null` means the user has never picked one, which is not the same as picking the default. */
  accentColorRef?: Ref<string | null> | ComputedRef<string | null>,
) {
  const extensionsStorageStore = useExtensionsStorageStore();
  const currentTheme = ref<'light' | 'dark'>('dark');
  const isDark = computed(() => currentTheme.value === 'dark');
  const appliedThemeVariables = new Set<string>();
  let hasAppliedTheme = false;
  let appliedTheme: Theme | null = null;

  function getSystemPreference(): 'light' | 'dark' {
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  }

  function clearThemeVariables() {
    if (typeof document === 'undefined' || !document.documentElement) {
      return;
    }

    for (const variableName of appliedThemeVariables) {
      document.documentElement.style.removeProperty(variableName);
    }

    appliedThemeVariables.clear();
  }

  function applyBaseTheme(theme: 'light' | 'dark') {
    currentTheme.value = theme;
    document.documentElement.classList.toggle('dark', currentTheme.value === 'dark');
  }

  function resolveBuiltinTheme(theme: 'light' | 'dark' | 'system'): 'light' | 'dark' {
    return theme === 'system' ? getSystemPreference() : theme;
  }

  function getTransitionOrigin(): ThemeTransitionOrigin {
    return transitionOriginRef?.value ?? {
      x: window.innerWidth,
      y: 0,
    };
  }

  function animateViewTransition(transitionId: number) {
    if (transitionId !== activeViewTransitionId) {
      return;
    }

    const { x, y } = getTransitionOrigin();
    const maxRadius = Math.hypot(
      Math.max(x, window.innerWidth - x),
      Math.max(y, window.innerHeight - y),
    );

    activeViewTransitionAnimation = document.documentElement.animate(
      {
        clipPath: [
          `circle(0px at ${x}px ${y}px)`,
          `circle(${maxRadius}px at ${x}px ${y}px)`,
        ],
      },
      {
        duration: THEME_TRANSITION_DURATION_MS,
        easing: 'ease-in-out',
        pseudoElement: '::view-transition-new(root)',
      },
    );

    activeViewTransitionAnimation.finished
      .catch(() => undefined)
      .finally(() => {
        if (transitionId === activeViewTransitionId) {
          activeViewTransitionAnimation = null;
          activeViewTransition = null;
        }
      });
  }

  function canUseViewTransition(): boolean {
    if (typeof document === 'undefined' || typeof window === 'undefined') {
      return false;
    }

    const viewTransitionDocument = document as ViewTransitionDocument;

    return typeof window.matchMedia === 'function'
      && typeof viewTransitionDocument.startViewTransition === 'function'
      && typeof document.documentElement.animate === 'function'
      && !window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  }

  function cancelActiveViewTransition() {
    activeViewTransitionAnimation?.cancel();
    activeViewTransitionAnimation = null;
    activeViewTransition?.skipTransition();
    activeViewTransition = null;
  }

  function runThemeTransition(applyThemeChange: () => void) {
    if (!canUseViewTransition()) {
      applyThemeChange();
      return;
    }

    cancelActiveViewTransition();

    const viewTransitionDocument = document as ViewTransitionDocument;
    const transitionId = activeViewTransitionId + 1;
    activeViewTransitionId = transitionId;

    try {
      activeViewTransition = viewTransitionDocument.startViewTransition?.(applyThemeChange) ?? null;
    }
    catch {
      activeViewTransition = null;
      applyThemeChange();
      return;
    }

    activeViewTransition?.ready
      .then(() => animateViewTransition(transitionId))
      .catch(() => undefined);
  }

  function applyTheme(theme: Theme) {
    clearThemeVariables();

    const themeOption = findThemeOption(
      theme,
      extensionsStorageStore.extensionsData.installedExtensions,
    );

    if (!themeOption || themeOption.source === 'builtin') {
      const parsedTheme = parseThemeId(theme);
      const resolvedTheme = parsedTheme?.source === 'builtin'
        ? resolveBuiltinTheme(parsedTheme.builtinThemeId)
        : 'dark';

      applyBaseTheme(resolvedTheme);
      return;
    }

    applyBaseTheme(themeOption.baseTheme);

    for (const [variableName, variableValue] of Object.entries(themeOption.variables)) {
      document.documentElement.style.setProperty(variableName, variableValue);
      appliedThemeVariables.add(variableName);
    }
  }

  /**
   * Written as an inline custom property on the root, which outranks both the `.dark` and
   * light blocks in the stylesheet, so one value covers every theme. Applied after the theme
   * so a theme that also defines `--primary` does not overwrite the user's choice.
   *
   * Three cases, in priority order:
   *
   * 1. The user picked an accent — it wins over everything, which is the point of the setting.
   * 2. No pick, but the active theme defines `--primary` — the theme keeps it. This used to be
   *    clobbered: the setting defaulted to a concrete colour rather than to "unset", so a
   *    theme's accent was overwritten even by a user who had never opened the setting.
   * 3. Neither — fall back to the default, so the app looks the same as it always has instead
   *    of dropping through to whatever the stylesheet happens to define.
   */
  function applyAccentColor() {
    if (typeof document === 'undefined' || !document.documentElement) {
      return;
    }

    const accentColor = accentColorRef?.value?.trim();

    if (accentColor) {
      document.documentElement.style.setProperty('--primary', accentColor);
      return;
    }

    if (appliedThemeVariables.has('--primary')) {
      return;
    }

    document.documentElement.style.setProperty('--primary', DEFAULT_ACCENT_COLOR);
  }

  function setTheme(theme: Theme) {
    if ((transitionsEnabledRef?.value ?? true) && hasAppliedTheme && theme !== appliedTheme) {
      runThemeTransition(() => applyTheme(theme));
    }
    else {
      applyTheme(theme);
    }

    hasAppliedTheme = true;
    applyAccentColor();
    appliedTheme = theme;
  }

  function toggleTheme() {
    return setTheme(currentTheme.value === 'dark' ? 'light' : 'dark');
  }

  function handleSystemThemeChange(event: MediaQueryListEvent) {
    const parsedTheme = parseThemeId(themeSettingRef.value);

    if (parsedTheme?.source === 'builtin' && parsedTheme.builtinThemeId === 'system') {
      if (transitionsEnabledRef?.value ?? true) {
        runThemeTransition(() => applyBaseTheme(event.matches ? 'dark' : 'light'));
      }
      else {
        applyBaseTheme(event.matches ? 'dark' : 'light');
      }
    }
  }

  function init() {
    setTheme(themeSettingRef.value);
    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
    mediaQuery.addEventListener('change', handleSystemThemeChange);
  }

  watchEffect(() => {
    setTheme(themeSettingRef.value);
  });

  watchEffect(() => {
    void accentColorRef?.value;
    applyAccentColor();
  });

  init();

  return {
    isDark,
    currentTheme,
    toggleTheme,
    setTheme,
  };
}
