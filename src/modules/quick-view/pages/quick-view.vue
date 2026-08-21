<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
Copyright © 2026 Cortexist, LLC (modifications). All rights reserved.
-->

<script setup lang="ts">
import {
  ref, computed, onMounted, onUnmounted, watch, nextTick,
  type ComponentPublicInstance,
} from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { emitTo, listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  Loader2Icon,
  FileWarningIcon,
  FileImageIcon,
  FileTextIcon,
  Music2Icon,
  VideoIcon,
} from '@lucide/vue';
import { ScrollArea } from '@/components/ui/scroll-area';
import { toast } from '@/components/ui/toaster';
import {
  determineFileType,
  getFileName,
  getFileExtension,
  getQuickViewDisplayUrl,
  isHttpOrHttpsUrl,
  fetchQuickViewSiblingPathsFromDisk,
  OPEN_MEDIA_REQUEST_EVENT,
  QUICK_VIEW_DISPLAYED_PATH_CHANGED_EVENT,
  QUICK_VIEW_LOAD_FILE_EVENT,
  QUICK_VIEW_SIBLING_PATHS_CHANGED_EVENT,
  QUICK_VIEW_BACKGROUND_PLAYBACK_EVENT,
  QUICK_VIEW_RESTORED_EVENT,
  QUICK_VIEW_STOP_PLAYBACK_EVENT,
  QUICK_VIEW_SETTLE_UNSAVED_EDITS_EVENT,
  type QuickViewFileType,
} from '@/stores/runtime/quick-view';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import {
  backgroundPlayerAfterViewChange,
  isPlaybackFileType,
  shouldKeepPlayingAfterDismissal,
  type QuickViewDismissal,
} from '@/modules/quick-view/utils/background-playback';
import { convertMediaSrc } from '@/utils/media-src';
import { withContentVersion } from '@/utils/file-content-version';
import { useWatchedFileContentVersion } from '@/composables/use-file-content-version';
import { MediaPlayer } from '@/components/ui/media-player';
import { ImageViewer } from '@/components/ui/image-viewer';
import WindowActions from '@/modules/window-toolbar/window-actions.vue';
import UnsavedChangesDialog from '@/modules/quick-view/components/unsaved-changes-dialog.vue';
import { quickViewWindowTitle } from '@/modules/quick-view/utils/window-title';
import {
  decodeTextFileBytesWithEncoding,
  encodeTextFileBytes,
  type TextFileSourceEncoding,
} from '@/utils/decode-text-file-bytes';
import {
  TextEditor,
  type TextEditorMarkdownMode,
} from '@/components/ui/text-editor';
import type { DirContents, DirEntry } from '@/types/dir-entry';
import { getParentDirectory } from '@/utils/normalize-path';
import {
  AUXILIARY_WINDOW_RELEASED_EVENT,
  buildAuxiliaryWindowReadyPayload,
  QUICK_VIEW_WINDOW_READY_EVENT,
  releaseAuxiliaryWindow,
} from '@/utils/auxiliary-windows';
import { useImageThumbnails } from '@/modules/navigator/components/file-browser/composables/use-image-thumbnails';
import { useVideoThumbnails } from '@/modules/navigator/components/file-browser/composables/use-video-thumbnails';
import { useHorizontalFixedVirtualList } from '@/composables/use-horizontal-fixed-virtual-list';
import { useAudioCovers } from '@/composables/use-audio-covers';
import { useArtistShow } from '@/composables/use-artist-show';
import { waitForFirstPaint } from '@/utils/first-paint';

const { t } = useI18n();

const QUICK_VIEW_TEXT_PREVIEW_MAX_BYTES = 4 * 1024 * 1024;
/** Long enough for a cold window to map; short enough that the degraded path is not a hang. */
const FIRST_PAINT_TIMEOUT_MS = 1500;

/** See the first-load wait in `applyLoadedFile`. */
let hasAppliedFirstLoad = false;

/**
 * Whether this window is a whole session — launched to view one file, with no main window
 * behind it — rather than a limb of the file manager. Decided once at mount by asking the
 * backend; it changes where files come from (a command instead of main's events) and what
 * closing means (quitting instead of hiding).
 */
let isStandaloneViewer = false;

const currentFilePath = ref<string | null>(null);
const resolvedSiblingPaths = ref<string[]>([]);
const siblingPathsProvidedByMain = ref(false);

const userSettingsStore = useUserSettingsStore();
/** The mounted player, whichever of the two kinds is on screen. Absent for everything else. */
const mediaPlayerRef = ref<{
  isPlaying: boolean;
  pause: () => void;
  restart: () => void;
} | null>(null);
/** Set while a background session is registered with the backend. See `isPlaybackOutOfView`. */
const isPlayingInBackground = ref(false);
/**
 * A playback file kept mounted behind whatever the view shows — a song left playing under a
 * text file opened over it. The window being hidden is the other way playback gets out of
 * sight, and the session the backend is told about covers both; see `isPlaybackOutOfView`.
 */
const backgroundMediaPath = ref<string | null>(null);
/** Set from this window being put away until it is brought back, by whichever side does it. */
const isWindowHidden = ref(false);

/**
 * Whether what this window shows may be edited. Only sigma's own opens say yes: another
 * application is handed a viewer — the desktop entry offers it media types alone — and the
 * text files it can reach through the strip must not turn that viewer into an editor. Stamped
 * by every load, so it follows ownership of the window.
 */
const isEditingAllowed = ref(false);
/** Set while `settleAllUnsavedEdits` runs, when stashed edits are brought back even if editing is off. */
let isSettlingUnsavedEdits = false;

type UnsavedChangesChoice = 'save' | 'discard' | 'cancel';
/** The open question about unsaved edits, holding the resolver its answer goes to. */
const unsavedChangesPrompt = ref<{ resolve: (choice: UnsavedChangesChoice) => void } | null>(null);
const isUnsavedChangesPromptOpen = computed(() => unsavedChangesPrompt.value !== null);
let pendingUnsavedChangesAnswer: Promise<UnsavedChangesChoice> | null = null;

const stripThumbnails = useImageThumbnails();
const stripAudioCovers = useAudioCovers();
const artistShow = useArtistShow();
const stripVideoThumbnails = useVideoThumbnails();
const stripDirEntryByPath = ref<Record<string, DirEntry>>({});
let stripEntryLoadToken = 0;
let stripThumbnailParentKey: string | null = null;
let stripVirtualThumbRangePrevious: {
  start: number;
  end: number;
} = {
  start: 0,
  end: 0,
};

const isLoading = ref(true);
const stripScrollAreaRef = ref<InstanceType<typeof ScrollArea> | null>(null);
const stripScrollViewportRef = ref<HTMLElement | null>(null);

function uniqueSiblingPaths(paths: string[]): string[] {
  return Array.from(new Set(paths));
}

function syncQuickViewStripViewportRef() {
  const instance = stripScrollAreaRef.value as unknown as ComponentPublicInstance | null;
  const rawElement = instance && '$el' in instance ? instance.$el : null;
  const root = rawElement instanceof HTMLElement ? rawElement : null;
  stripScrollViewportRef.value = root?.querySelector<HTMLElement>('.sigma-ui-scroll-area__viewport') ?? null;
}

watch([stripScrollAreaRef, () => resolvedSiblingPaths.value.length], () => {
  void nextTick(syncQuickViewStripViewportRef);
}, { immediate: true });

const pdfIframeRef = ref<HTMLIFrameElement | null>(null);
const textEditorRef = ref<InstanceType<typeof TextEditor> | null>(null);
const textEditorValue = ref('');
const textSavedBaseline = ref('');
const textSourceEncoding = ref<TextFileSourceEncoding>('utf8');
const textSaveRoundTripSafe = ref(true);
const textWasTruncated = ref(false);
const textPreviewError = ref<string | null>(null);
const textPreviewLoading = ref(false);
const textSaveInProgress = ref(false);
let textPreviewRequestId = 0;
let unlistenLoadFile: UnlistenFn | null = null;
let unlistenCloseRequested: UnlistenFn | null = null;
let unlistenSiblingPathsChanged: UnlistenFn | null = null;
let unlistenWindowReleased: UnlistenFn | null = null;
let unlistenRestored: UnlistenFn | null = null;
let unlistenStopRequested: UnlistenFn | null = null;
let unlistenOpenMediaRequest: UnlistenFn | null = null;
let unlistenSettleRequested: UnlistenFn | null = null;

