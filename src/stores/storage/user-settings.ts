// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
// Copyright © 2026 Cortexist, LLC (modifications). All rights reserved.

import cloneDeep from 'lodash.clonedeep';
import { defineStore } from 'pinia';
import { LazyStore } from '@tauri-apps/plugin-store';
import { ref, computed, watch } from 'vue';
import type {
  UserSettings,
  LocalizationLanguage,
  UserSettingsPath,
  UserSettingsValue,
  InfusionPageSettings,
  VisualFiltersSettings,
  Theme,
} from '@/types/user-settings';
import {
  backgroundMedia,
  DEFAULT_BACKGROUND_FILE_NAME,
  DEFAULT_INFUSION_BACKGROUND_FILE_NAME,
} from '@/data/background-media';
import { normalizeThemeSelection } from '@/modules/themes/registry';
import { useTheme } from './composables/use-theme';
import type { ThemeTransitionOrigin } from './composables/use-theme';
import { useUserPathsStore } from './user-paths';
import { useExtensionsStorageStore } from './extensions';
import { i18n } from '@/localization';
import { getLanguage } from '@/localization/data';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  buildAllowedUserSettingsStorageKeys,
  DEFAULT_GLOBAL_SEARCH_IGNORED_PATHS,
  migrateUserSettingsStorage,
  USER_SETTINGS_SCHEMA_VERSION_KEY,
  USER_SETTINGS_SCHEMA_VERSION,
} from '@/stores/schemas/user-settings';
import { SEARCH_CONSTANTS } from '@/constants';
import {
  canUseStartupStorageFastPath,
  getStartupStorageFile,
  getStartupStorageRecord,
  type StartupStorageFileBootstrap,
} from './utils/startup-storage-bootstrap';
import { BUILTIN_NAVIGATOR_ICON_THEME_IDS } from '@/types/icon-theme';

export const USER_SETTINGS_THEME_CHANGED_EVENT = 'user-settings:theme-changed';

export { DEFAULT_ACCENT_COLOR } from '@/stores/storage/composables/use-theme';

/**
 * Appearance that every window has to mirror, not only the one the change was made in.
 *
 * Each window hydrates its own copy of the settings at startup, so a value written here is
 * invisible to the others until they are told. Auxiliary windows are also prelaunched and
 * reused, so one can easily have been created before the user ever opened settings.
 *
 * Both fields are optional: a change to one does not have to restate the other.
 */
type ThemeChangedEventPayload = {
  theme?: Theme;
  /** `null` clears the accent; absent means this broadcast is not about the accent. */
  accentColor?: string | null;
};

