// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
// Copyright © 2026 Cortexist, LLC (modifications). All rights reserved.

import { markRaw } from 'vue';
import { useI18n } from 'vue-i18n';
import QRCode from 'qrcode';
import { invoke } from '@tauri-apps/api/core';
import { toast, ToastStatic } from '@/components/ui/toaster';
import { useLanShareStore } from '@/stores/runtime/lan-share';
import { useNavigatorSelectionStore } from '@/stores/runtime/navigator-selection';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import { useWorkspacesStore } from '@/stores/storage/workspaces';
import type { LanShareMode } from '@/types/lan-share';
import type { LanShareSettings } from '@/types/user-settings';
import type { DirEntry } from '@/types/dir-entry';
import { getParentDirectory } from '@/utils/normalize-path';

type LanShareResult = {
  address: string;
  hostname_address: string | null;
  ios_address: string | null;
};

type LanShareConfigPayload = {
  enableHttp: boolean;
  enableHttps: boolean;
  certPath: string | null;
  keyPath: string | null;
  customHostname: string | null;
};

function usesCertificateFile(settings: LanShareSettings): boolean {
  return settings.protocol !== 'httpOnly' && settings.certificateSource === 'certificateFile';
}

function buildLanShareConfig(settings: LanShareSettings): LanShareConfigPayload {
  const certPath = settings.certificatePath.trim();
  const keyPath = settings.privateKeyPath.trim();
  const customHostname = settings.customHostname.trim();

  return {
    enableHttp: settings.protocol !== 'httpsOnly',
    enableHttps: settings.protocol !== 'httpOnly',
    certPath: usesCertificateFile(settings) ? certPath : null,
    keyPath: usesCertificateFile(settings) ? keyPath : null,
    customHostname: customHostname || null,
  };
}

type ResolveShareResult = {
  path: string;
  defaultMode: LanShareMode;
  isFile: boolean;
  hubPaths?: string[];
};

function resolveSingleEntry(entry: DirEntry): ResolveShareResult {
  if (entry.is_dir) {
    return {
      path: entry.path,
      defaultMode: 'ftp',
      isFile: false,
    };
  }

  return {
    path: entry.path,
    defaultMode: 'stream',
    isFile: true,
  };
}

function resolveSharePath(entries: DirEntry[]): ResolveShareResult {
  if (entries.length === 0) {
    throw new Error('No entries to share');
  }

  if (entries.length > 1) {
    const allFiles = entries.every(entry => entry.is_file);

    if (!allFiles) {
      return resolveSingleEntry(entries[0]);
    }

    return {
      path: entries[0].path,
      defaultMode: 'stream',
      isFile: false,
      hubPaths: entries.map(entry => entry.path),
    };
  }

  return resolveSingleEntry(entries[0]);
}

function getEffectivePath(path: string, mode: LanShareMode, isFile: boolean): string {
  if (mode === 'ftp' && isFile) {
    return getParentDirectory(path);
  }

  return path;
}

async function buildLanShareQrDataUrl(address: string): Promise<string> {
  return QRCode.toDataURL(address, {
    width: 128,
    margin: 1,
    color: {
      dark: '#ffffffdd',
      light: '#00000000',
    },
  });
}