watch(
  resolvedSiblingPaths,
  async (paths) => {
    const loadToken = ++stripEntryLoadToken;

    if (paths.length === 0) {
      stripDirEntryByPath.value = {};
      stripThumbnailParentKey = null;
      stripThumbnails.clearThumbnails();
      stripVideoThumbnails.clearThumbnails();
      return;
    }

    const localPaths = paths.filter(pathItem => !isHttpOrHttpsUrl(pathItem));
    const parentDirs = [...new Set(
      localPaths
        .map(pathItem => getParentDirectory(pathItem))
        .filter((directory): directory is string => Boolean(directory)),
    )].sort();

    const nextParentKey = parentDirs.join('\0');

    if (nextParentKey !== stripThumbnailParentKey) {
      stripThumbnailParentKey = nextParentKey;
      stripThumbnails.clearThumbnails();
      stripVideoThumbnails.clearThumbnails();
      stripVirtualThumbRangePrevious = {
        start: 0,
        end: 0,
      };
    }

    const nextMap: Record<string, DirEntry> = {};
    const pathSet = new Set(paths);

    try {
      for (const parentDir of parentDirs) {
        const contents = await invoke<DirContents>('read_dir', { path: parentDir });

        if (loadToken !== stripEntryLoadToken) {
          return;
        }

        for (const entry of contents.entries) {
          if (entry.is_file && pathSet.has(entry.path)) {
            nextMap[entry.path] = entry;
          }
        }
      }
    }
    catch {
      if (loadToken !== stripEntryLoadToken) {
        return;
      }

      stripDirEntryByPath.value = {};
      stripThumbnailParentKey = null;
      stripThumbnails.clearThumbnails();
      stripVideoThumbnails.clearThumbnails();
      stripVirtualThumbRangePrevious = {
        start: 0,
        end: 0,
      };
      return;
    }

    if (loadToken !== stripEntryLoadToken) {
      return;
    }

    stripDirEntryByPath.value = nextMap;
  },
  { immediate: true },
);

interface PendingTextState {
  text: string;
  baseline: string;
  encoding: TextFileSourceEncoding;
  truncated: boolean;
  saveRoundTripSafe: boolean;
}

interface ReadTextPreviewResult {
  bytes: number[];
  truncated: boolean;
}

const pendingTextEdits = ref<Record<string, PendingTextState>>({});

const fileType = computed((): QuickViewFileType => {
  if (!currentFilePath.value) return 'unsupported';
  return determineFileType(currentFilePath.value);
});

const fileName = computed((): string => {
  if (!currentFilePath.value) return '';
  return getFileName(currentFilePath.value);
});

/**
 * The file the player is mounted for: the one in view when that is a playback file, else the
 * one playing behind the view. Kept apart from `currentFilePath` so the player's source never
 * follows the view to a text file — unmounting is what would stop the sound.
 */
const playerPath = computed((): string | null => {
  if (currentFilePath.value && isPlaybackFileType(fileType.value)) {
    return currentFilePath.value;
  }

  return backgroundMediaPath.value;
});

const playerKind = computed((): QuickViewFileType => (
  playerPath.value ? determineFileType(playerPath.value) : 'unsupported'
));

const isPlayerInView = computed(() => playerPath.value !== null && playerPath.value === currentFilePath.value);

/**
 * Artwork for the open track: the picture embedded in the file, else a cover image sitting
 * beside it. Returning nothing leaves the player on its music-glyph fallback.
 */
const audioArtworkSrc = computed((): string | undefined => {
  const path = playerPath.value;

  if (!path || playerKind.value !== 'audio' || isHttpOrHttpsUrl(path)) {
    return undefined;
  }

  void stripAudioCovers.embeddedCovers.value;
  void stripAudioCovers.siblingCovers.value;

  const entry = stripDirEntryByPath.value[path];
  const embedded = entry ? stripAudioCovers.getEmbeddedCover(entry) : undefined;

  if (embedded) {
    return embedded;
  }

  return stripAudioCovers.getSiblingCover(path);
});

/**
 * This window watches its own file. It is handed a path and nothing else — and when another
 * file manager launched it there is no navigator in the process to notice anything on its
 * behalf — so re-saving the file while it is on screen has to reach the viewer from here.
 * A remote URL has no file behind it to watch, hence the local-only guard.
 */
const displayedFileContentVersion = useWatchedFileContentVersion(() => (
  currentFilePath.value && !isHttpOrHttpsUrl(currentFilePath.value)
    ? currentFilePath.value
    : null
));

const fileAssetUrl = computed((): string => {
  if (!currentFilePath.value) return '';

  return withContentVersion(
    getQuickViewDisplayUrl(currentFilePath.value),
    displayedFileContentVersion.value,
  );
});

/**
 * The player's file is watched on its own: behind the view it is not the displayed file, and
 * a save to the text file in front must not reach the player's source and restart the song.
 */
const playerContentVersion = useWatchedFileContentVersion(() => (
  playerPath.value && !isHttpOrHttpsUrl(playerPath.value)
    ? playerPath.value
    : null
));

// Video and audio go through the loopback media server on Linux, where the asset
// protocol cannot feed WebKitGTK's media backend. See @/utils/media-src.
const playerMediaUrl = computed((): string => {
  if (!playerPath.value) return '';
  if (isHttpOrHttpsUrl(playerPath.value)) return playerPath.value;

  return withContentVersion(
    convertMediaSrc(playerPath.value),
    playerContentVersion.value,
  );
});

/**
 * The file on disk behind whatever is playing, which is what media details are read from and
 * what the native frame decoder opens. A remote URL handed in by an extension has neither, so
 * those keep a plain player. The element is only rendered for a `playerPath` of its kind, so
 * this does not need to check the kind itself.
 */
const playerSourcePath = computed(() => {
  const path = playerPath.value;

  if (!path || isHttpOrHttpsUrl(path)) {
    return undefined;
  }

  return path;
});

const textIsDirty = computed(() => {
  if (!currentFilePath.value || determineFileType(currentFilePath.value) !== 'text') {
    return false;
  }

  return textEditorValue.value !== textSavedBaseline.value;
});

const canSaveText = computed(() => {
  if (!currentFilePath.value || determineFileType(currentFilePath.value) !== 'text') {
    return false;
  }

  if (
    textPreviewLoading.value
    || textPreviewError.value
    || textWasTruncated.value
    || !textSaveRoundTripSafe.value
    || textSaveInProgress.value
  ) {
    return false;
  }

  return textIsDirty.value;
});

const textEditorReadOnly = computed(() => (
  textWasTruncated.value || !textSaveRoundTripSafe.value || !isEditingAllowed.value
));

/**
 * Edits not on disk anywhere in this window — in the editor, or stashed for files it moved
 * away from. Reported to the backend as it changes, so a session ending (the main window
 * closing, a quit) knows to give this window the chance to ask rather than taking them with
 * it. Reported at once on start, so a fresh page clears whatever a previous one left behind.
 */
const hasUnsavedEdits = computed(() => (
  textIsDirty.value || Object.keys(pendingTextEdits.value).length > 0
));

watch(hasUnsavedEdits, (unsaved) => {
  void invoke('set_quick_view_unsaved_edits', { unsaved }).catch(() => {});
}, { immediate: true });

const isMarkdownQuickView = computed(() => {
  if (!currentFilePath.value) {
    return false;
  }

  return getFileExtension(currentFilePath.value) === 'md';
});

/** Where a markdown file's relative links resolve; a document fetched from a URL has no such place. */
const markdownSourcePath = computed(() => {
  const path = currentFilePath.value;

  if (!path || isHttpOrHttpsUrl(path)) {
    return null;
  }

  return path;
});

/**
 * Read or edit (or, for markdown, both): a preference about the editor rather than the file,
 * so it outlives the window — one for markdown and one for plain text, since a split view
 * means nothing to the latter and the two are different habits.
 */
const editorMode = computed((): TextEditorMarkdownMode => (
  isMarkdownQuickView.value
    ? userSettingsStore.userSettings.navigator.quickViewMarkdownMode
    : userSettingsStore.userSettings.navigator.quickViewTextMode
));

function setEditorMode(mode: TextEditorMarkdownMode) {
  if (isMarkdownQuickView.value) {
    void userSettingsStore.set('navigator.quickViewMarkdownMode', mode);
    return;
  }

  void userSettingsStore.set('navigator.quickViewTextMode', mode === 'read' ? 'read' : 'edit');
}

