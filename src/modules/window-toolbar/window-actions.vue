<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
Copyright © 2026 Cortexist, LLC (modifications). All rights reserved.
-->

<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window';
import { MinusIcon, SquareIcon, XIcon } from '@lucide/vue';
import { useI18n } from 'vue-i18n';
import {
  Tooltip,
  TooltipTrigger,
  TooltipContent,
} from '@/components/ui/tooltip';

const props = defineProps<{
  /**
   * Auxiliary windows need their own teardown before the window goes away, so they pass it
   * here rather than letting the button hide the window out from under their state.
   */
  closeHandler?: () => void | Promise<void>;
}>();

const { t } = useI18n();
const appWindow = getCurrentWindow();

interface WindowControlSupport {
  minimize?: boolean;
  maximize?: boolean;
}

/**
 * Tiling compositors size and place windows themselves, so minimize and maximize would be
 * dead buttons there. The backend decides once at startup and injects the answer before the
 * first paint, so this is read synchronously: anything asked over IPC would force the
 * titlebar to render before it knew, and briefly show buttons it then had to take away.
 *
 * A missing global means the injection did not run, which is not evidence of a tiling
 * desktop — default to drawing every control rather than withholding one.
 */
const support: WindowControlSupport = (
  window as typeof window & { __SFM_WINDOW_CONTROL_SUPPORT__?: WindowControlSupport }
).__SFM_WINDOW_CONTROL_SUPPORT__ ?? {};

const canMinimize = support.minimize !== false;
const canMaximize = support.maximize !== false;

function minimizeWindow() {
  appWindow.minimize();
}

function maximizeWindow() {
  appWindow.toggleMaximize();
}

function closeWindow() {
  if (props.closeHandler) {
    void props.closeHandler();
    return;
  }

  // `close()` rather than `hide()`, even though the window does end up merely hidden: this
  // asks Tauri to close it, which fires `CloseRequested` on the Rust side, and that handler
  // owns both the hiding and the decision about whether any window is left. Hiding directly
  // from here skipped that decision entirely, so closing the last window from the app's own
  // titlebar left the process running with nothing on screen.
  void appWindow.close();
}
</script>

<template>
  <div class="window-actions">
    <Tooltip v-if="canMinimize">
      <TooltipTrigger as-child>
        <div
          class="window-toolbar-button"
          @click="minimizeWindow"
        >
          <MinusIcon
            class="window-toolbar-button-icon"
            :size="16"
          />
        </div>
      </TooltipTrigger>
      <TooltipContent>
        {{ t('window.minimizeWindow') }}
      </TooltipContent>
    </Tooltip>
    <Tooltip v-if="canMaximize">
      <TooltipTrigger as-child>
        <div
          class="window-toolbar-button"
          @click="maximizeWindow"
        >
          <SquareIcon
            class="window-toolbar-button-icon"
            :size="14"
          />
        </div>
      </TooltipTrigger>
      <TooltipContent>
        {{ t('window.toggleWindowSize') }}
      </TooltipContent>
    </Tooltip>
    <Tooltip>
      <TooltipTrigger as-child>
        <div
          class="window-toolbar-button window-toolbar-button--red"
          @click="closeWindow"
        >
          <XIcon
            class="window-toolbar-button-icon"
            :size="18"
          />
        </div>
      </TooltipTrigger>
      <TooltipContent>
        {{ t('window.closeWindow') }}
      </TooltipContent>
    </Tooltip>
  </div>
</template>

<style scoped>
.window-actions {
  z-index: 2;
  display: flex;
}

.window-toolbar-button {
  display: inline-flex;
  width: 46px;
  height: var(--window-toolbar-height);
  align-items: center;
  justify-content: center;
  cursor: pointer;
  user-select: none;
}

.window-toolbar-button:hover {
  background: hsl(var(--foreground) / 10%);
}

.window-toolbar-button-icon {
  stroke: hsl(var(--foreground) / 50%);
}

.window-toolbar-button--red:hover {
  background: hsl(var(--destructive) / 50%);
}
</style>