export function useLanShare() {
  const { t } = useI18n();
  const lanShareStore = useLanShareStore();
  const userSettingsStore = useUserSettingsStore();
  const workspacesStore = useWorkspacesStore();
  const navigatorSelectionStore = useNavigatorSelectionStore();

  async function startShare(entries: DirEntry[]) {
    if (entries.length === 0) return;

    const resolved = resolveSharePath(entries);
    const { path: originalPath, defaultMode, isFile, hubPaths } = resolved;

    await startShareWithMode(originalPath, defaultMode, isFile, {
      openPopover: true,
      hubPaths,
    });
  }

  async function startShareCurrentDirectory() {
    const tab = workspacesStore.currentTab;
    if (!tab || tab.type !== 'directory' || !tab.path) return;

    await startShareWithMode(tab.path, 'ftp', false, { openPopover: true });
  }

  async function startShareSelectedItems() {
    const entries = navigatorSelectionStore.selectedDirEntries;
    if (entries.length === 0) return;

    await startShare(entries);
  }

  async function startShareWithMode(
    originalPath: string,
    mode: LanShareMode,
    isFile: boolean,
    options?: {
      openPopover?: boolean;
      hubPaths?: string[];
      skipReplaceConfirm?: boolean;
    },
  ) {
    const skipReplaceConfirm = options?.skipReplaceConfirm ?? false;

    if (!skipReplaceConfirm && lanShareStore.activeSession) {
      lanShareStore.openReplaceShareDialog({
        originalPath,
        mode,
        isFile,
        openPopover: options?.openPopover ?? true,
        hubPaths: options?.hubPaths,
      });
      return;
    }

    const effectivePath = getEffectivePath(originalPath, mode, isFile);
    const openPopover = options?.openPopover ?? true;
    const hubPaths = options?.hubPaths;
    const hubPathsPayload = hubPaths && hubPaths.length >= 2 ? hubPaths : null;

    const lanShareSettings = userSettingsStore.userSettings.lanShare;

    // Surfaced here rather than left to the backend, whose "file not found" for an empty
    // path would send the user hunting for a file that was never chosen.
    if (
      usesCertificateFile(lanShareSettings)
      && (!lanShareSettings.certificatePath.trim() || !lanShareSettings.privateKeyPath.trim())
    ) {
      showErrorToast(t('lanShare.missingCertificateFiles'));
      return;
    }

    try {
      const result = await invoke<LanShareResult>('start_lan_share', {
        path: effectivePath,
        shareMode: mode,
        hubPaths: hubPathsPayload,
        config: buildLanShareConfig(lanShareSettings),
      });

      lanShareStore.setQrDataUrl(null);
      const sharedPathLabel
        = hubPathsPayload && hubPathsPayload.length >= 2
          ? t('lanShare.hubSharedSelectionCount', { count: hubPathsPayload.length })
          : effectivePath;

      lanShareStore.setActiveSession({
        address: result.address,
        hostnameAddress: result.hostname_address ?? undefined,
        iosAddress: result.ios_address ?? undefined,
        mode,
        sharedPath: sharedPathLabel,
        originalPath,
        isFile,
        hubPaths: hubPathsPayload ?? undefined,
      });

      try {
        const qrAddress = result.hostname_address ?? result.address;
        lanShareStore.setQrDataUrl(await buildLanShareQrDataUrl(qrAddress));
      }
      catch {
        lanShareStore.setQrDataUrl(null);
      }

      if (openPopover) {
        lanShareStore.isPopoverOpen = true;
      }
    }
    catch (error) {
      showErrorToast(String(error));
    }
  }

  async function switchLanShareMode(newMode: LanShareMode) {
    const session = lanShareStore.activeSession;
    if (!session) return;

    if (session.hubPaths && session.hubPaths.length >= 2 && newMode === 'ftp') {
      return;
    }

    await startShareWithMode(session.originalPath, newMode, session.isFile, {
      openPopover: false,
      hubPaths: session.hubPaths,
      skipReplaceConfirm: true,
    });
  }

  function copyLanShareAddress(value: string) {
    navigator.clipboard.writeText(value).then(() => {
      toast.custom(markRaw(ToastStatic), {
        componentProps: {
          data: {
            title: t('dialogs.localShareManagerDialog.addressCopiedToClipboard'),
            description: value,
          },
        },
        duration: 2000,
      });
    });
  }

  function showErrorToast(errorMessage: string) {
    toast.custom(markRaw(ToastStatic), {
      componentProps: {
        data: {
          title: t('lanShare.failedToStart'),
          description: errorMessage,
        },
      },
      duration: 5000,
    });
  }

  async function stopShare() {
    try {
      await invoke('stop_lan_share');
    }
    catch (error) {
      console.error('Failed to stop LAN share:', error);
    }

    lanShareStore.setActiveSession(null);
  }

  async function confirmReplaceShare() {
    const pending = lanShareStore.pendingReplaceShare;

    if (!pending) {
      lanShareStore.closeReplaceShareDialog();
      return;
    }

    lanShareStore.closeReplaceShareDialog();
    await startShareWithMode(
      pending.originalPath,
      pending.mode,
      pending.isFile,
      {
        openPopover: pending.openPopover,
        hubPaths: pending.hubPaths,
        skipReplaceConfirm: true,
      },
    );
  }

  function cancelReplaceShare() {
    lanShareStore.closeReplaceShareDialog();
  }

  return {
    startShare,
    startShareCurrentDirectory,
    startShareSelectedItems,
    switchLanShareMode,
    stopShare,
    copyLanShareAddress,
    confirmReplaceShare,
    cancelReplaceShare,
  };
}