// A preference about the editor rather than the file, so it outlives the window.

function thumbStripKind(path: string): 'image' | 'video' | 'audio' | 'document' {
  const type = determineFileType(path);

  if (type === 'image' || type === 'video' || type === 'audio') {
    return type;
  }

  return 'document';
}

const thumbsWithKind = computed(() =>
  resolvedSiblingPaths.value.map(path => ({
    path,
    kind: thumbStripKind(path),
    hasUnsavedBadge: Boolean(pendingTextEdits.value[path])
      || (path === currentFilePath.value && textIsDirty.value),
  })),
);

function quickViewStripImageSrc(path: string): string | undefined {
  if (determineFileType(path) !== 'image') {
    return undefined;
  }

  if (isHttpOrHttpsUrl(path) || getFileExtension(path) === 'svg') {
    return getQuickViewDisplayUrl(path);
  }

  void stripThumbnails.imageThumbnails.value;

  const entry = stripDirEntryByPath.value[path];

  if (!entry) {
    return undefined;
  }

  return stripThumbnails.getImageThumbnail(entry);
}

function quickViewStripVideoSrc(path: string): string | undefined {
  if (determineFileType(path) !== 'video' || isHttpOrHttpsUrl(path)) {
    return undefined;
  }

  void stripVideoThumbnails.videoThumbnails.value;

  const entry = stripDirEntryByPath.value[path];

  if (!entry) {
    return undefined;
  }

  return stripVideoThumbnails.getVideoThumbnail(entry);
}

/**
 * Audio has no frame to sample, so the strip shows the picture embedded in the file. Falls
 * back to the music glyph, which is what the template does when this returns nothing.
 */
function quickViewStripAudioSrc(path: string): string | undefined {
  if (determineFileType(path) !== 'audio' || isHttpOrHttpsUrl(path)) {
    return undefined;
  }

  void stripAudioCovers.embeddedCovers.value;

  const entry = stripDirEntryByPath.value[path];

  if (!entry) {
    return undefined;
  }

  return stripAudioCovers.getEmbeddedCover(entry);
}

function quickViewStripImageShowsSpinner(path: string): boolean {
  if (determineFileType(path) !== 'image') {
    return false;
  }

  if (isHttpOrHttpsUrl(path) || getFileExtension(path) === 'svg') {
    return false;
  }

  void stripThumbnails.imageThumbnails.value;

  const entry = stripDirEntryByPath.value[path];

  if (!entry) {
    return true;
  }

  const readySrc = stripThumbnails.getImageThumbnail(entry);

  if (readySrc) {
    return false;
  }

  return !stripThumbnails.shouldShowImageThumbnailFallback(entry);
}

function cancelQuickViewStripThumbnailForSiblingIndex(entryIndex: number) {
  const paths = resolvedSiblingPaths.value;

  if (entryIndex < 0 || entryIndex >= paths.length) {
    return;
  }

  cancelQuickViewStripThumbnailForPath(paths[entryIndex]);
}

function cancelQuickViewStripThumbnailForPath(path: string | undefined) {
  if (!path || isHttpOrHttpsUrl(path)) {
    return;
  }

  const fileType = determineFileType(path);
  const entry = stripDirEntryByPath.value[path];

  if (!entry) {
    return;
  }

  if (fileType === 'video') {
    stripVideoThumbnails.cancelVideoThumbnail(entry);
    return;
  }

  if (fileType !== 'image' || getFileExtension(path) === 'svg') {
    return;
  }

  stripThumbnails.cancelImageThumbnail(entry);
}

const QUICK_VIEW_STRIP_THUMB_WIDTH = 64;
const QUICK_VIEW_STRIP_THUMB_GAP = 8;

const stripVirtualItemCount = computed(() => resolvedSiblingPaths.value.length);

const stripVirtual = useHorizontalFixedVirtualList({
  itemCount: stripVirtualItemCount,
  itemWidthPx: QUICK_VIEW_STRIP_THUMB_WIDTH,
  itemGapPx: QUICK_VIEW_STRIP_THUMB_GAP,
  viewportRef: stripScrollViewportRef,
});

const stripVirtualTotalWidthPx = stripVirtual.totalWidthPx;
const stripVirtualRowLeftPx = stripVirtual.rowAbsoluteLeftPx;

const stripVirtualVisibleThumbs = computed(() => {
  const { start, end } = stripVirtual.visibleRange.value;
  return thumbsWithKind.value.slice(start, end);
});

watch(
  () => ({
    paths: resolvedSiblingPaths.value,
    start: stripVirtual.visibleRange.value.start,
    end: stripVirtual.visibleRange.value.end,
  }),
  (next, previous) => {
    if (next.paths.length === 0) {
      stripVirtualThumbRangePrevious = {
        start: 0,
        end: 0,
      };
      return;
    }

    const pathsChanged = !previous || next.paths !== previous.paths;

    const { start: rangeStart, end: rangeEnd } = next;
    const { start: previousStart, end: previousEnd } = stripVirtualThumbRangePrevious;
    const nextVisiblePaths = pathsChanged ? new Set(next.paths.slice(rangeStart, rangeEnd)) : null;

    for (let entryIndex = previousStart; entryIndex < previousEnd; entryIndex += 1) {
      const previousPath = previous?.paths[entryIndex];
      const shouldCancel = pathsChanged
        ? !nextVisiblePaths?.has(previousPath ?? '')
        : entryIndex < rangeStart || entryIndex >= rangeEnd;

      if (shouldCancel) {
        if (pathsChanged) {
          cancelQuickViewStripThumbnailForPath(previousPath);
          continue;
        }

        cancelQuickViewStripThumbnailForSiblingIndex(entryIndex);
      }
    }

    stripVirtualThumbRangePrevious = {
      start: rangeStart,
      end: rangeEnd,
    };
  },
  { flush: 'post' },
);

function isEditableKeyboardTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }

  if (target.isContentEditable) {
    return true;
  }

  const tag = target.tagName;
  return tag === 'TEXTAREA' || tag === 'INPUT' || tag === 'SELECT';
}

function resetQuickViewWindowState(shouldNotifyMainWindow = true) {
  // Putting the window away is not closing the file: what was typed waits in the stash, as it
  // does when moving between files, and reopening the file finds it. Only saving or discarding
  // lets go of it — see `closeWindow`, which asks before a close that would lose it.
  stashCurrentTextIfDirty();
  currentFilePath.value = null;
  backgroundMediaPath.value = null;
  resolvedSiblingPaths.value = [];
  siblingPathsProvidedByMain.value = false;

  if (shouldNotifyMainWindow) {
    void emitTo(
      {
        kind: 'WebviewWindow',
        label: 'main',
      },
      QUICK_VIEW_DISPLAYED_PATH_CHANGED_EVENT,
      { path: null },
    );
  }
}

function shouldKeepPlayingAfter(dismissal: QuickViewDismissal): boolean {
  return shouldKeepPlayingAfterDismissal({
    behavior: userSettingsStore.userSettings.navigator.quickViewPlaybackOnDismiss,
    dismissal,
    isPlaying: mediaPlayerRef.value?.isPlaying === true,
  });
}

/**
 * Registers the file playing out of sight with the backend — which is what stops the
 * nothing-visible rule from quitting the app mid-track, and what the outside world's marker
 * advertises. The main window is told too, so its shortcut can bring the file back instead of
 * reloading it.
 */
async function sendToBackgroundPlayback(): Promise<void> {
  /**
   * The same session is asked for more than once: dismissing from in here hides the window
   * through the main window, which answers by telling this one it was released, and the
   * derived `isPlaybackOutOfView` asks again on its own schedule. Only the first starts it;
   * without this the main window would announce it twice over.
   */
  if (isPlayingInBackground.value) {
    return;
  }

  isPlayingInBackground.value = true;

  await invoke('set_quick_view_background_playback', { playing: true }).catch(() => {});

  if (!isStandaloneViewer) {
    void emitTo(
      {
        kind: 'WebviewWindow',
        label: 'main',
      },
      QUICK_VIEW_BACKGROUND_PLAYBACK_EVENT,
      {
        path: playerPath.value,
        active: true,
      },
    );
  }
}

/**
 * Ends the background session — the window came back, the file finished, or it was closed
 * outright. The backend stops holding the process open for it, and may find there is now
 * nothing left on screen and quit, which is the whole point of reporting the end.
 */