export const useUserSettingsStore = defineStore('userSettings', () => {
  const userPathsStore = useUserPathsStore();
  const extensionsStorageStore = useExtensionsStorageStore();

  const userSettingsStorage = ref<LazyStore | null>(null);
  const userSettingsDefault = ref<UserSettings | null>(null);
  const themeTransitionOrigin = ref<ThemeTransitionOrigin | null>(null);
  const themeTransitionsEnabled = ref(false);
  const themeChangeEventUnlisten = ref<UnlistenFn | null>(null);
  const allowedUserSettingsStorageKeys = ref<Set<string>>(new Set());
  const userSettings = ref<UserSettings>({
    language: {
      name: 'English',
      locale: 'en',
      isCorrected: true,
      isRtl: false,
    },
    theme: 'dark',
    // Unset rather than the default color, so a theme supplying its own `--primary` is not
    // overridden on behalf of a user who never chose an accent. See `applyAccentColor`.
    accentColor: null,
    text: {
      font: 'system-ui',
    },
    transparentToolbars: false,
    dateTime: {
      month: 'short',
      regionalFormat: {
        code: 'en',
        name: 'English',
      },
      autoDetectRegionalFormat: true,
      hour12: false,
      showRelativeDates: true,
      properties: {
        showSeconds: false,
        showMilliseconds: false,
      },
    },
    navigator: {
      lastTabCloseBehavior: 'createDefaultTab',
      boldActiveTabTitle: false,
      layout: {
        type: {
          title: 'listLayout',
          name: 'list',
        },
        dirItemOptions: {
          title: {
            height: 32,
          },
          directory: {
            height: 48,
          },
          file: {
            height: 48,
          },
        },
      },
      infoPanel: {
        show: false,
        dynamicSize: false,
        widthPx: null,
        previewHeightPx: null,
        showFullSizeImagePreview: false,
        muteVideoPreviewByDefault: false,
        autoplayVideoPreview: false,
      },
      showHiddenFiles: false,
      splitViewMode: 'split',
      folderIconTheme: BUILTIN_NAVIGATOR_ICON_THEME_IDS.system,
      fileIconTheme: BUILTIN_NAVIGATOR_ICON_THEME_IDS.system,
      listColumnVisibility: {
        kind: true,
        links: false,
        linkTarget: false,
        linkStatus: false,
        items: true,
        size: true,
        modified: true,
        created: false,
        tags: false,
      },
      listColumnFillWidth: true,
      listColumnWidths: {},
      listColumnFlexWeights: {},
      listColumnOrder: ['items', 'size', 'modified', 'created', 'tags', 'kind', 'links', 'linkStatus'],
      listSortColumn: null,
      listSortDirection: 'asc',
      gridSortColumn: 'name',
      gridSortDirection: 'asc',
      enableBoxSelection: false,
      increaseFileViewGaps: false,
    },
    globalSearch: {
      scanDepth: 7,
      autoScanPeriodMinutes: 60,
      autoReindexWhenIdle: true,
      ignoredPaths: [...DEFAULT_GLOBAL_SEARCH_IGNORED_PATHS],
      selectedDriveRoots: [],
      parallelScan: false,
      resultLimit: SEARCH_CONSTANTS.DEFAULT_RESULT_LIMIT,
      includeFiles: true,
      includeDirectories: true,
      exactMatch: false,
      typoTolerance: true,
      lastManualCancelTime: null,
    },
    UIZoomLevel: 1.0,
    showHomeBanner: true,
    homeBannerIndex: 0,
    homeBannerMediaId: DEFAULT_BACKGROUND_FILE_NAME,
    homeBannerPauseVideoWhenIdle: true,
    customBackgroundMedia: [],
    homeBannerPositions: {},
    driveCard: {
      showSpaceIndicator: true,
      spaceIndicatorStyle: 'linearVertical',
    },
    clipboard: {
      showToolbarForExternalImages: true,
      showToolbarForExternalPaths: true,
    },
    userDirectories: {},
    infusion: {
      enabled: true,
      sameSettingsForAllPages: true,
      selectedPageToCustomize: '',
      pauseVideoWhenIdle: true,
      pages: {
        '': createDefaultInfusionPageSettings(),
        'home': createDefaultInfusionPageSettingsHome(),
        'navigator': createDefaultInfusionPageSettings(),
        'dashboard': createDefaultInfusionPageSettings(),
        'settings': createDefaultInfusionPageSettings(),
        'extensions': createDefaultInfusionPageSettings(),
      },
    },
    visualFilters: createDefaultVisualFiltersSettings(),
    settingsCurrentTab: 'general',
    shortcuts: {},
    shortcutUserAlternateChordSlots: {},
    globalShortcuts: {},
    focusWindowOnDriveConnected: true,
    preventDropdownCloseFocusReturn: false,
    quickAccessOnHover: true,
    tooltipDelayMs: 0,
    launchAtStartup: false,
    launchAtStartupHidden: false,
    performance: {
      prelaunchQuickViewWindow: true,
      prelaunchPrintViewWindow: false,
    },
    appUpdates: {
      autoCheck: true,
      lastCheckTimestamp: 0,
    },
    changelog: {
      showOnUpdate: true,
      lastSeenVersion: '',
    },
  });

  function createDefaultInfusionBackground() {
    const media = backgroundMedia.find(item => item.fileName === DEFAULT_INFUSION_BACKGROUND_FILE_NAME)
      ?? backgroundMedia[0];
    const index = backgroundMedia.findIndex(item => item.fileName === media.fileName);

    return {
      type: media.type as 'image' | 'video',
      path: media.fileName,
      index: index >= 0 ? index : 0,
      mediaId: media.fileName,
    };
  }

  function createDefaultInfusionPageSettings(): InfusionPageSettings {
    const background = createDefaultInfusionBackground();

    return {
      blur: 64,
      mediaContrast: 100,
      mediaBrightness: 100,
      opacity: 15,
      noise: 5,
      noiseScale: 0.5,
      mixBlendMode: 'normal',
      background,
    };
  }

  function createDefaultInfusionPageSettingsHome(): InfusionPageSettings {
    const background = createDefaultInfusionBackground();

    return {
      blur: 32,
      mediaContrast: 100,
      mediaBrightness: 100,
      opacity: 5,
      noise: 5,
      noiseScale: 0.5,
      mixBlendMode: 'normal',
      background,
    };
  }

  function createDefaultVisualFiltersSettings(): VisualFiltersSettings {
    return {
      brightness: 100,
      contrast: 100,
      dialogOverlayBlur: 8,
    };
  }

  function clampVisualFilterValue(value: number): number {
    return Math.min(200, Math.max(80, value));
  }

  function clampDialogOverlayBlur(value: number): number {
    const candidate = Number.isFinite(value) ? Math.round(value) : 8;
    return Math.min(32, Math.max(0, candidate));
  }

  function applyDialogOverlayBackdropBlur(blurPixels: number) {
    if (typeof document === 'undefined' || !document.documentElement) {
      return;
    }

    const blur = clampDialogOverlayBlur(blurPixels);
    document.documentElement.style.setProperty('--sigma-dialog-overlay-backdrop-blur', `${blur}px`);
  }

  function applyBodyVisualFilters(visualFilters: Pick<VisualFiltersSettings, 'brightness' | 'contrast'>) {
    if (typeof document === 'undefined' || !document.documentElement) {
      return;
    }

    const brightness = clampVisualFilterValue(visualFilters.brightness);
    const contrast = clampVisualFilterValue(visualFilters.contrast);

    // Still published for `.infusion-image`/`.infusion-video`, which carry a real blur and so
    // are already paying for a filter pass whatever these are set to.
    document.documentElement.style.setProperty('--sigma-visual-filter-brightness', String(brightness));
    document.documentElement.style.setProperty('--sigma-visual-filter-contrast', String(contrast));

    /*
     * `none` rather than an identity filter when nothing is being adjusted. `brightness(100%)
     * contrast(100%)` looks like a no-op but still forces every frame of the page — and every
     * image — through an offscreen surface. See the note in `styles/main.css`.
     */
    const isAdjusted = brightness !== 100 || contrast !== 100;
    const bodyFilter = isAdjusted
      ? `brightness(${brightness}%) contrast(${contrast}%)`
      : 'none';
    // Images undo the body filter so they are shown as authored; with no body filter there is
    // nothing to undo.
    const mediaFilter = isAdjusted
      ? `contrast(${(10000 / contrast).toFixed(3)}%) brightness(${(10000 / brightness).toFixed(3)}%)`
      : 'none';

    document.documentElement.style.setProperty('--sigma-body-filter', bodyFilter);
    document.documentElement.style.setProperty('--sigma-media-filter', mediaFilter);
  }

  const defaultFontFamily = computed(
    () => userSettings.value.text?.font ?? 'system-ui',
  );
  const themeSettingRef = computed(() => userSettings.value.theme);
  const normalizedThemeSelection = computed(() => {
    if (!extensionsStorageStore.isInitialized) {
      return userSettings.value.theme;
    }

    return normalizeThemeSelection(
      userSettings.value.theme,
      extensionsStorageStore.extensionsData.installedExtensions,
    );
  });
  // Passed through unresolved: `useTheme` needs to tell "never chosen" from "chose the
  // default color", so collapsing null to a concrete value here would lose the distinction.
  const accentColorRef = computed(() => userSettings.value.accentColor);
  const { setTheme } = useTheme(
    themeSettingRef,
    themeTransitionOrigin,
    themeTransitionsEnabled,
    accentColorRef,
  );

  watch(
    () => [
      userSettings.value.visualFilters.brightness,
      userSettings.value.visualFilters.contrast,
      userSettings.value.visualFilters.dialogOverlayBlur,
    ] as const,
    ([brightness, contrast, dialogOverlayBlur]) => {
      applyBodyVisualFilters({
        brightness,
        contrast,
      });
      applyDialogOverlayBackdropBlur(dialogOverlayBlur);
    },
    { immediate: true },
  );

  watch(normalizedThemeSelection, (normalizedTheme) => {
    if (!extensionsStorageStore.isInitialized || normalizedTheme === userSettings.value.theme) {
      return;
    }

    void set('theme', normalizedTheme);
  });

  function applyUserSettingsEntries(settingsEntries: Iterable<[string, unknown]>) {
    for (const [key, value] of settingsEntries) {
      if (key === USER_SETTINGS_SCHEMA_VERSION_KEY) {
        continue;
      }

      const normalizedKey = normalizeStorageKeyToMemory(key);

      if (!allowedUserSettingsStorageKeys.value.has(normalizedKey)) {
        continue;
      }

      setNestedValue(userSettings.value as Record<string, unknown>, normalizedKey, value);
    }
  }

  async function loadUserSettings() {
    try {
      const settings = await userSettingsStorage.value?.entries();

      if (!settings || settings.length === 0) {
        return;
      }

      applyUserSettingsEntries(settings);
    }
    catch (error) {
      console.error('Failed to load user settings:', error);
    }
  }

  function normalizeStorageKeyToMemory(key: string): string {
    return key.replace('infusion.pages.global.', 'infusion.pages..');
  }

  function setNestedValue(obj: Record<string, unknown>, path: string, value: unknown) {
    const keys = path.split('.');
    let current = obj;

    for (let keyIndex = 0; keyIndex < keys.length - 1; keyIndex++) {
      const key = keys[keyIndex];

      if (current[key] === undefined || typeof current[key] !== 'object') {
        current[key] = {};
      }

      current = current[key] as Record<string, unknown>;
    }

    current[keys[keys.length - 1]] = value;
  }

  async function initUserSettings() {
    try {
      if (!userSettingsStorage.value) {
        userSettingsStorage.value = await new LazyStore(userPathsStore.customPaths.appUserDataSettingsPath);
        await userSettingsStorage.value.save();
      }

      if (!userSettingsDefault.value) {
        userSettingsDefault.value = cloneDeep(userSettings.value);
      }

      if (userSettingsDefault.value && allowedUserSettingsStorageKeys.value.size === 0) {
        allowedUserSettingsStorageKeys.value = buildAllowedUserSettingsStorageKeys(userSettingsDefault.value);
      }
    }
    catch (error) {
      console.error('Failed to initialize user settings storage:', error);
    }
  }

  async function setUserSettingsStorage(key: string, value: unknown) {
    try {
      if (userSettingsStorage.value) {
        await userSettingsStorage.value.set(key, value);
        await userSettingsStorage.value.save();
      }
    }
    catch (error: unknown) {
      console.error(`Failed to save to storage: ${key}: ${value}`, error);
    }
  }

  async function broadcastAppearanceChange(payload: ThemeChangedEventPayload) {
    try {
      await emit(USER_SETTINGS_THEME_CHANGED_EVENT, payload);
    }
    catch (error) {
      console.error('Failed to broadcast appearance change:', error);
    }
  }

  async function ensureThemeChangeListener() {
    if (themeChangeEventUnlisten.value) {
      return;
    }

    try {
      themeChangeEventUnlisten.value = await listen<ThemeChangedEventPayload>(
        USER_SETTINGS_THEME_CHANGED_EVENT,
        (event) => {
          const { theme, accentColor } = event.payload;

          // Assigning only on a real change keeps the emitting window from reacting to its
          // own broadcast, which `emit` also delivers locally.
          if (theme !== undefined && theme !== userSettings.value.theme) {
            userSettings.value.theme = theme;
          }

          if (accentColor !== undefined && accentColor !== userSettings.value.accentColor) {
            userSettings.value.accentColor = accentColor;
          }
        },
      );
    }
    catch (error) {
      console.error('Failed to listen for theme changes:', error);
    }
  }

  function applyTextDirection(locale: string) {
    // Resolve the canonical language definition so the text direction stays
    // correct even if older stored settings predate the `isRtl` flag.
    const language = getLanguage(locale);
    const isRtl = language?.isRtl ?? false;
    const root = document.documentElement;
    root.setAttribute('dir', isRtl ? 'rtl' : 'ltr');
    root.setAttribute('lang', locale);
  }

  async function setLanguage(newLanguage: LocalizationLanguage) {
    userSettings.value.language = newLanguage;
    i18n.global.locale.value = newLanguage.locale as typeof i18n.global.locale.value;
    applyTextDirection(newLanguage.locale);
    await setUserSettingsStorage('language', newLanguage);
  }

  function initTheme() {
    setTheme(userSettings.value.theme);
  }

  function setThemeTransitionOrigin(origin: ThemeTransitionOrigin | null) {
    themeTransitionOrigin.value = origin;
  }

  function initLanguage() {
    i18n.global.locale.value = userSettings.value.language.locale as typeof i18n.global.locale.value;
    applyTextDirection(userSettings.value.language.locale);
  }

  async function initZoom() {
    const webview = getCurrentWebview();
    const zoomLevel = userSettings.value.UIZoomLevel ?? 1.0;
    await webview.setZoom(zoomLevel);
  }

  async function toggleInfoPanel() {
    userSettings.value.navigator.infoPanel.show = !userSettings.value.navigator.infoPanel.show;
    await setUserSettingsStorage('navigator.infoPanel.show', userSettings.value.navigator.infoPanel.show);
  }

  async function set<P extends UserSettingsPath>(key: P, value: UserSettingsValue<P>) {
    const keys = key.split('.');
    let current: Record<string, unknown> = userSettings.value as Record<string, unknown>;

    for (let keyIndex = 0; keyIndex < keys.length - 1; keyIndex++) {
      current = current[keys[keyIndex]] as Record<string, unknown>;
    }

    current[keys[keys.length - 1]] = value;

    // Both feed `--primary` and the theme class on the document root, which every window
    // maintains for itself. Anything else here is read from storage on next launch.
    if (key === 'theme') {
      await broadcastAppearanceChange({ theme: value as Theme });
    }
    else if (key === 'accentColor') {
      // `null` is a real value here — clearing the accent has to reach the other windows too.
      await broadcastAppearanceChange({ accentColor: value as string | null });
    }

    await setUserSettingsStorage(key, value);
  }

  function hydrateUserSettingsFromBootstrap(bootstrapFile?: StartupStorageFileBootstrap): boolean {
    if (!canUseStartupStorageFastPath(bootstrapFile, USER_SETTINGS_SCHEMA_VERSION)) {
      return false;
    }

    const bootstrapRecord = getStartupStorageRecord(bootstrapFile);

    if (bootstrapRecord) {
      applyUserSettingsEntries(Object.entries(bootstrapRecord));
    }

    return true;
  }

  async function init(bootstrapFile?: StartupStorageFileBootstrap) {
    const resolvedBootstrapFile = bootstrapFile ?? await getStartupStorageFile('userSettings');
    await initUserSettings();
    const loadedFromBootstrap = hydrateUserSettingsFromBootstrap(resolvedBootstrapFile);

    if (!loadedFromBootstrap && userSettingsStorage.value) {
      try {
        await migrateUserSettingsStorage(userSettingsStorage.value);
      }
      catch (error) {
        console.error('[UserSettings] Migration failed, loading with current storage state:', error);
      }
    }

    if (!loadedFromBootstrap) {
      await loadUserSettings();
    }

    initTheme();
    themeTransitionsEnabled.value = true;
    await ensureThemeChangeListener();
    initLanguage();
    await initZoom();
  }

  return {
    userSettings,
    userSettingsDefault,
    defaultFontFamily,
    init,
    set,
    setUserSettingsStorage,
    setLanguage,
    setThemeTransitionOrigin,
    toggleInfoPanel,
  };
});
