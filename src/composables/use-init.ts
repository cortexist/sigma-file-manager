// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { nextTick, onMounted, onUnmounted } from 'vue';
import { useRouter } from 'vue-router';
import { useUserPathsStore } from '@/stores/storage/user-paths';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import { useUserStatsStore } from '@/stores/storage/user-stats';
import { useWorkspacesStore } from '@/stores/storage/workspaces';
import { usePlatformStore } from '@/stores/runtime/platform';
import { useGlobalSearchStore } from '@/stores/runtime/global-search';
import { useAppWindowStore } from '@/stores/runtime/app-window';
import {
  BUILTIN_NAVIGATION_PAGE_SHORTCUTS,
  useShortcutsStore,
} from '@/stores/runtime/shortcuts';
import { useGlobalShortcutsStore } from '@/stores/runtime/global-shortcuts';
import { useTerminalsStore } from '@/stores/runtime/terminals';
import { useBackgroundMediaStore } from '@/stores/runtime/background-media';
import { disableWebViewFeatures } from '@/utils/disable-web-view-features';
import { useAppUpdater } from '@/modules/app-updater';
import { useExtensionsStore } from '@/stores/runtime/extensions';
import { useArchiveJobsStore } from '@/stores/runtime/archive-jobs';
import { useDeleteJobsStore } from '@/stores/runtime/delete-jobs';
import { useCopyMoveJobsStore } from '@/stores/runtime/copy-move-jobs';
import { OPEN_MEDIA_REQUEST_EVENT, useQuickViewStore } from '@/stores/runtime/quick-view';
import { getParentDirectory } from '@/utils/normalize-path';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { SIGMA_AUTOSTART_CLI_FLAG } from '@/constants/autostart';
import { applyLaunchAtStartupPreference } from '@/utils/autostart-sync';
import {
  prelaunchConfiguredAuxiliaryWindows,
  setupAuxiliaryWindowLifecycle,
} from '@/utils/auxiliary-windows';
import {
  resolveLaunchTargetsFromArgs,
  type LaunchContext,
} from '@/utils/launch-directories';
import { applyUiZoomStep } from '@/utils/ui-zoom';
import { useClipboardFocusSync } from '@/composables/use-clipboard-focus-sync';
import { toggleMainWindowFullscreen } from '@/utils/window-fullscreen';
import { removeAppSplash } from '@/utils/app-splash';
import { logInitTrace, traceInitStep } from '@/utils/init-trace';
import { warmPathComparisonVolumeCache } from '@/utils/path-comparison-volume-cache';
import { preloadNavigatorRoute } from '@/utils/open-navigator-directory';

const APP_LAUNCH_ARGS_EVENT = 'app-launch-args';
/** "Show in Folder" requests from other applications, via the FileManager1 DBus service. */
const SHOW_IN_FOLDER_EVENT = 'file-manager:show';
const STARTUP_BACKGROUND_REFRESH_TIMEOUT_MS = 1500;
const STARTUP_DIR_ENTRY_TIMEOUT_MS = 2000;