async function endBackgroundPlayback(): Promise<void> {
  if (!isPlayingInBackground.value) {
    return;
  }

  isPlayingInBackground.value = false;

  await invoke('set_quick_view_background_playback', { playing: false }).catch(() => {});

  if (!isStandaloneViewer) {
    void emitTo(
      {
        kind: 'WebviewWindow',
        label: 'main',
      },
      QUICK_VIEW_BACKGROUND_PLAYBACK_EVENT,
      {
        path: null,
        active: false,
      },
    );
  }
}

/**
 * Playback is reported as it changes, whether this window is hidden or in plain sight: it is
 * what tells the backend that a click on the launcher or the tray belongs to this window
 * rather than the file manager. Sound is what a user follows, and it does not stop being the
 * thing they are reaching for just because the window is visible on another workspace.
 */
const isPlayerPlaying = computed(() => mediaPlayerRef.value?.isPlaying === true);

watch(isPlayerPlaying, (playing) => {
  void invoke('set_quick_view_playing', { playing }).catch(() => {});
});

/**
 * What a background session *is*: playback the user cannot see — behind a hidden window, or
 * behind another file shown in a visible one. Derived from those facts rather than set at each
 * transition, so the marker the outside world watches can never say one thing while the sound
 * does another. A file that plays itself out ends the session the same way, and lets the app
 * quit if nothing is left on screen.
 *
 * Flushed after render because the player reports through its component ref, which lags a
 * tick behind the view: judged before the render, a file being dropped as the window is put
 * away would still read as playing and register a session it is about to end.
 */
const isPlaybackOutOfView = computed(() => (
  isPlayerPlaying.value && (isWindowHidden.value || !isPlayerInView.value)
));

watch(isPlaybackOutOfView, (outOfView) => {
  if (outOfView) {
    void sendToBackgroundPlayback();
  }
  else {
    void endBackgroundPlayback();
  }
}, { flush: 'post' });

/**
 * Asks what to do with edits about to be lost, and waits for the answer. Asked from within
 * this window, of this window — no caller has to know, which is what lets another application
 * use Quick View as its viewer and still get the question. A second request while it is open
 * (the window manager's close on top of the close button, say) joins the first rather than
 * asking twice.
 */
function askAboutUnsavedChanges(): Promise<UnsavedChangesChoice> {
  if (pendingUnsavedChangesAnswer) {
    return pendingUnsavedChangesAnswer;
  }

  pendingUnsavedChangesAnswer = new Promise<UnsavedChangesChoice>((resolve) => {
    unsavedChangesPrompt.value = { resolve };
  }).finally(() => {
    unsavedChangesPrompt.value = null;
    pendingUnsavedChangesAnswer = null;
  });

  return pendingUnsavedChangesAnswer;
}

function answerUnsavedChanges(choice: UnsavedChangesChoice) {
  unsavedChangesPrompt.value?.resolve(choice);
}

/**
 * Settles unsaved edits before a close that would lose them. Resolves false when the close
 * should not go ahead: the user said so, or a save they asked for failed — the toast has said
 * why, and the edits stay where they can see them.
 */
async function settleUnsavedChangesBeforeClosing(): Promise<boolean> {
  const choice = await askAboutUnsavedChanges();

  if (choice === 'cancel') {
    return false;
  }

  if (choice === 'save') {
    await saveTextFile();
    return !textIsDirty.value;
  }

  // Discarded: back to the saved text, so nothing downstream stashes what was just let go.
  textEditorValue.value = textSavedBaseline.value;

  if (currentFilePath.value) {
    removePendingEditForPath(currentFilePath.value);
  }

  return true;
}

/**
 * Waits for the text pane to have loaded `path` — the watcher on `currentFilePath` does the
 * loading, on its own schedule.
 */
async function waitForTextPreview(path: string) {
  await nextTick();

  while (currentFilePath.value === path && textPreviewLoading.value) {
    await new Promise<void>(resolve => setTimeout(resolve, 16));
  }
}

/**
 * Settles every unsaved edit this window holds before they would be lost: the file in view
 * first, then each stashed file in turn, brought into view so the question is about something
 * the user can see. Resolves false as soon as one answer is "cancel", leaving the rest where
 * they were — nothing has been lost yet, and nothing is.
 */
async function settleAllUnsavedEdits(): Promise<boolean> {
  isSettlingUnsavedEdits = true;

  try {
    for (;;) {
      if (textIsDirty.value) {
        if (!(await settleUnsavedChangesBeforeClosing())) {
          return false;
        }

        continue;
      }

      const [stashedPath] = Object.keys(pendingTextEdits.value);

      if (!stashedPath) {
        return true;
      }

      if (currentFilePath.value === stashedPath) {
        // Already in view, showing the disk's version because editing was off when it loaded.
        await loadTextPreview(stashedPath);
      }
      else {
        await showFile(stashedPath);
        await waitForTextPreview(stashedPath);
      }

      // A stash that turns out to match the file has nothing to ask about.
      if (!textIsDirty.value) {
        removePendingEditForPath(stashedPath);
      }
    }
  }
  finally {
    isSettlingUnsavedEdits = false;
  }
}

async function closeWindow(dismissal: QuickViewDismissal = 'dismiss') {
  // Only a close takes edits away: hiding keeps the page, and with it the stash that moving
  // between files uses, so Space and Escape cost nothing. (A session ending with the window
  // hidden asks through `QUICK_VIEW_SETTLE_UNSAVED_EDITS_EVENT` instead.)
  if (dismissal === 'close' && hasUnsavedEdits.value) {
    if (!(await settleAllUnsavedEdits())) {
      return;
    }
  }

  if (shouldKeepPlayingAfter(dismissal)) {
    isWindowHidden.value = true;
    // Deliberately keeps the file mounted: the player going away is what stops the sound. The
    // session is registered before the window goes, so the exit check that follows the hide
    // finds its reason to stay rather than racing the derived watcher for it.
    await sendToBackgroundPlayback();
    await bringBackgroundPlayerIntoView();
  }
  else {
    await endBackgroundPlayback();
    resetQuickViewWindowState(!isStandaloneViewer);
    isWindowHidden.value = true;
    await nextTick();
  }

  /**
   * A standalone viewer is the whole session, so dismissing it quits: hide first so the
   * window vanishes immediately, then let the shared nothing-visible rule end the process.
   * The auxiliary release path below would instead ask a main window that does not exist.
   */
  if (isStandaloneViewer) {
    await getCurrentWindow().hide();
    await invoke('exit_if_no_windows_left');
    return;
  }

  await releaseAuxiliaryWindow('quick-view');
}

/**
 * Drops the open file without touching the filmstrip, leaving the window on its empty state
 * with the surviving files still one click away. Used when the file on screen is deleted:
 * carrying on with a pane playing something that no longer exists is worse than showing
 * nothing, and there is no way to know whether any neighbour is worth advancing to.
 */
function clearDisplayedFile() {
  const removedPath = currentFilePath.value;

  if (removedPath) {
    const remainingEdits = { ...pendingTextEdits.value };
    delete remainingEdits[removedPath];
    pendingTextEdits.value = remainingEdits;
  }

  // Retire any in-flight read so a late reply cannot repopulate the pane.
  textPreviewRequestId += 1;

  // A file still playing behind the view is the natural thing to show in place of one that is
  // gone. Its view change runs synchronously, so the stash it takes is removed right after.
  if (backgroundMediaPath.value) {
    void bringBackgroundPlayerIntoView();

    if (removedPath) {
      removePendingEditForPath(removedPath);
    }

    return;
  }

  currentFilePath.value = null;
  textEditorValue.value = '';
  textSavedBaseline.value = '';
  textPreviewError.value = null;
  textPreviewLoading.value = false;
  textWasTruncated.value = false;
  textSaveRoundTripSafe.value = true;

  void getCurrentWindow().setTitle(quickViewWindowTitle());
  void emitTo(
    {
      kind: 'WebviewWindow',
      label: 'main',
    },
    QUICK_VIEW_DISPLAYED_PATH_CHANGED_EVENT,
    { path: null },
  );
}

/**
 * A listing that no longer mentions the open file is not proof the file went away — the
 * browser may just be filtered, which would hide it while it sits happily on disk. Only the
 * filesystem can settle that, so confirm before clearing anything.
 */
