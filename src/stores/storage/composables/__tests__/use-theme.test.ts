// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
// Copyright © 2026 Cortexist, LLC (modifications). All rights reserved.

import { nextTick, ref } from 'vue';
import {
  beforeEach, describe, expect, it, vi,
} from 'vitest';
import type { InstalledExtensionData } from '@/types/extension';
import type { Theme } from '@/types/user-settings';

const extensionsData = {
  installedExtensions: {} as Record<string, InstalledExtensionData>,
};

vi.mock('@/stores/storage/extensions', () => ({
  useExtensionsStorageStore: () => ({
    extensionsData,
  }),
}));

import { DEFAULT_ACCENT_COLOR, useTheme } from '@/stores/storage/composables/use-theme';

function createInstalledExtensionData(): InstalledExtensionData {
  return {
    version: '1.0.0',
    enabled: true,
    autoUpdate: true,
    installedAt: 1,
    manifest: {
      id: 'test.palette',
      name: 'Test Palette',
      version: '1.0.0',
      repository: 'https://github.com/example/test-palette',
      license: 'MIT',
      extensionType: 'api',
      main: 'index.js',
      permissions: [],
      contributes: {
        themes: [
          {
            id: 'midnight',
            title: 'Midnight',
            baseTheme: 'dark',
            variables: {
              '--primary': '200 80% 60%',
            },
          },
        ],
      },
      engines: {
        sigmaFileManager: '>=2.0.0',
      },
    },
    settings: {
      scopedDirectories: [],
      customSettings: {},
    },
  };
}

