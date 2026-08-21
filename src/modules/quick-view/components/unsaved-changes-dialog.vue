<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';

/**
 * The one question Quick View's editor asks: what to do with edits about to be lost. It is
 * asked by the window, of the window, so it behaves the same whoever opened it — the file
 * manager, or another application using Quick View as its viewer. Escape, the overlay and
 * the close cross all mean "cancel", the safe answer.
 */
defineProps<{
  open: boolean;
  fileName: string;
}>();

const emit = defineEmits<{
  save: [];
  discard: [];
  cancel: [];
}>();

const { t } = useI18n();

function handleOpenChange(isOpen: boolean) {
  if (!isOpen) {
    emit('cancel');
  }
}
</script>

<template>
  <Dialog
    :open="open"
    @update:open="handleOpenChange"
  >
    <DialogContent class="unsaved-changes-dialog">
      <DialogHeader>
        <DialogTitle>{{ t('quickView.unsavedChangesTitle') }}</DialogTitle>
        <DialogDescription>
          {{ t('quickView.unsavedChangesDescription', { fileName }) }}
        </DialogDescription>
      </DialogHeader>

      <DialogFooter>
        <Button
          variant="outline"
          @click="emit('cancel')"
        >
          {{ t('cancel') }}
        </Button>
        <Button
          variant="outline"
          @click="emit('discard')"
        >
          {{ t('quickView.discardText') }}
        </Button>
        <Button @click="emit('save')">
          {{ t('quickView.saveText') }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
.unsaved-changes-dialog {
  max-width: 28rem;
}
</style>