async function discardDisplayedFileIfDeleted(paths: string[]) {
  const displayedPath = currentFilePath.value;

  if (!displayedPath || isHttpOrHttpsUrl(displayedPath) || paths.includes(displayedPath)) {
    return;
  }

  try {
    if (await invoke<boolean>('path_exists', { path: displayedPath })) {
      return;
    }
  }
  catch {
    // Unconfirmed is not deleted; leave what is on screen alone.
    return;
  }

  // A new file may have loaded while the check was in flight.
  if (currentFilePath.value !== displayedPath) {
    return;
  }

  clearDisplayedFile();
}

async function setQuickViewWindowTitle(path: string) {
  const quickWindow = getCurrentWindow();
  await quickWindow.setTitle(quickViewWindowTitle(getFileName(path)));
}

async function ensureResolvedSiblingPaths(): Promise<string[]> {
  if (!currentFilePath.value) {
    return [];
  }

  let paths = resolvedSiblingPaths.value;

  if (paths.length <= 1 && !siblingPathsProvidedByMain.value) {
    paths = await fetchQuickViewSiblingPathsFromDisk(currentFilePath.value);
    paths = uniqueSiblingPaths(paths);
    resolvedSiblingPaths.value = paths;
  }

  return paths;
}

function stashCurrentTextIfDirty() {
  const path = currentFilePath.value;

  if (!path || determineFileType(path) !== 'text') {
    return;
  }

  if (textPreviewLoading.value || textPreviewError.value) {
    return;
  }

  if (textEditorValue.value === textSavedBaseline.value) {
    return;
  }

  pendingTextEdits.value = {
    ...pendingTextEdits.value,
    [path]: {
      text: textEditorValue.value,
      baseline: textSavedBaseline.value,
      encoding: textSourceEncoding.value,
      truncated: textWasTruncated.value,
      saveRoundTripSafe: textSaveRoundTripSafe.value,
    },
  };
}

function removePendingEditForPath(path: string) {
  if (!pendingTextEdits.value[path]) {
    return;
  }

  const next = { ...pendingTextEdits.value };
  delete next[path];
  pendingTextEdits.value = next;
}

/**
 * Settles what the player does when `path` takes the view, then shows the file. Quick View
 * shows one thing at a time, and this is the one place that rule meets the player — see
 * `backgroundPlayerAfterViewChange` for what becomes of a file that was playing.
 */
function takeView(path: string) {
  const displayed = currentFilePath.value;

  backgroundMediaPath.value = backgroundPlayerAfterViewChange({
    displayed: displayed
      ? {
          path: displayed,
          isPlayback: isPlaybackFileType(fileType.value),
        }
      : null,
    background: backgroundMediaPath.value,
    incoming: {
      path,
      isPlayback: isPlaybackFileType(determineFileType(path)),
    },
    behavior: userSettingsStore.userSettings.navigator.quickViewPlaybackOnDismiss,
    isPlaying: mediaPlayerRef.value?.isPlaying === true,
  });

  if (displayed !== path) {
    stashCurrentTextIfDirty();
  }

  currentFilePath.value = path;
}

/**
 * Puts a file in the view and tells the main window, whose shortcut toggles on what is shown.
 * Every way into the view from within this window goes through here.
 */
async function showFile(path: string) {
  takeView(path);
  await setQuickViewWindowTitle(path);
  void emitTo(
    {
      kind: 'WebviewWindow',
      label: 'main',
    },
    QUICK_VIEW_DISPLAYED_PATH_CHANGED_EVENT,
    { path },
  );
}

/**
 * The playing file takes the view back from whatever was opened over it — where the EQ
 * button, the launcher and the shortcut on the file all lead. The file in front is simply
 * dropped: the view holds one thing, and the editor here is for quick edits, not for guarding
 * a document against the user's own next move. Unsaved text is stashed the way moving between
 * siblings stashes it, so reopening the document in this window finds it again.
 */
async function bringBackgroundPlayerIntoView() {
  const path = backgroundMediaPath.value;

  if (!path) {
    return;
  }

  await showFile(path);

  // The strip belonged to the other file's folder; one that does not list this file is
  // replaced by its own folder, as a file opened from disk gets.
  if (!resolvedSiblingPaths.value.includes(path)) {
    resolvedSiblingPaths.value = [];
    siblingPathsProvidedByMain.value = false;
    await ensureResolvedSiblingPaths();
  }

  await nextTick();
  scrollActiveThumbIntoView();
}

async function selectPath(path: string) {
  if (path === currentFilePath.value) {
    return;
  }

  await showFile(path);
}

async function loadTextPreview(path: string) {
  if (isHttpOrHttpsUrl(path)) {
    textPreviewLoading.value = false;
    textPreviewError.value = t('quickView.unsupportedFileType');
    return;
  }

  const pending = pendingTextEdits.value[path];

  // Stashed edits come back only where they can be acted on: a viewer that may not edit shows
  // the file as it is on disk and leaves the stash for when editing is back, or for the
  // settling a session's end runs.
  if (pending && (isEditingAllowed.value || isSettlingUnsavedEdits)) {
    ++textPreviewRequestId;
    textPreviewLoading.value = true;
    textPreviewError.value = null;
    removePendingEditForPath(path);
    textWasTruncated.value = pending.truncated;
    textSaveRoundTripSafe.value = pending.saveRoundTripSafe ?? true;
    textSourceEncoding.value = pending.encoding;
    textEditorValue.value = pending.text;
    textSavedBaseline.value = pending.baseline;
    textPreviewLoading.value = false;
    return;
  }

  const requestId = ++textPreviewRequestId;
  textPreviewLoading.value = true;
  textPreviewError.value = null;
  textEditorValue.value = '';
  textSavedBaseline.value = '';
  textWasTruncated.value = false;
  textSaveRoundTripSafe.value = true;

  try {
    const preview = await invoke<ReadTextPreviewResult>('read_text_preview', {
      path,
      maxBytes: QUICK_VIEW_TEXT_PREVIEW_MAX_BYTES,
    });

    if (requestId !== textPreviewRequestId) {
      return;
    }

    const bytes = new Uint8Array(preview.bytes);
    textWasTruncated.value = preview.truncated;

    const { text, encoding, saveRoundTripSafe } = decodeTextFileBytesWithEncoding(bytes);
    textSaveRoundTripSafe.value = saveRoundTripSafe;
    textSourceEncoding.value = encoding;
    textEditorValue.value = text;
    textSavedBaseline.value = text;
  }
  catch (caught) {
    if (requestId !== textPreviewRequestId) {
      return;
    }

    const message = caught instanceof Error ? caught.message : String(caught);
    textPreviewError.value = message;
  }
  finally {
    if (requestId === textPreviewRequestId) {
      textPreviewLoading.value = false;
    }
  }
}

function revertTextChanges() {
  if (!canSaveText.value) {
    return;
  }

  textEditorValue.value = textSavedBaseline.value;
}

async function saveTextFile() {
  const path = currentFilePath.value;

  if (!path || isHttpOrHttpsUrl(path) || determineFileType(path) !== 'text' || textWasTruncated.value || !canSaveText.value) {
    return;
  }

  textSaveInProgress.value = true;

  try {
    const bytes = encodeTextFileBytes(textEditorValue.value, textSourceEncoding.value);
    await invoke('write_file_binary', {
      path,
      data: Array.from(bytes),
    });
    textSavedBaseline.value = textEditorValue.value;
    removePendingEditForPath(path);
    toast.success(t('quickView.textSaved'), { duration: 2500 });
  }
  catch {
    toast.error(t('quickView.textSaveFailed'));
  }
  finally {
    textSaveInProgress.value = false;
  }
}

async function goToSibling(offset: number) {
  if (!currentFilePath.value) {
    return;
  }

  const paths = await ensureResolvedSiblingPaths();

  if (paths.length <= 1) {
    return;
  }

  const currentIndex = paths.indexOf(currentFilePath.value);
  const fromIndex = currentIndex >= 0 ? currentIndex : 0;
  const nextIndex = fromIndex + offset;

  if (nextIndex < 0 || nextIndex >= paths.length) {
    return;
  }

  const nextPath = paths[nextIndex];

  if (nextPath === currentFilePath.value) {
    return;
  }

  await showFile(nextPath);
}

function scrollActiveThumbIntoView() {
  const activePath = currentFilePath.value;

  if (!activePath) {
    return;
  }

  const activeIndex = resolvedSiblingPaths.value.indexOf(activePath);

  if (activeIndex < 0) {
    return;
  }

  void nextTick(() => {
    requestAnimationFrame(() => {
      stripVirtual.scrollItemIntoViewCentered(activeIndex, 'auto');
    });
  });
}