describe('useTheme', () => {
  beforeEach(() => {
    document.documentElement.className = '';
    document.documentElement.style.cssText = '';
    extensionsData.installedExtensions = {};
    Object.defineProperty(window, 'matchMedia', {
      configurable: true,
      writable: true,
      value: vi.fn(() => ({
        matches: false,
        addEventListener: vi.fn(),
      })),
    });
    Object.defineProperty(document, 'startViewTransition', {
      configurable: true,
      writable: true,
      value: undefined,
    });
    Object.defineProperty(document.documentElement, 'animate', {
      configurable: true,
      writable: true,
      value: undefined,
    });
  });

  it('applies extension theme variables and removes them when switching away', async () => {
    extensionsData.installedExtensions = {
      'test.palette': createInstalledExtensionData(),
    };

    const theme = ref<Theme>('extension:test.palette:midnight');
    const { currentTheme } = useTheme(theme);

    expect(currentTheme.value).toBe('dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);
    expect(document.documentElement.style.getPropertyValue('--primary')).toBe('200 80% 60%');

    theme.value = 'light';
    await nextTick();

    expect(currentTheme.value).toBe('light');
    expect(document.documentElement.classList.contains('dark')).toBe(false);
    // The theme's accent is gone, so the default takes over rather than leaving `--primary`
    // unset and dropping through to whatever the stylesheet defines.
    expect(document.documentElement.style.getPropertyValue('--primary')).toBe(
      DEFAULT_ACCENT_COLOR,
    );
  });

  describe('accent color priority', () => {
    /**
     * The setting used to default to a concrete color, so this path was unreachable: a
     * theme's accent was overwritten even for a user who had never opened the setting.
     */
    it('lets an unset accent defer to one supplied by the theme', () => {
      extensionsData.installedExtensions = {
        'test.palette': createInstalledExtensionData(),
      };

      useTheme(
        ref<Theme>('extension:test.palette:midnight'),
        undefined,
        undefined,
        ref<string | null>(null),
      );

      expect(document.documentElement.style.getPropertyValue('--primary')).toBe('200 80% 60%');
    });

    it('lets a chosen accent override one supplied by the theme', () => {
      extensionsData.installedExtensions = {
        'test.palette': createInstalledExtensionData(),
      };

      useTheme(
        ref<Theme>('extension:test.palette:midnight'),
        undefined,
        undefined,
        ref<string | null>('12 100% 50%'),
      );

      expect(document.documentElement.style.getPropertyValue('--primary')).toBe('12 100% 50%');
    });

    it('falls back to the default when neither the user nor the theme supplies one', () => {
      useTheme(ref<Theme>('dark'), undefined, undefined, ref<string | null>(null));

      expect(document.documentElement.style.getPropertyValue('--primary')).toBe(
        DEFAULT_ACCENT_COLOR,
      );
    });

    it('tracks a later accent choice', async () => {
      const accentColor = ref<string | null>(null);
      useTheme(ref<Theme>('dark'), undefined, undefined, accentColor);

      accentColor.value = '12 100% 50%';
      await nextTick();

      expect(document.documentElement.style.getPropertyValue('--primary')).toBe('12 100% 50%');
    });
  });

  it('uses a view transition after initial theme changes', async () => {
    const skipTransitionMock = vi.fn();
    const animateMock = vi.fn(() => ({
      cancel: vi.fn(),
      finished: Promise.resolve(),
    }));
    const startViewTransitionMock = vi.fn((callback: () => void) => {
      callback();
      return {
        ready: Promise.resolve(),
        skipTransition: skipTransitionMock,
      };
    });

    Object.defineProperty(document, 'startViewTransition', {
      configurable: true,
      writable: true,
      value: startViewTransitionMock,
    });
    Object.defineProperty(document.documentElement, 'animate', {
      configurable: true,
      writable: true,
      value: animateMock,
    });

    const theme = ref<Theme>('dark');
    useTheme(theme);

    expect(startViewTransitionMock).not.toHaveBeenCalled();

    theme.value = 'light';
    await nextTick();

    expect(startViewTransitionMock).toHaveBeenCalledTimes(1);
    expect(document.documentElement.classList.contains('dark')).toBe(false);

    await Promise.resolve();

    expect(animateMock).toHaveBeenCalledWith(
      expect.objectContaining({
        clipPath: expect.arrayContaining([
          expect.stringContaining('circle(0px at '),
        ]),
      }),
      expect.objectContaining({
        duration: 500,
        easing: 'ease-in-out',
        pseudoElement: '::view-transition-new(root)',
      }),
    );
  });

  it('does not use a view transition while transitions are disabled', async () => {
    const animateMock = vi.fn(() => ({
      cancel: vi.fn(),
      finished: Promise.resolve(),
    }));
    const startViewTransitionMock = vi.fn((callback: () => void) => {
      callback();

      return {
        ready: Promise.resolve(),
        skipTransition: vi.fn(),
      };
    });

    Object.defineProperty(document, 'startViewTransition', {
      configurable: true,
      writable: true,
      value: startViewTransitionMock,
    });
    Object.defineProperty(document.documentElement, 'animate', {
      configurable: true,
      writable: true,
      value: animateMock,
    });

    const theme = ref<Theme>('dark');
    const transitionsEnabled = ref(false);
    useTheme(theme, undefined, transitionsEnabled);

    theme.value = 'light';
    await nextTick();

    expect(startViewTransitionMock).not.toHaveBeenCalled();
    expect(document.documentElement.classList.contains('dark')).toBe(false);

    transitionsEnabled.value = true;
    theme.value = 'dark';
    await nextTick();

    expect(startViewTransitionMock).toHaveBeenCalledTimes(1);
    expect(document.documentElement.classList.contains('dark')).toBe(true);
  });

  it('interrupts active view transitions for rapid theme changes', async () => {
    const firstSkipTransitionMock = vi.fn();
    const secondSkipTransitionMock = vi.fn();
    const firstAnimationCancelMock = vi.fn();
    const readyPromises = [
      Promise.resolve(),
      Promise.resolve(),
    ];
    const animateMock = vi.fn(() => ({
      cancel: firstAnimationCancelMock,
      finished: new Promise<void>(() => undefined),
    }));
    const startViewTransitionMock = vi.fn((callback: () => void) => {
      const callIndex = startViewTransitionMock.mock.calls.length - 1;
      callback();

      return {
        ready: readyPromises[callIndex],
        skipTransition: callIndex === 0 ? firstSkipTransitionMock : secondSkipTransitionMock,
      };
    });

    Object.defineProperty(document, 'startViewTransition', {
      configurable: true,
      writable: true,
      value: startViewTransitionMock,
    });
    Object.defineProperty(document.documentElement, 'animate', {
      configurable: true,
      writable: true,
      value: animateMock,
    });

    const theme = ref<Theme>('dark');
    useTheme(theme);

    theme.value = 'light';
    await nextTick();
    await Promise.resolve();

    theme.value = 'dark';
    await nextTick();

    expect(startViewTransitionMock).toHaveBeenCalledTimes(2);
    expect(firstSkipTransitionMock).toHaveBeenCalledTimes(1);
    expect(firstAnimationCancelMock).toHaveBeenCalledTimes(1);
    expect(document.documentElement.classList.contains('dark')).toBe(true);
  });
});

describe('the focus ring follows the accent', () => {
  beforeEach(() => {
    document.documentElement.className = '';
    document.documentElement.style.cssText = '';
    extensionsData.installedExtensions = {};
    Object.defineProperty(window, 'matchMedia', {
      configurable: true,
      writable: true,
      value: vi.fn(() => ({
        matches: false,
        addEventListener: vi.fn(),
      })),
    });
    Object.defineProperty(document, 'startViewTransition', {
      configurable: true,
      writable: true,
      value: undefined,
    });
    Object.defineProperty(document.documentElement, 'animate', {
      configurable: true,
      writable: true,
      value: undefined,
    });
  });

  /**
   * The regression this exists for: `--ring` was a fixed near-white grey, so focusing a
   * field lit it up in white while other parts of the window used the accent. Users read
   * that as a fault, not as two conventions — and it was most obvious in extension forms,
   * which had no way to opt into the accent.
   */
  it('uses the accent the user picked', async () => {
    const theme = ref<Theme>('dark');
    useTheme(theme, undefined, undefined, ref('330 100% 50%'));
    await nextTick();

    expect(document.documentElement.style.getPropertyValue('--ring')).toBe('330 100% 50%');
    expect(document.documentElement.style.getPropertyValue('--primary')).toBe('330 100% 50%');
  });

  it('falls back to the default accent when the user has picked none', async () => {
    const theme = ref<Theme>('dark');
    useTheme(theme, undefined, undefined, ref(null));
    await nextTick();

    expect(document.documentElement.style.getPropertyValue('--ring')).toBe(DEFAULT_ACCENT_COLOR);
  });

  it('follows a theme\'s own primary when the user has picked no accent', async () => {
    extensionsData.installedExtensions = { 'test.palette': createInstalledExtensionData() };

    const theme = ref<Theme>('extension:test.palette:midnight');
    useTheme(theme, undefined, undefined, ref(null));
    await nextTick();

    expect(document.documentElement.style.getPropertyValue('--ring')).toBe('200 80% 60%');
  });

  /** A theme that names its own ring keeps it, the same rule that already governs primary. */
  it('leaves a ring the theme defined for itself', async () => {
    const extension = createInstalledExtensionData();
    extension.manifest.contributes!.themes![0].variables = {
      '--primary': '200 80% 60%',
      '--ring': '0 0% 100%',
    };
    extensionsData.installedExtensions = { 'test.palette': extension };

    const theme = ref<Theme>('extension:test.palette:midnight');
    useTheme(theme, undefined, undefined, ref('330 100% 50%'));
    await nextTick();

    expect(document.documentElement.style.getPropertyValue('--ring')).toBe('0 0% 100%');
    expect(document.documentElement.style.getPropertyValue('--primary')).toBe('330 100% 50%');
  });

  it('tracks a later change to the accent', async () => {
    const theme = ref<Theme>('dark');
    const accent = ref<string | null>('330 100% 50%');
    useTheme(theme, undefined, undefined, accent);
    await nextTick();

    accent.value = '120 60% 45%';
    await nextTick();

    expect(document.documentElement.style.getPropertyValue('--ring')).toBe('120 60% 45%');
  });
});