export function useInit() {
  const router = useRouter();
  const userSettingsStore = useUserSettingsStore();
  const userStatsStore = useUserStatsStore();
  const workspacesStore = useWorkspacesStore();
  const userPathsStore = useUserPathsStore();
  const platformStore = usePlatformStore();
  const globalSearchStore = useGlobalSearchStore();
  const appWindowStore = useAppWindowStore();
  const shortcutsStore = useShortcutsStore();
  const globalShortcutsStore = useGlobalShortcutsStore();
  const terminalsStore = useTerminalsStore();
  const backgroundMediaStore = useBackgroundMediaStore();
  const extensionsStore = useExtensionsStore();
  const archiveJobsStore = useArchiveJobsStore();
  const deleteJobsStore = useDeleteJobsStore();
  const copyMoveJobsStore = useCopyMoveJobsStore();
  const quickViewStore = useQuickViewStore();
  const { initAutoCheck } = useAppUpdater();
  useClipboardFocusSync();
  let appLaunchArgsUnlisten: UnlistenFn | null = null;
  let openMediaRequestUnlisten: UnlistenFn | null = null;
  let showInFolderUnlisten: UnlistenFn | null = null;
  let auxiliaryWindowLifecycleUnlisten: UnlistenFn | null = null;
  const backgroundTasks = new Set<Promise<void>>();

  function isMainWebviewWindow(): boolean {
    return getCurrentWebviewWindow().label === 'main';
  }

  function registerShortcutHandlers() {
    shortcutsStore.registerHandler('toggleGlobalSearch', () => {
      if (!globalSearchStore.isOpen) {
        router.push({ name: 'navigator' });
        globalSearchStore.open();
      }
      else {
        globalSearchStore.close();
      }
    });

    for (const shortcut of BUILTIN_NAVIGATION_PAGE_SHORTCUTS) {
      shortcutsStore.registerHandler(shortcut.id, () => {
        router.push({ name: shortcut.routeName });
      });
    }

    shortcutsStore.registerHandler('navigatePageBack', () => {
      router.go(-1);
    });
    shortcutsStore.registerHandler('navigatePageForward', () => {
      router.go(1);
    });
    shortcutsStore.registerHandler('uiZoomIncrease', () => {
      void applyUiZoomStep(1);
    });
    shortcutsStore.registerHandler('uiZoomDecrease', () => {
      void applyUiZoomStep(-1);
    });
    shortcutsStore.registerHandler('toggleFullscreen', () => {
      void toggleMainWindowFullscreen();
    });
  }

  function unregisterShortcutHandlers() {
    shortcutsStore.unregisterHandler('toggleGlobalSearch');

    for (const shortcut of BUILTIN_NAVIGATION_PAGE_SHORTCUTS) {
      shortcutsStore.unregisterHandler(shortcut.id);
    }

    shortcutsStore.unregisterHandler('navigatePageBack');
    shortcutsStore.unregisterHandler('navigatePageForward');
    shortcutsStore.unregisterHandler('uiZoomIncrease');
    shortcutsStore.unregisterHandler('uiZoomDecrease');
    shortcutsStore.unregisterHandler('toggleFullscreen');
  }

  function shouldKeepMainWindowHidden(
    launchContext: LaunchContext,
    openedLaunchTargets: boolean,
  ): boolean {
    if (openedLaunchTargets) {
      return false;
    }

    if (launchContext.hadAbsorbedShellPaths) {
      return false;
    }

    return launchContext.hadDelegatedShellPaths && launchContext.args.length <= 1;
  }

  function isCurrentNavigationReload(): boolean {
    const [navigationEntry] = performance.getEntriesByType('navigation') as PerformanceNavigationTiming[];

    return navigationEntry?.type === 'reload';
  }

  async function revealMainWindow(
    launchContextOverride?: LaunchContext,
    openedLaunchTargets = false,
  ) {
    await traceInitStep('revealMainWindow:nextTick', async () => {
      await nextTick();
      await new Promise(resolve => setTimeout(resolve, 0));
    });

    const currentWindow = getCurrentWindow();

    if (currentWindow.label === 'main') {
      const launchContext = launchContextOverride ?? await traceInitStep(
        'revealMainWindow:get_launch_context',
        () => invoke<LaunchContext>('get_launch_context'),
      );
      const launchedFromOsAutostart = launchContext.args.includes(SIGMA_AUTOSTART_CLI_FLAG);
      const stayHiddenAfterAutostart = launchedFromOsAutostart
        && userSettingsStore.userSettings.launchAtStartupHidden;
      const keepHidden = stayHiddenAfterAutostart
        || shouldKeepMainWindowHidden(launchContext, openedLaunchTargets);

      if (keepHidden) {
        await traceInitStep('revealMainWindow:hide', () => currentWindow.hide());
      }
      else {
        await traceInitStep('revealMainWindow:show', () => currentWindow.show());

        if (!isCurrentNavigationReload()) {
          await traceInitStep('revealMainWindow:setFocus', () => currentWindow.setFocus());
        }
      }
    }

    removeAppSplash();
  }

  async function openDirectoriesFromLaunchArgs(launchContext: LaunchContext): Promise<boolean> {
    const launchTargets = await resolveLaunchTargetsFromArgs(
      launchContext,
      path => workspacesStore.getDirEntry({
        path,
        timeoutMs: STARTUP_DIR_ENTRY_TIMEOUT_MS,
      }),
    );

    if (launchTargets.length === 0) {
      return false;
    }

    await router.push({ name: 'navigator' });

    for (const launchTarget of launchTargets) {
      await workspacesStore.openOrFocusTabGroup(launchTarget.directoryPath);

      if (launchTarget.focusPath) {
        workspacesStore.setPendingLaunchReveal(
          launchTarget.directoryPath,
          launchTarget.focusPath,
        );
      }
    }

    return true;
  }

  async function registerAppLaunchArgsListener() {
    if (appLaunchArgsUnlisten || !isMainWebviewWindow()) {
      return;
    }

    appLaunchArgsUnlisten = await listen<LaunchContext>(APP_LAUNCH_ARGS_EVENT, async (event) => {
      const didOpenTargets = await openDirectoriesFromLaunchArgs(event.payload);

      if (didOpenTargets) {
        const currentWindow = getCurrentWindow();
        await currentWindow.show();
        await currentWindow.setFocus();
      }
    });
  }

  function unregisterAppLaunchArgsListener() {
    appLaunchArgsUnlisten?.();
    appLaunchArgsUnlisten = null;
  }

  /**
   * A second app launch carrying a media file — usually another application opening it via
   * sigma once sigma is the registered viewer. The backend already resolved the path; the
   * file goes straight to Quick View rather than being revealed in a browser tab, and the
   * main window deliberately stays as it was.
   */
  async function registerOpenMediaRequestListener() {
    if (openMediaRequestUnlisten || !isMainWebviewWindow()) {
      return;
    }

    openMediaRequestUnlisten = await listen<{ path: string }>(
      OPEN_MEDIA_REQUEST_EVENT,
      async (event) => {
        // The external caller becomes quick view's owner: this viewing session outlives the
        // main window, which only sweeps content it put up itself.
        await quickViewStore.openFileFromMainWindow(event.payload.path, null, 'external');
      },
    );
  }

  function unregisterOpenMediaRequestListener() {
    openMediaRequestUnlisten?.();
    openMediaRequestUnlisten = null;
  }

  interface ShowInFolderRequest {
    items: string[];
    folders: string[];
  }

  /**
   * "Show in Folder" from another application — the `FileManager1` DBus interface, routed
   * through the backend. Items are revealed the way CLI file arguments are: the folder opens
   * with the file selected. This is a click that expects a window, so the main window shows
   * even from background residency.
   */
  async function applyShowInFolderRequest(request: ShowInFolderRequest) {
    const targets = [
      ...request.folders.map(path => ({
        directoryPath: path,
        focusPath: null as string | null,
      })),
      ...request.items.map(path => ({
        directoryPath: getParentDirectory(path) ?? path,
        focusPath: path,
      })),
    ];

    if (targets.length === 0) {
      return;
    }

    await router.push({ name: 'navigator' });

    for (const target of targets) {
      await workspacesStore.openOrFocusTabGroup(target.directoryPath);

      if (target.focusPath) {
        workspacesStore.setPendingLaunchReveal(target.directoryPath, target.focusPath);
      }
    }

    const currentWindow = getCurrentWindow();
    await currentWindow.show();
    await currentWindow.setFocus();
  }

  async function registerShowInFolderListener() {
    if (showInFolderUnlisten || !isMainWebviewWindow()) {
      return;
    }

    showInFolderUnlisten = await listen<ShowInFolderRequest>(
      SHOW_IN_FOLDER_EVENT,
      async (event) => {
        await applyShowInFolderRequest(event.payload);
      },
    );

    // Anything that arrived while this window was still booting — a DBus-activated cold
    // start, most likely — is waiting in the backend's queue.
    const pending = await invoke<ShowInFolderRequest[]>('drain_show_in_folder_requests');

    for (const request of pending) {
      await applyShowInFolderRequest(request);
    }
  }

  function unregisterShowInFolderListener() {
    showInFolderUnlisten?.();
    showInFolderUnlisten = null;
  }

  function runInBackground(task: () => Promise<void>, errorMessage: string) {
    const backgroundTask: Promise<void> = new Promise<void>((resolve) => {
      setTimeout(() => {
        task().catch((error) => {
          console.error(errorMessage, error);
        }).finally(resolve);
      }, 0);
    }).finally(() => {
      backgroundTasks.delete(backgroundTask);
    });

    backgroundTasks.add(backgroundTask);
  }

  async function awaitBackgroundTasks() {
    await Promise.allSettled([...backgroundTasks]);
  }

  function runInBackgroundWithTrace(
    stepLabel: string,
    task: () => Promise<unknown>,
    errorMessage: string,
  ) {
    runInBackground(
      () => traceInitStep(stepLabel, task)
        .then(() => undefined)
        .catch(() => undefined),
      errorMessage,
    );
  }

  async function init() {
    try {
      await runInit();
    }
    catch (error) {
      console.error('Failed to initialize app:', error);
      removeAppSplash();
    }
  }

  async function runInit() {
    const isMainWindow = isMainWebviewWindow();

    logInitTrace(`init started (mainWindow=${isMainWindow})`);

    await traceInitStep('platformStore.init', () => platformStore.init());
    await traceInitStep('pathComparisonVolumeCache.warm', () => warmPathComparisonVolumeCache());
    await traceInitStep('userPathsStore.init', () => userPathsStore.init());
    await traceInitStep('userSettingsStore.init', () => userSettingsStore.init());

    let initialLaunchContext: LaunchContext | undefined;
    let openedInitialLaunchTargets = false;
    let loadedInitialTabGroup = false;

    await traceInitStep(
      'backgroundMediaStore.refreshCustomBackgrounds',
      () => backgroundMediaStore.refreshCustomBackgrounds({
        timeoutMs: STARTUP_BACKGROUND_REFRESH_TIMEOUT_MS,
      }),
    );
    await traceInitStep('userStatsStore.init', () => userStatsStore.init());
    await traceInitStep(
      'workspacesStore.init',
      () => workspacesStore.init(undefined, { loadInitialTabGroup: false }),
    );

    if (isMainWindow) {
      initialLaunchContext = await traceInitStep(
        'invoke:get_launch_context',
        () => invoke<LaunchContext>('get_launch_context'),
      );

      if (initialLaunchContext.hadAbsorbedShellPaths) {
        try {
          await traceInitStep(
            'workspacesStore.loadCurrentTabGroup (absorbed shell)',
            () => workspacesStore.loadCurrentTabGroup({
              dirEntryTimeoutMs: STARTUP_DIR_ENTRY_TIMEOUT_MS,
            }),
          );
          loadedInitialTabGroup = true;
          openedInitialLaunchTargets = await traceInitStep(
            'openDirectoriesFromLaunchArgs (absorbed shell)',
            () => openDirectoriesFromLaunchArgs(initialLaunchContext as LaunchContext),
          );
        }
        catch (absorbedShellError) {
          console.error('Failed to prepare absorbed shell launch targets:', absorbedShellError);
        }
      }
    }

    await traceInitStep(
      'revealMainWindow',
      () => revealMainWindow(initialLaunchContext, openedInitialLaunchTargets),
    );
    await traceInitStep(
      'appWindowStore.initMainWindowStateListeners',
      () => appWindowStore.initMainWindowStateListeners(),
    );

    if (isMainWindow) {
      await traceInitStep('shortcutsStore.init', async () => {
        shortcutsStore.init();
      });
      await traceInitStep('globalShortcutsStore.init', () => globalShortcutsStore.init());
    }

    disableWebViewFeatures(isMainWindow);

    runInBackgroundWithTrace('background:preloadNavigatorRoute', async () => {
      preloadNavigatorRoute();
    }, 'Failed to preload navigator route:');

    runInBackgroundWithTrace('background:restoreStartupTabs', async () => {
      if (!loadedInitialTabGroup) {
        await workspacesStore.loadCurrentTabGroup({
          dirEntryTimeoutMs: STARTUP_DIR_ENTRY_TIMEOUT_MS,
        });
        loadedInitialTabGroup = true;
      }

      if (isMainWindow && initialLaunchContext && !openedInitialLaunchTargets) {
        openedInitialLaunchTargets = await openDirectoriesFromLaunchArgs(initialLaunchContext);

        if (openedInitialLaunchTargets) {
          await revealMainWindow(initialLaunchContext, true);
        }
      }
    }, 'Failed to restore startup tabs:');

    runInBackgroundWithTrace(
      'background:applyLaunchAtStartupPreference',
      () => applyLaunchAtStartupPreference(userSettingsStore.userSettings.launchAtStartup),
      'Failed to apply launch at startup preference:',
    );
    runInBackgroundWithTrace(
      'background:userStats.runDeferredMaintenance',
      () => userStatsStore.runDeferredMaintenance(),
      'Failed to run deferred user stats maintenance:',
    );
    runInBackgroundWithTrace(
      'background:globalSearch.initOnLaunch',
      () => globalSearchStore.initOnLaunch(),
      'Failed to initialize global search on launch:',
    );
    runInBackgroundWithTrace(
      'background:terminals.init',
      () => terminalsStore.init(),
      'Failed to initialize terminals:',
    );
    runInBackgroundWithTrace(
      'background:extensions.init',
      () => extensionsStore.init(),
      'Failed to initialize extensions:',
    );
    runInBackgroundWithTrace(
      'background:archiveJobs.ensureEventListeners',
      () => archiveJobsStore.ensureEventListeners(),
      'Failed to initialize archive jobs:',
    );
    runInBackgroundWithTrace(
      'background:deleteJobs.ensureEventListeners',
      () => deleteJobsStore.ensureEventListeners(),
      'Failed to initialize delete jobs:',
    );
    runInBackgroundWithTrace(
      'background:copyMoveJobs.ensureEventListeners',
      () => copyMoveJobsStore.ensureEventListeners(),
      'Failed to initialize copy and move jobs:',
    );
    runInBackgroundWithTrace(
      'background:quickView.ensureMainWindowDisplayedPathListener',
      () => quickViewStore.ensureMainWindowDisplayedPathListener(),
      'Failed to initialize quick view listener:',
    );

    if (isMainWindow) {
      runInBackgroundWithTrace(
        'background:auxiliaryWindows.prelaunchConfigured',
        () => prelaunchConfiguredAuxiliaryWindows(),
        'Failed to prelaunch auxiliary windows:',
      );
      runInBackgroundWithTrace(
        'background:auxiliaryWindows.setupLifecycle',
        async () => {
          auxiliaryWindowLifecycleUnlisten = await setupAuxiliaryWindowLifecycle();
        },
        'Failed to set up auxiliary window lifecycle:',
      );
    }

    if (isMainWindow) {
      runInBackgroundWithTrace(
        'background:appUpdater.initAutoCheck',
        () => initAutoCheck(),
        'Failed to initialize app updater:',
      );
    }

  }

  onMounted(() => {
    if (isMainWebviewWindow()) {
      registerShortcutHandlers();
      void registerAppLaunchArgsListener();
      void registerOpenMediaRequestListener();
      void registerShowInFolderListener();
    }
  });

  onUnmounted(() => {
    if (isMainWebviewWindow()) {
      unregisterShortcutHandlers();
      unregisterAppLaunchArgsListener();
      unregisterOpenMediaRequestListener();
      unregisterShowInFolderListener();
    }

    appWindowStore.disposeMainWindowStateListeners();

    if (auxiliaryWindowLifecycleUnlisten) {
      void auxiliaryWindowLifecycleUnlisten();
      auxiliaryWindowLifecycleUnlisten = null;
    }
  });

  return {
    init,
    awaitBackgroundTasks,
  };
}