async function handleKeydown(event: KeyboardEvent) {
  // The open question owns the keyboard: Escape answers it (cancel) rather than closing again.
  if (isUnsavedChangesPromptOpen.value) {
    return;
  }

  const saveShortcut = (event.ctrlKey || event.metaKey) && event.code === 'KeyS';

  if (saveShortcut) {
    if (currentFilePath.value && determineFileType(currentFilePath.value) === 'text') {
      event.preventDefault();

      if (canSaveText.value && isEditingAllowed.value) {
        await saveTextFile();
      }
    }

    return;
  }

  const printPdfShortcut = (event.ctrlKey || event.metaKey)
    && event.code === 'KeyP'
    && !event.altKey;

  if (printPdfShortcut) {
    if (
      currentFilePath.value
      && determineFileType(currentFilePath.value) === 'pdf'
      && !isEditableKeyboardTarget(event.target)
    ) {
      const pdfInnerWindow = pdfIframeRef.value?.contentWindow;

      if (pdfInnerWindow) {
        event.preventDefault();
        event.stopPropagation();
        pdfInnerWindow.focus();
        pdfInnerWindow.print();
        return;
      }
    }
  }

  if (event.code === 'Escape') {
    event.preventDefault();

    // An open find bar takes Escape first, the way a dialog would; only then does it mean close.
    if (textEditorRef.value?.closeFind()) {
      return;
    }

    await closeWindow();
    return;
  }

  if (event.code === 'Space' && !isEditableKeyboardTarget(event.target)) {
    event.preventDefault();
    await closeWindow();
    return;
  }

  if (event.code === 'ArrowLeft' || event.code === 'ArrowRight') {
    if (isEditableKeyboardTarget(event.target)) {
      return;
    }

    event.preventDefault();
    await goToSibling(event.code === 'ArrowLeft' ? -1 : 1);
  }
}

async function applyLoadedFile(payload: {
  path: string;
  siblingPaths: string[] | null;
  /** Whether the caller is sigma itself; see `isEditingAllowed`. */
  editable: boolean;
}) {
  /**
   * The first file a fresh process receives arrives while this window is still being mapped —
   * whoever called `show()` only knows the request was made. Building a media pipeline against
   * a window that has no surface yet is how the first quick view of a session used to hang on
   * a spinner, so the first load waits until frames are actually being produced. Every later
   * load finds a window that has kept its surface, where this costs at most two frames.
   */
  if (!hasAppliedFirstLoad) {
    hasAppliedFirstLoad = true;
    await waitForFirstPaint(FIRST_PAINT_TIMEOUT_MS);
  }

  // Whoever sent the file shows the window around it; the standalone viewer shows itself.
  isWindowHidden.value = false;
  isEditingAllowed.value = payload.editable;

  /**
   * The file the player already has arriving again is still a request to open it. It happens
   * once a background session has ended on its own — the file played itself out, or was
   * stopped from outside — and this window was left hidden with the file still mounted: the
   * main window knows of no session to bring back, so it opens the file the way it would any
   * other. For any other file that means starting from the top, and the same file must not be
   * the one exception. Taking the view changes nothing the player can see, so it is told.
   * (The main window brings a file it knows to be playing back through `QUICK_VIEW_RESTORED_EVENT`
   * instead, which is what keeps the position.)
   */
  const isReopeningPlayerFile = payload.path === playerPath.value;

  takeView(payload.path);

  if (isReopeningPlayerFile) {
    mediaPlayerRef.value?.restart();
  }

  resolvedSiblingPaths.value = uniqueSiblingPaths(payload.siblingPaths ?? []);
  siblingPathsProvidedByMain.value = payload.siblingPaths !== null;
  isLoading.value = false;
  await setQuickViewWindowTitle(payload.path);
  await ensureResolvedSiblingPaths();
  await nextTick();
  scrollActiveThumbIntoView();
}

async function setupEventListeners() {
  const currentWindow = getCurrentWindow();

  unlistenLoadFile = await listen<{
    path: string;
    siblingPaths: string[] | null;
    editable: boolean;
  }>(
    QUICK_VIEW_LOAD_FILE_EVENT,
    event => applyLoadedFile(event.payload),
  );

  unlistenOpenMediaRequest = await listen<{ path: string }>(
    OPEN_MEDIA_REQUEST_EVENT,
    async (event) => {
      // In a normal session the main window answers these; the viewer answers only for itself.
      if (!isStandaloneViewer) {
        return;
      }

      // Shown before the file lands, as on the viewer's first load: a file arriving may end
      // a background session, and the exit check that ends with must find this window on
      // screen — the viewer is the whole process, and a hidden one is nothing to keep it for.
      const currentWindow = getCurrentWindow();
      await currentWindow.show();
      await applyLoadedFile({
        path: event.payload.path,
        siblingPaths: null,
        editable: false,
      });
      await currentWindow.setFocus();
    },
  );

  /**
   * The browser this window was opened from re-sends its file list whenever the directory
   * watcher sees the folder change, so the strip follows files being created and deleted.
   * The main window has already checked the list belongs to this window's directory.
   */
  unlistenSiblingPathsChanged = await listen<{ paths: string[] }>(
    QUICK_VIEW_SIBLING_PATHS_CHANGED_EVENT,
    async (event) => {
      const paths = uniqueSiblingPaths(event.payload.paths);

      resolvedSiblingPaths.value = paths;
      siblingPathsProvidedByMain.value = true;

      await discardDisplayedFileIfDeleted(paths);

      await nextTick();
      scrollActiveThumbIntoView();
    },
  );

  /**
   * The main window hiding this one is the same gesture as dismissing it from in here, so it
   * gets the same answer: keep playing if that is what the setting and the player call for,
   * and otherwise drop the file, which unmounts the player and stops the sound. Without this
   * the hidden window kept playing regardless, with no way to reach it.
   *
   * The main window has already let go of its side, so there is nothing to report back to it.
   */
  unlistenWindowReleased = await listen(AUXILIARY_WINDOW_RELEASED_EVENT, async () => {
    if (isStandaloneViewer) {
      return;
    }

    if (shouldKeepPlayingAfter('dismiss')) {
      isWindowHidden.value = true;
      await sendToBackgroundPlayback();
      // A hidden window shows what it plays: a text file opened over the song was closed by
      // this very gesture, so the song takes the view back now rather than on the way back.
      await bringBackgroundPlayerIntoView();
      return;
    }

    await endBackgroundPlayback();
    resetQuickViewWindowState(false);
    isWindowHidden.value = true;
  });

  /**
   * Back on screen: whatever is playing takes the view back from anything opened over it, and
   * being visible again is what ends the session — `isPlaybackOutOfView` sees to that.
   */
  unlistenRestored = await listen(QUICK_VIEW_RESTORED_EVENT, async () => {
    isWindowHidden.value = false;
    await bringBackgroundPlayerIntoView();
  });

  /**
   * The outside world asking the background session to stop. Pausing is the whole job:
   * `isPlaybackOutOfView` treats the silence exactly like a file that played itself out, ends
   * the session, and lets the app quit if nothing is left on screen.
   */
  unlistenStopRequested = await listen(QUICK_VIEW_STOP_PLAYBACK_EVENT, () => {
    if (!isPlayingInBackground.value) {
      return;
    }

    mediaPlayerRef.value?.pause();
  });

  /**
   * The session is ending and this window holds unsaved edits; the backend has put it back on
   * screen. Ask, then finish what was started: quit if that was the request, otherwise put the
   * window away as a dismissal would — what is left, a song playing on say, decides whether
   * the process stays. A cancel leaves the window up, and the session with it.
   */
  unlistenSettleRequested = await listen<{ quitAfter: boolean }>(
    QUICK_VIEW_SETTLE_UNSAVED_EDITS_EVENT,
    async (event) => {
      isWindowHidden.value = false;

      if (!(await settleAllUnsavedEdits())) {
        return;
      }

      if (event.payload.quitAfter) {
        await invoke('exit_app');
        return;
      }

      await closeWindow('dismiss');
    },
  );

  unlistenCloseRequested = await currentWindow.onCloseRequested(async (event) => {
    event.preventDefault();
    // The window manager's close ends the file, like the window's own close button.
    await closeWindow('close');
  });
}

watch(fileType, (newType) => {
  if (newType === 'unsupported' && currentFilePath.value) {
    closeWindow();
  }
});

// Resolved ahead of the show being asked for, so a track left playing has its material ready
// by the time the ten-second countdown runs out. Follows the player rather than the view: a
// song playing behind a text file keeps its material for when it takes the view back.
watch(playerPath, (path) => {
  if (path && !isHttpOrHttpsUrl(path) && determineFileType(path) === 'audio') {
    void artistShow.load(path);
  }
  else {
    void artistShow.load(null);
  }
});

watch(currentFilePath, (path) => {
  void nextTick(() => {
    scrollActiveThumbIntoView();
  });

  if (!path || determineFileType(path) !== 'text') {
    textEditorValue.value = '';
    textSavedBaseline.value = '';
    textSourceEncoding.value = 'utf8';
    textSaveRoundTripSafe.value = true;
    textWasTruncated.value = false;
    textPreviewError.value = null;
    textPreviewLoading.value = false;
    return;
  }

  void loadTextPreview(path);
});

/**
 * A text file rewritten underneath the editor is re-read, so the pane shows the file rather
 * than the memory of it. Unsaved edits win: someone typing here has a version of this file
 * that exists nowhere else, and replacing it with what landed on disk would destroy the only
 * copy. Their edits stay, and saving them resolves it the way they choose.
 *
 * Media and images need nothing here — their sources carry the version, so the viewer reloads
 * on its own.
 */
watch(displayedFileContentVersion, (version, previousVersion) => {
  const path = currentFilePath.value;

  if (!version || !previousVersion || version === previousVersion) {
    return;
  }

  if (!path || determineFileType(path) !== 'text' || textIsDirty.value) {
    return;
  }

  void loadTextPreview(path);
});

onMounted(async () => {
  window.addEventListener('keydown', handleKeydown, true);
  await setupEventListeners();
  void invoke('configure_webview_hide_pdf_more_settings').catch(() => {});

  /**
   * Launched to view one file: no main window exists, so nothing will send a load event.
   * The window shows itself *before* loading — the first-paint wait inside `applyLoadedFile`
   * needs frames to be produced, and a hidden window produces none.
   */
  const standaloneFile = await invoke<string | null>('standalone_launch_file');

  if (standaloneFile) {
    isStandaloneViewer = true;
    const currentWindow = getCurrentWindow();
    await currentWindow.show();
    await currentWindow.setFocus();
    await applyLoadedFile({
      path: standaloneFile,
      siblingPaths: null,
      editable: false,
    });
    return;
  }

  void emitTo(
    {
      kind: 'WebviewWindow',
      label: 'main',
    },
    QUICK_VIEW_WINDOW_READY_EVENT,
    buildAuxiliaryWindowReadyPayload(),
  );
  isLoading.value = false;
});

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeydown, true);
  stripThumbnails.clearThumbnails();
  stripVideoThumbnails.clearThumbnails();

  if (unlistenLoadFile) {
    unlistenLoadFile();
  }

  if (unlistenOpenMediaRequest) {
    unlistenOpenMediaRequest();
  }

  if (unlistenCloseRequested) {
    unlistenCloseRequested();
  }

  if (unlistenSiblingPathsChanged) {
    unlistenSiblingPathsChanged();
  }

  if (unlistenWindowReleased) {
    unlistenWindowReleased();
  }

  if (unlistenRestored) {
    unlistenRestored();
  }

  if (unlistenStopRequested) {
    unlistenStopRequested();
  }

  if (unlistenSettleRequested) {
    unlistenSettleRequested();
  }
});
</script>

<template>
  <div class="quick-view">
    <!-- App-drawn titlebar so this window matches the frameless main window rather than
         wearing the desktop's own decorations. -->
    <div
      class="quick-view__titlebar"
      data-tauri-drag-region
    >
      <span
        class="quick-view__titlebar-title"
        data-tauri-drag-region
      >{{ fileName }}</span>
      <!-- The one gesture that means "done with this file" rather than "hide it". -->
      <WindowActions :close-handler="() => closeWindow('close')" />
    </div>

    <div
      v-if="isLoading"
      class="quick-view__loading"
    >
      <Loader2Icon
        :size="48"
        class="quick-view__loading-icon"
      />
    </div>

    <template v-else-if="currentFilePath">
      <div
        class="quick-view__body"
        :class="{
          'quick-view__body--stretch': fileType === 'text',
          'quick-view__body--media': isPlayerInView,
        }"
      >
        <!-- Deliberately *not* keyed on the file path. The viewer and the players own the
             element that `requestFullscreen` was called on, and remounting it on every
             next-file dropped the window out of fullscreen mid-browse. Both components reset
             themselves when `src` changes, so one instance can carry the whole folder. -->
        <ImageViewer
          v-if="fileType === 'image'"
          :src="fileAssetUrl"
          :alt="fileName"
          class="quick-view__image"
        />

        <iframe
          ref="pdfIframeRef"
          v-else-if="fileType === 'pdf'"
          :key="`${currentFilePath}-pdf`"
          :src="fileAssetUrl"
          class="quick-view__pdf"
        />

        <TextEditor
          v-else-if="fileType === 'text'"
          ref="textEditorRef"
          :key="`${currentFilePath}-text`"
          v-model="textEditorValue"
          :readonly="textEditorReadOnly"
          :markdown="isMarkdownQuickView"
          :source-path="markdownSourcePath"
          :mode="editorMode"
          :can-save="canSaveText"
          :saving="textSaveInProgress"
          :loading="textPreviewLoading"
          :error="textPreviewError"
          @update:mode="setEditorMode"
          @save="void saveTextFile()"
          @revert="revertTextChanges()"
        >
          <template #status>
            <span v-if="textWasTruncated">{{ t('quickView.readOnlyTruncated') }}</span>
            <span v-else-if="!textSaveRoundTripSafe">{{ t('quickView.readOnlyEncoding') }}</span>
            <span v-else-if="!isEditingAllowed">{{ t('quickView.readOnlyExternal') }}</span>
          </template>
        </TextEditor>

        <div
          v-else-if="!isPlayerInView"
          :key="`${currentFilePath}-unsupported`"
          class="quick-view__unsupported"
        >
          <FileWarningIcon
            :size="64"
            class="quick-view__unsupported-icon"
          />
          <p class="quick-view__unsupported-text">
            {{ t('quickView.unsupportedFileType') }}
          </p>
        </div>

        <!-- Mounted for `playerPath`, not for the file in view: a song plays on behind a text
             file opened over it, and hiding its element rather than unmounting it is what
             keeps the sound. One player at a time, so there is never a second one to duet. -->
        <MediaPlayer
          v-if="playerKind === 'video'"
          v-show="isPlayerInView"
          ref="mediaPlayerRef"
          :src="playerMediaUrl"
          kind="video"
          class="quick-view__video"
          :source-path="playerSourcePath"
          allow-frame-capture
          autoplay
        />

        <MediaPlayer
          v-else-if="playerKind === 'audio'"
          v-show="isPlayerInView"
          ref="mediaPlayerRef"
          :src="playerMediaUrl"
          kind="audio"
          :source-path="playerSourcePath"
          :poster="audioArtworkSrc"
          :now-playing="isPlayerInView ? artistShow.show.value : null"
          class="quick-view__audio"
          autoplay
        />
      </div>
    </template>

    <div
      v-else
      class="quick-view__empty"
    >
      <p>{{ t('quickView.noFileSelected') }}</p>
    </div>

    <div
      v-if="resolvedSiblingPaths.length > 0"
      class="quick-view__strip"
    >
      <ScrollArea
        ref="stripScrollAreaRef"
        orientation="horizontal"
        class="quick-view__strip-scroll"
      >
        <div
          class="quick-view__strip-virtual-spacer"
          :style="{
            width: `${stripVirtualTotalWidthPx}px`,
            minHeight: `${QUICK_VIEW_STRIP_THUMB_WIDTH}px`,
          }"
        >
          <div
            class="quick-view__strip-row quick-view__strip-row--virtual"
            role="tablist"
            :aria-label="t('quickView.thumbnailStripLabel')"
            :style="{ left: `${stripVirtualRowLeftPx}px` }"
          >
            <button
              v-for="thumb in stripVirtualVisibleThumbs"
              :key="thumb.path"
              type="button"
              role="tab"
              class="quick-view__thumb"
              :class="{ 'quick-view__thumb--active': thumb.path === currentFilePath }"
              :aria-selected="thumb.path === currentFilePath"
              :aria-setsize="resolvedSiblingPaths.length"
              :aria-posinset="resolvedSiblingPaths.indexOf(thumb.path) + 1"
              :data-quick-view-thumb="thumb.path"
              :title="thumb.hasUnsavedBadge ? t('quickView.thumbnailUnsavedHint') : undefined"
              @click="void selectPath(thumb.path)"
            >
              <img
                v-if="thumb.kind === 'image' && quickViewStripImageSrc(thumb.path)"
                class="quick-view__thumb-image"
                :src="quickViewStripImageSrc(thumb.path)"
                alt=""
              >
              <Loader2Icon
                v-else-if="thumb.kind === 'image' && quickViewStripImageShowsSpinner(thumb.path)"
                :size="28"
                class="quick-view__thumb-loading-icon"
                aria-hidden="true"
              />
              <FileImageIcon
                v-else-if="thumb.kind === 'image'"
                class="quick-view__thumb-icon"
                :size="28"
                aria-hidden="true"
              />
              <img
                v-else-if="thumb.kind === 'video' && quickViewStripVideoSrc(thumb.path)"
                class="quick-view__thumb-image"
                :src="quickViewStripVideoSrc(thumb.path)"
                alt=""
              >
              <VideoIcon
                v-else-if="thumb.kind === 'video'"
                class="quick-view__thumb-icon"
                :size="28"
                aria-hidden="true"
              />
              <img
                v-else-if="thumb.kind === 'audio' && quickViewStripAudioSrc(thumb.path)"
                class="quick-view__thumb-image"
                :src="quickViewStripAudioSrc(thumb.path)"
                alt=""
              >
              <Music2Icon
                v-else-if="thumb.kind === 'audio'"
                class="quick-view__thumb-icon"
                :size="28"
                aria-hidden="true"
              />
              <FileTextIcon
                v-else-if="thumb.kind === 'document'"
                class="quick-view__thumb-icon"
                :size="28"
                aria-hidden="true"
              />
              <span
                v-if="thumb.hasUnsavedBadge"
                class="quick-view__thumb-unsaved-badge"
                aria-hidden="true"
              />
            </button>
          </div>
        </div>
      </ScrollArea>
    </div>

    <div class="quick-view__hint">
      {{ t('quickView.closeHint') }}
    </div>

    <UnsavedChangesDialog
      :open="isUnsavedChangesPromptOpen"
      :file-name="fileName"
      @save="answerUnsavedChanges('save')"
      @discard="answerUnsavedChanges('discard')"
      @cancel="answerUnsavedChanges('cancel')"
    />
  </div>
</template>

<style scoped>
.quick-view {
  display: flex;
  overflow: hidden;
  width: 100vw;
  height: 100vh;
  flex-direction: column;
  align-items: stretch;
  background: hsl(var(--background, 0 0% 100%));
}

.quick-view__loading {
  display: flex;
  flex: 1 1 auto;
  align-items: center;
  justify-content: center;
}

.quick-view__loading-icon {
  animation: spin 1s linear infinite;
  color: hsl(var(--muted-foreground, 0 0% 45%));
}

@keyframes spin {
  from {
    transform: rotate(0deg);
  }

  to {
    transform: rotate(360deg);
  }
}

.quick-view__titlebar {
  display: flex;
  height: var(--window-toolbar-height);
  flex: none;
  align-items: center;
  justify-content: space-between;
  padding-left: 12px;
  gap: 8px;
}

.quick-view__titlebar-title {
  overflow: hidden;
  color: hsl(var(--foreground) / 70%);
  font-size: 13px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.quick-view__body {
  display: flex;
  min-height: 0;
  box-sizing: border-box;
  flex: 1 1 0;
  align-items: center;
  justify-content: center;
  padding: 8px;
}

/* Video already letterboxes itself against black, so the pane padding only costs height
   that a portrait video in a narrow window cannot spare. */

.quick-view__body--media {
  padding: 0;
}

.quick-view__body--stretch {
  width: 100%;
  align-items: stretch;
  align-self: stretch;
}

/* Fills the pane the way the video player does, rather than shrink-wrapping the picture:
   the viewer letterboxes inside itself, and its zoom and fullscreen controls need to sit
   against the viewing area rather than against the image's own edges. */

.quick-view__image {
  min-width: 0;
  min-height: 0;
  flex: 1 1 auto;
  align-self: stretch;
}

/* The player fills the pane and letterboxes the video inside it, so the control bar spans
   the viewing area instead of tracking the video's own edges. */

.quick-view__video {
  min-width: 0;
  min-height: 0;
  flex: 1 1 auto;
  align-self: stretch;
}

.quick-view__audio {
  min-width: 0;
  min-height: 0;
  flex: 1 1 auto;
  align-self: stretch;
}

.quick-view__pdf {
  width: 100%;
  height: 100%;
  min-height: 0;
  border: none;
  background: white;
}

.quick-view__unsupported {
  display: flex;
  flex-direction: column;
  align-items: center;
  color: hsl(var(--muted-foreground, 0 0% 45%));
  gap: 16px;
}

.quick-view__unsupported-icon {
  opacity: 0.5;
}

.quick-view__unsupported-text {
  margin: 0;
  font-size: 14px;
}

.quick-view__empty {
  display: flex;
  flex: 1 1 auto;
  align-items: center;
  justify-content: center;
  color: hsl(var(--muted-foreground, 0 0% 45%));
  font-size: 14px;
}

.quick-view__strip {
  width: 100%;
  flex: 0 0 auto;
  padding: 6px 12px 0;
  border-top: 1px solid hsl(var(--border, 0 0% 90%));
  background: hsl(var(--background, 0 0% 100%) / 95%);
}

.quick-view__strip-scroll {
  width: 100%;
  height: 85px;
}

.quick-view__strip-scroll :deep(.sigma-ui-scroll-area__viewport > div) {
  width: max-content;
  max-width: none;
}

.quick-view__strip-virtual-spacer {
  position: relative;
  box-sizing: border-box;
}

.quick-view__strip-row--virtual {
  position: absolute;
  top: 0;
  left: 0;
}

.quick-view__strip-row {
  display: flex;
  flex-flow: row nowrap;
  align-items: center;
  gap: 8px;
}

.quick-view__thumb {
  position: relative;
  display: flex;
  overflow: hidden;
  width: 64px;
  height: 64px;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 2px solid transparent;
  border-radius: 6px;
  background: hsl(var(--muted, 0 0% 96%));
  cursor: pointer;
  transition:
    border-color 0.15s ease,
    box-shadow 0.15s ease,
    opacity 0.15s ease;
}

.quick-view__thumb-unsaved-badge {
  position: absolute;
  top: 4px;
  right: 4px;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: hsl(var(--destructive, 0 84% 45%));
  box-shadow: 0 0 0 2px hsl(var(--background, 0 0% 100%));
  pointer-events: none;
}

.quick-view__thumb:hover {
  opacity: 0.92;
}

.quick-view__thumb--active {
  border-color: hsl(var(--ring, 220 90% 50%));
  box-shadow: 0 0 0 1px hsl(var(--ring, 220 90% 50%) / 35%);
}

.quick-view__thumb-image {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.quick-view__thumb-icon {
  color: hsl(var(--muted-foreground, 0 0% 45%));
  opacity: 0.85;
}

.quick-view__thumb-loading-icon {
  animation: spin 1s linear infinite;
  color: hsl(var(--muted-foreground, 0 0% 45%));
  opacity: 0.85;
}

.quick-view__hint {
  flex: 0 0 auto;
  padding: 0 8px 8px;
  background: hsl(var(--background, 0 0% 100%) / 90%);
  color: hsl(var(--muted-foreground, 0 0% 45%));
  font-size: 12px;
  text-align: center;
}

@media (prefers-color-scheme: dark) {
  .quick-view {
    background: hsl(var(--background, 0 0% 10%));
  }

  .quick-view__strip {
    border-top-color: hsl(var(--border, 0 0% 20%));
    background: hsl(var(--background, 0 0% 10%) / 95%);
  }

  .quick-view__thumb {
    background: hsl(var(--muted, 0 0% 18%));
  }

  .quick-view__thumb-unsaved-badge {
    box-shadow: 0 0 0 2px hsl(var(--background, 0 0% 10%));
  }

  .quick-view__hint {
    background: hsl(var(--background, 0 0% 10%) / 90%);
  }
}
</style>
